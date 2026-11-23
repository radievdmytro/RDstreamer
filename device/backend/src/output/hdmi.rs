use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app::AppSrc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use super::decoded::{SharedDecoder, Subscription};
use super::display::{Display, ModeInfo};
use super::hotplug;
use crate::input::h264;
use crate::output::overlay::{self, Overlay, Rect};
use crate::settings::{DisplaySettings, Scaling};

// kmssink takes the shared decoder's YUV dmabufs straight onto a display
// plane. A v4l2convert here turned every frame into 32-bit RGB first, which
// capped HDMI at ~20 fps once the goggles went to 60 fps. vc4 is an atomic
// driver whose commit already waits for vblank; without skip-vsync kmssink
// waited a second time, 30 fps at most. Only the newest picture waits for
// the screen, so the decoder never does
const PIPELINE: &str = "appsrc name=src is-live=true block=false format=time \
    leaky-type=downstream max-buffers=1 ! \
    {crop}kmssink name=sink sync=false skip-vsync=true";

// With a look: the image processor copies each picture into its own RGBA
// buffer (straight from the decoder, the GPU held on to the decoder's few
// buffers and it stalled after 16 pictures), the GPU grades it, and the
// result goes to the screen as a dmabuf, without the processor touching it.
// Tried on a Pi 4: 1080p manages about 48 fps, 720p a smooth 60
const PIPELINE_LOOK: &str = "appsrc name=src is-live=true block=false format=time \
    leaky-type=downstream max-buffers=1 ! \
    {convert} ! video/x-raw,format=RGBA{size} ! \
    queue max-size-buffers=2 max-size-bytes=0 max-size-time=0 leaky=downstream ! \
    glupload ! rdlook ! gldownload ! video/x-raw(memory:DMABuf) ! \
    {crop}kmssink name=sink sync=false skip-vsync=true";

// DRM plane rotation bits
const ROTATE_0: i32 = 0x1;
const ROTATE_180: i32 = 0x4;
const REFLECT_X: i32 = 0x10;

struct Running {
    pipeline: gst::Pipeline,
    sink: gst::Element,
    /// Grading on the GPU, and whether that has failed
    look: bool,
    failed: Arc<AtomicBool>,
    /// Pictures handed to the pipeline, to tell a frozen screen from no video
    fed: Arc<AtomicU64>,
    /// Dropped before the pipeline stops, so nothing is handed to it
    subscription: Option<Subscription>,
}

impl Running {
    fn stop(mut self) {
        self.subscription.take();
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

struct State {
    /// What the settings ask for, kept while there's no screen to show it on
    wanted: DisplaySettings,
    /// What the screen was last set up with
    cfg: DisplaySettings,
    display: Option<Display>,
    running: Option<Running>,
    /// The look failed on this device: shown without it until its settings
    /// change
    look_failed: bool,
    /// When the output last had to be started again after breaking
    recovered_at: Option<Instant>,
}

pub struct GstPlayer {
    overlay: Arc<Overlay>,
    decoder: Arc<SharedDecoder>,
    state: Mutex<State>,
}

impl GstPlayer {
    pub fn new(
        cfg: &DisplaySettings,
        overlay: Arc<Overlay>,
        decoder: Arc<SharedDecoder>,
    ) -> anyhow::Result<Arc<Self>> {
        gst::init()?;
        let this = Arc::new(Self {
            overlay,
            decoder,
            state: Mutex::new(State {
                wanted: cfg.clone(),
                cfg: DisplaySettings {
                    hdmi_output: false,
                    ..cfg.clone()
                },
                display: None,
                running: None,
                look_failed: false,
                recovered_at: None,
            }),
        });
        // Watching before the first look, so a screen plugged in meanwhile
        // isn't missed
        let watcher = Arc::clone(&this);
        if let Err(e) = hotplug::watch(move || watcher.hotplug()) {
            eprintln!("[hdmi] screens plugged in later won't be noticed: {e}");
        }
        if let Err(e) = this.apply(cfg) {
            eprintln!("[hdmi] not started: {e}");
        }

        let stats = Arc::clone(&this);
        thread::spawn(move || {
            let mut last_rendered: u64 = 0;
            let mut frozen = Frozen::default();
            loop {
                thread::sleep(Duration::from_secs(1));
                stats.watchdog();
                // Also catches the video changing size, e.g. 16:9 to 4:3
                stats.refresh_logo();
                let Some((sink, fed, failed)) = stats.running() else {
                    frozen = Frozen::default();
                    continue;
                };
                let s = sink.property::<gst::Structure>("stats");
                let rendered = s.get::<u64>("rendered").unwrap_or(0);
                let dropped = s.get::<u64>("dropped").unwrap_or(0);
                // Pictures keep arriving but none reach the screen: the
                // pipeline is stuck, and the watchdog starts it again
                if frozen.update(fed, rendered) {
                    eprintln!("[hdmi] the picture froze; starting the output again");
                    failed.store(true, Ordering::Relaxed);
                    frozen = Frozen::default();
                }
                if rendered != last_rendered && crate::settings::verbose() {
                    println!(
                        "[hdmi] {} fps (dropped={})",
                        rendered.saturating_sub(last_rendered),
                        dropped
                    );
                }
                last_rendered = rendered;
            }
        });
        Ok(this)
    }

    /// Everything but a mode change is instant; a mode change also rebuilds
    /// the pipeline, so the picture returns with the next keyframe. With no
    /// screen plugged in, the output starts once one is
    pub fn apply(&self, cfg: &DisplaySettings) -> Result<(), String> {
        let mut st = self.state.lock().unwrap();
        st.wanted = cfg.clone();
        self.show(&mut st)
    }

    /// A pipeline that broke while running is started again on a freshly
    /// set up screen; a look that keeps breaking is dropped for the plain
    /// picture rather than none
    fn watchdog(&self) {
        let mut st = self.state.lock().unwrap();
        let Some(look) = st
            .running
            .as_ref()
            .filter(|r| r.failed.load(Ordering::Relaxed))
            .map(|r| r.look)
        else {
            return;
        };
        let again = st
            .recovered_at
            .is_some_and(|t| t.elapsed() < Duration::from_secs(30));
        if look && again {
            eprintln!("[hdmi] the look failed on the GPU again; showing the picture without it");
            st.look_failed = true;
        } else {
            eprintln!("[hdmi] the output broke; starting it again");
        }
        st.recovered_at = Some(Instant::now());
        if let Some(r) = st.running.take() {
            r.stop();
        }
        st.display = None;
        if let Err(e) = self.show(&mut st) {
            eprintln!("[hdmi] {e}");
        }
    }

    /// Whether the look is wanted and works here
    pub fn look_error(&self) -> Option<String> {
        if let Err(e) = crate::output::look::register() {
            return Some(format!("GPU grading isn't available: {e}"));
        }
        let st = self.state.lock().unwrap();
        st.look_failed.then(|| {
            "The GPU couldn't grade the picture, so HDMI shows it without the look. \
             The Logs page says why."
                .to_string()
        })
    }

    /// A screen came or went somewhere: let go of ours if it's gone or was
    /// swapped for another, then show on whatever is plugged in now
    fn hotplug(&self) {
        let mut st = self.state.lock().unwrap();
        if !st.wanted.hdmi_output {
            return;
        }
        if let Some(display) = &st.display {
            let found = display.probe();
            if found
                .as_ref()
                .is_some_and(|modes| *modes == display.modes())
            {
                return;
            }
            if let Some(r) = st.running.take() {
                r.stop();
            }
            st.display = None;
            eprintln!(
                "[hdmi] screen {}",
                if found.is_some() {
                    "changed"
                } else {
                    "unplugged"
                }
            );
        }
        if let Err(e) = self.show(&mut st) {
            eprintln!("[hdmi] {e}");
        }
    }

    /// Bring the screen in line with what the settings ask for
    fn show(&self, st: &mut State) -> Result<(), String> {
        let cfg = &st.wanted.clone();

        if !cfg.hdmi_output {
            if let Some(r) = st.running.take() {
                r.stop();
                eprintln!("[hdmi] output off");
            }
            // Hands the screen back to the console
            st.display = None;
            st.cfg = cfg.clone();
            return Ok(());
        }

        // Another LUT, strength or dewarp shows on the next picture; turning
        // the GPU stage on or off, or its quality, needs another pipeline
        crate::output::look::select(&cfg.look, &cfg.dewarp);
        if st.cfg.look != cfg.look || st.cfg.dewarp != cfg.dewarp {
            st.look_failed = false;
        }
        // The GPU stage is there for either the look or dewarp
        let look = (cfg.look.enabled || cfg.dewarp.enabled)
            && !st.look_failed
            && crate::output::look::register().is_ok();
        let unchanged = st.running.is_some()
            && st.running.as_ref().is_some_and(|r| r.look == look)
            && st.cfg.look.quality == cfg.look.quality
            && st.cfg.mode == cfg.mode
            && st.cfg.scaling == cfg.scaling
            && st.cfg.rotate_180 == cfg.rotate_180
            && st.cfg.mirror == cfg.mirror;
        if unchanged {
            st.cfg = cfg.clone();
            return Ok(());
        }

        if let Some(r) = st.running.take() {
            r.stop();
        }

        // Swapping one pipeline for another on the same screen sometimes
        // leaves the display in a state the new one can't start in ("Element
        // failed to change its state"); setting the screen up afresh, as a
        // change of resolution does, always brings it back
        match self.start(st, cfg, look) {
            Ok(()) => Ok(()),
            Err(e) => {
                eprintln!("[hdmi] {e}; setting the screen up afresh");
                st.display = None;
                thread::sleep(Duration::from_millis(300));
                self.start(st, cfg, look)
            }
        }
    }

    /// Opens the screen if it isn't yet, and starts a pipeline on it
    fn start(&self, st: &mut State, cfg: &DisplaySettings, look: bool) -> Result<(), String> {
        let fresh = st.display.is_none();
        if fresh {
            let Some(display) = Display::open()? else {
                eprintln!("[hdmi] no screen connected, waiting for one");
                return Ok(());
            };
            st.display = Some(display);
        }
        let mode_changed = fresh || st.cfg.mode != cfg.mode;
        let display = st.display.as_mut().expect("opened above");
        if mode_changed && let Err(e) = display.set_mode(&cfg.mode) {
            // Otherwise the next try would take the mode as already set
            st.display = None;
            return Err(e);
        }
        let mode = display
            .current()
            .cloned()
            .ok_or("The screen has no active mode")?;

        let (mut running, appsrc) = build(cfg, &mode, display.fd(), look)?;
        if let Err(e) = running.pipeline.set_state(gst::State::Playing) {
            running.stop();
            return Err(format!("Couldn't start HDMI output: {e}"));
        }
        // Carries the caps along, so the screen follows a change of size
        let fed = Arc::clone(&running.fed);
        match self.decoder.subscribe(move |sample| {
            fed.fetch_add(1, Ordering::Relaxed);
            let _ = appsrc.push_sample(sample);
        }) {
            Ok(s) => running.subscription = Some(s),
            Err(e) => {
                running.stop();
                return Err(format!("Couldn't start HDMI output: {e}"));
            }
        }
        if st.cfg.hdmi_output != cfg.hdmi_output || fresh {
            eprintln!("[hdmi] output on");
        }
        st.running = Some(running);
        st.cfg = cfg.clone();
        Ok(())
    }

    /// The connected screen's modes, and the one in use
    pub fn modes(&self) -> Result<(Vec<ModeInfo>, Option<ModeInfo>), String> {
        let st = self.state.lock().unwrap();
        if let Some(d) = &st.display {
            return Ok((d.modes(), d.current().cloned()));
        }
        drop(st);
        // Output is off, or waiting: look without taking the screen from the
        // console
        let d = Display::open()?.ok_or("No screen connected to HDMI")?;
        Ok((d.modes(), None))
    }

    pub fn refresh_logo(&self) {
        let mut st = self.state.lock().unwrap();
        let State {
            cfg,
            display,
            running,
            ..
        } = &mut *st;
        let Some(display) = display.as_mut() else {
            return;
        };
        let target = (|| {
            // Already in the picture: drawing it again would double it
            if self.overlay.burned() {
                return None;
            }
            let (logo, overlay_cfg) = self.overlay.active()?;
            let screen = display.current()?;
            let caps = running.as_ref()?.sink.static_pad("sink")?.current_caps()?;
            let s = caps.structure(0)?;
            let video = (s.get::<i32>("width").ok()?, s.get::<i32>("height").ok()?);
            let screen = (screen.width as u32, screen.height as u32);
            let area = video_area(screen, (video.0 as u32, video.1 as u32), cfg.scaling);
            let mut at = overlay::place(area, (logo.width, logo.height), &overlay_cfg);
            // The video plane is turned in hardware; the logo goes with it
            if cfg.rotate_180 {
                at.x = screen.0 as i32 - at.x - at.w as i32;
                at.y = screen.1 as i32 - at.y - at.h as i32;
            }
            if cfg.mirror {
                at.x = screen.0 as i32 - at.x - at.w as i32;
            }
            Some((logo, overlay_cfg.opacity_percent, at))
        })();
        match target {
            Some((logo, opacity, at)) => {
                let rotation = plane_rotation(cfg) as u64;
                if let Err(e) =
                    display.show_logo(&logo, self.overlay.version(), at, opacity, rotation)
                {
                    eprintln!("[hdmi] {e}");
                }
            }
            None => display.hide_logo(),
        }
    }

    /// Pictures shown and dropped since the output started
    pub fn frames(&self) -> Option<(u64, u64)> {
        let stats = self.sink()?.property::<gst::Structure>("stats");
        Some((
            stats.get::<u64>("rendered").unwrap_or(0),
            stats.get::<u64>("dropped").unwrap_or(0),
        ))
    }

    /// The running pipeline's screen, pictures fed and failure flag
    fn running(&self) -> Option<(gst::Element, u64, Arc<AtomicBool>)> {
        let st = self.state.lock().unwrap();
        let r = st.running.as_ref()?;
        Some((
            r.sink.clone(),
            r.fed.load(Ordering::Relaxed),
            Arc::clone(&r.failed),
        ))
    }

    fn sink(&self) -> Option<gst::Element> {
        let st = self.state.lock().unwrap();
        st.running.as_ref().map(|r| r.sink.clone())
    }
}

fn plane_rotation(cfg: &DisplaySettings) -> i32 {
    let mut rotation = if cfg.rotate_180 { ROTATE_180 } else { ROTATE_0 };
    if cfg.mirror {
        rotation |= REFLECT_X;
    }
    rotation
}

/// Where kmssink puts the picture: centred and fitted, or across the whole
/// screen when filling (cropped edges fall outside it)
fn video_area(screen: (u32, u32), video: (u32, u32), scaling: Scaling) -> Rect {
    let full = Rect {
        x: 0,
        y: 0,
        w: screen.0,
        h: screen.1,
    };
    if scaling == Scaling::Fill || video.0 == 0 || video.1 == 0 {
        return full;
    }
    // Compare shapes without floats: video wider than the screen?
    if video.0 as u64 * screen.1 as u64 >= video.1 as u64 * screen.0 as u64 {
        let h = (screen.0 as u64 * video.1 as u64 / video.0 as u64) as u32;
        Rect {
            x: 0,
            y: ((screen.1 - h.min(screen.1)) / 2) as i32,
            w: screen.0,
            h,
        }
    } else {
        let w = (screen.1 as u64 * video.0 as u64 / video.1 as u64) as u32;
        Rect {
            x: ((screen.0 - w.min(screen.0)) / 2) as i32,
            y: 0,
            w,
            h: screen.1,
        }
    }
}

/// Tells a frozen screen from one with no video: pictures fed but none shown
/// for a few seconds in a row
#[derive(Default)]
pub(crate) struct Frozen {
    fed: u64,
    rendered: u64,
    seconds: u32,
}

impl Frozen {
    const SECONDS: u32 = 3;

    /// One reading a second; true once it has been frozen long enough
    pub(crate) fn update(&mut self, fed: u64, rendered: u64) -> bool {
        let fed_now = fed.saturating_sub(self.fed);
        let shown = rendered != self.rendered;
        self.fed = fed;
        self.rendered = rendered;
        // A dozen pictures a second at least: real video, not the splash
        if fed_now >= 10 && !shown {
            self.seconds += 1;
        } else {
            self.seconds = 0;
        }
        self.seconds >= Self::SECONDS
    }
}

fn build(
    cfg: &DisplaySettings,
    mode: &ModeInfo,
    fd: i32,
    look: bool,
) -> Result<(Running, AppSrc), String> {
    // Fill crops in the display hardware: aspectratiocrop only attaches crop
    // metadata, which kmssink turns into the plane's source rectangle
    let crop = match cfg.scaling {
        Scaling::Fit => String::new(),
        Scaling::Fill => format!(
            "aspectratiocrop aspect-ratio={}/{} ! ",
            mode.width, mode.height
        ),
    };
    let description = if look {
        // The Pi's image processor; elsewhere, the processor does it
        let isp = gst::ElementFactory::find("v4l2convert").is_some();
        let convert = if isp {
            "v4l2convert output-io-mode=dmabuf-import"
        } else {
            "videoconvertscale"
        };
        crate::output::look::VIDEO_LEVELS.store(isp, Ordering::Relaxed);
        let size = match cfg.look.quality {
            crate::settings::LookQuality::Speed => ",height=720,pixel-aspect-ratio=1/1",
            crate::settings::LookQuality::Quality => "",
        };
        PIPELINE_LOOK
            .replace("{convert}", convert)
            .replace("{size}", size)
            .replace("{crop}", &crop)
    } else {
        PIPELINE.replace("{crop}", &crop)
    };
    let pipeline = gst::parse::launch(&description)
        .map_err(|e| format!("HDMI pipeline: {e}"))?
        .dynamic_cast::<gst::Pipeline>()
        .expect("Failed to cast to Pipeline");

    let appsrc = pipeline
        .by_name("src")
        .expect("Failed to find appsrc")
        .dynamic_cast::<AppSrc>()
        .expect("Failed to cast to AppSrc");
    appsrc.set_stream_type(gstreamer_app::AppStreamType::Stream);
    appsrc.set_latency(gst::ClockTime::ZERO, gst::ClockTime::NONE);

    let sink = pipeline.by_name("sink").expect("Failed to find kmssink");
    sink.set_property("fd", fd);
    // Always set, or the plane keeps the last pipeline's rotation
    sink.set_property(
        "plane-properties",
        gst::Structure::builder("s")
            .field("rotation", plane_rotation(cfg))
            .build(),
    );

    let failed = Arc::new(AtomicBool::new(false));
    if let Some(bus) = pipeline.bus() {
        let failed = Arc::clone(&failed);
        bus.set_sync_handler(move |_, msg| {
            use gst::MessageView;
            match msg.view() {
                MessageView::Error(err) => {
                    eprintln!("[hdmi] error: {} ({:?})", err.error(), err.debug());
                    failed.store(true, Ordering::Relaxed);
                }
                MessageView::Warning(w) => {
                    eprintln!("[hdmi] warning: {} ({:?})", w.error(), w.debug());
                }
                _ => (),
            }
            gst::BusSyncReply::Drop
        });
    }

    Ok((
        Running {
            pipeline,
            sink,
            look,
            failed,
            fed: Arc::new(AtomicU64::new(0)),
            subscription: None,
        },
        appsrc,
    ))
}

/// Patch SPS level for the Raspberry Pi 4's H.264 decoder, which is limited to level 4.2
pub(crate) fn patch_sps_level(data: &mut [u8]) {
    let Some(sps) = h264::nals(data).find(|nal| nal.kind == h264::SPS) else {
        return;
    };
    // Raising this to e.g. 5.2 to lift 4.2's ~16fps/4K macroblock-rate cap was
    // tried and made the decoder fail outright - no picture at all
    if let Some(level) = data.get_mut(sps.header + 3) {
        *level = 42; // H.264 level 4.2
    }
}

impl Drop for GstPlayer {
    fn drop(&mut self) {
        if let Some(r) = self.state.lock().unwrap().running.take() {
            r.stop();
        }
    }
}

#[cfg(test)]
#[path = "hdmi.test.rs"]
mod tests;

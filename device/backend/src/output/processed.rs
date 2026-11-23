//! The HDMI look (LUT, lens correction, stabilisation) in the streams and
//! browsers too, when asked for: the decoded pictures through the same GPU
//! shader, then the hardware encoder, at up to 1080p30. While it runs and
//! produces, the network outputs take its pictures instead of the goggles'
//! own (recording keeps the original); if it stops producing they get the
//! original again within a second.

use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app::{AppSink, AppSrc};

use crate::output::decoded::{SharedDecoder, Subscription, Thinner};
use crate::settings::DisplaySettings;

const MAX_FPS: u32 = 30;
const BITRATE: u32 = 8_000_000;
/// A keyframe a second, for viewers joining and recovering
const KEYFRAME_EVERY: u32 = MAX_FPS;
/// With no picture out for this long, the original goes out again
const STALE_MS: u64 = 1000;
/// With nothing out this long after starting, the next way is tried
const FIRST_PICTURE_MS: u64 = 5000;
/// Starts every picture, which is how the browsers' assembler finds them
const AUD: [u8; 6] = [0, 0, 0, 1, 9, 0xf0];

type Output = Box<dyn Fn(&[u8]) + Send + Sync>;

/// Which outputs take the processed picture
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Targets {
    pub rtsp: bool,
    pub srt: bool,
    pub rtmp: bool,
    pub udp: bool,
    pub web: bool,
}

impl Targets {
    pub fn of(s: &crate::settings::Settings) -> Self {
        Self {
            rtsp: s.rtsp.processed,
            srt: s.srt.processed,
            rtmp: s.rtmp.processed,
            udp: s.udp.processed,
            web: s.viewer.processed && s.viewer.grade_in == crate::settings::GradeIn::Pi,
        }
    }

    pub fn any(&self) -> bool {
        self.rtsp || self.srt || self.rtmp || self.udp || self.web
    }
}

pub struct Processed {
    decoder: Arc<SharedDecoder>,
    targets: Mutex<Targets>,
    running: Mutex<Option<Encoding>>,
    output: Mutex<Option<Arc<Output>>>,
    /// When the last picture came out, in ms since `epoch`
    last_out: Arc<AtomicU64>,
    epoch: Instant,
    /// Which of `WAYS` it runs (or tries) now; past the end, none worked
    way: AtomicU8,
    /// Each start counts up; a failure only counts for the start it's from
    generation: AtomicU64,
    status: Mutex<ProcessingStatus>,
}

/// From the GPU stage to the encoder, tried in turn until one produces
const WAYS: [(&str, &str); 3] = [
    (
        "zero-copy",
        "gldownload ! video/x-raw(memory:DMABuf),format=RGBA ! v4l2convert output-io-mode=dmabuf-import ! video/x-raw,format=NV12",
    ),
    (
        "copied",
        "gldownload ! video/x-raw,format=RGBA ! v4l2convert ! video/x-raw,format=NV12",
    ),
    (
        "720p, converted on the CPU",
        "gldownload ! video/x-raw,format=RGBA ! videoconvert ! video/x-raw,format=I420",
    ),
];

/// For the web page: whether the processed picture is going out, and if
/// not, why
#[derive(serde::Serialize, Clone, Debug, Default)]
pub struct ProcessingStatus {
    /// off, starting, running or failed
    pub state: &'static str,
    /// Why it's off or failed, or how it runs
    pub detail: String,
    pub targets: TargetsView,
}

#[derive(serde::Serialize, Clone, Debug, Default)]
pub struct TargetsView {
    pub rtsp: bool,
    pub srt: bool,
    pub rtmp: bool,
    pub udp: bool,
    pub web: bool,
}

struct Encoding {
    stabilise: bool,
    delay_ms: u16,
    pipeline: gst::Pipeline,
    subscription: Option<Subscription>,
    _steady: Option<crate::output::look::stab::Hold>,
}

impl Drop for Encoding {
    fn drop(&mut self) {
        self.subscription.take();
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

impl Processed {
    pub fn new(decoder: Arc<SharedDecoder>) -> Arc<Self> {
        Arc::new(Self {
            decoder,
            targets: Mutex::new(Targets::default()),
            running: Mutex::new(None),
            output: Mutex::new(None),
            last_out: Arc::new(AtomicU64::new(0)),
            epoch: Instant::now(),
            way: AtomicU8::new(0),
            generation: AtomicU64::new(0),
            status: Mutex::new(ProcessingStatus {
                state: "off",
                detail: "Not started yet".into(),
                targets: TargetsView::default(),
            }),
        })
    }

    /// Where its pictures go: the network outputs
    pub fn set_output(&self, output: impl Fn(&[u8]) + Send + Sync + 'static) {
        *self.output.lock().unwrap() = Some(Arc::new(Box::new(output)));
    }

    pub fn set_targets(&self, t: Targets) {
        *self.targets.lock().unwrap() = t;
    }

    /// The outputs that should take its pictures now: none while it isn't
    /// producing
    pub fn taking(&self) -> Targets {
        if self.active() {
            *self.targets.lock().unwrap()
        } else {
            Targets::default()
        }
    }

    pub fn status(&self) -> ProcessingStatus {
        let mut st = self.status.lock().unwrap().clone();
        let t = *self.targets.lock().unwrap();
        st.targets = TargetsView {
            rtsp: t.rtsp,
            srt: t.srt,
            rtmp: t.rtmp,
            udp: t.udp,
            web: t.web,
        };
        if st.state == "starting" && self.active() {
            st.state = "running";
        }
        st
    }

    fn set_status(&self, state: &'static str, detail: impl Into<String>) {
        let mut st = self.status.lock().unwrap();
        st.state = state;
        st.detail = detail.into();
    }

    /// Whether it's producing
    pub fn active(&self) -> bool {
        // Not through `running`: its lock is held while a pipeline stops,
        // which waits for the pictures in flight, which ask this
        let last = self.last_out.load(Ordering::Relaxed);
        let now = self.epoch.elapsed().as_millis() as u64;
        last > 0 && now.saturating_sub(last) < STALE_MS
    }

    /// Runs or stops with the settings
    pub fn apply(self: &Arc<Self>, cfg: &DisplaySettings) {
        let targets = self.targets.lock().unwrap().any();
        let look = cfg.look.enabled || cfg.dewarp.enabled || cfg.stabilization.enabled;
        let hardware = self.decoder.hardware();
        let wanted = targets && look && hardware;
        let mut running = self.running.lock().unwrap();
        let same = match &*running {
            Some(e) => {
                wanted
                    && e.stabilise == cfg.stabilization.enabled
                    && e.delay_ms == crate::output::look::delay_of(cfg)
            }
            None => !wanted,
        };
        if same {
            return;
        }
        *running = None;
        self.last_out.store(0, Ordering::Relaxed);
        if !wanted {
            println!("[processed] streams get the goggles' own video");
            self.set_status(
                "off",
                if !targets {
                    "No output has Processing on"
                } else if !look {
                    "LUT, Dewarp and Stabilisation are all off on the Display page"
                } else {
                    "Needs the Pi's hardware decoder"
                },
            );
            return;
        }
        if let Err(e) = crate::output::look::register() {
            eprintln!("[processed] no GPU stage here");
            self.set_status("failed", format!("No GPU stage: {e}"));
            return;
        }
        // Settings changed: the best way first again
        self.way.store(0, Ordering::Relaxed);
        self.run(
            &mut running,
            cfg.stabilization.enabled,
            crate::output::look::delay_of(cfg),
        );
    }

    /// Starts the current way, or the next ones if it can't
    fn run(self: &Arc<Self>, running: &mut Option<Encoding>, stabilise: bool, delay_ms: u16) {
        loop {
            let way = self.way.load(Ordering::Relaxed) as usize;
            let Some((name, _)) = WAYS.get(way) else {
                return;
            };
            match self.start(stabilise, delay_ms, way) {
                Ok(e) => {
                    println!("[processed] streams get the HDMI look, encoded again ({name})");
                    self.set_status("starting", format!("Encoding ({name})"));
                    *running = Some(e);
                    return;
                }
                Err(e) => {
                    eprintln!("[processed] {name}: {e}");
                    self.fail(way, &e);
                }
            }
        }
    }

    /// The way `way` didn't work: the next one, or none if it was the last
    fn fail(&self, way: usize, why: &str) {
        let _ = self.way.compare_exchange(
            way as u8,
            way as u8 + 1,
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
        if way + 1 >= WAYS.len() {
            self.set_status(
                "failed",
                format!("Couldn't run, the original goes out instead. Last error: {why}"),
            );
        }
    }

    fn start(
        self: &Arc<Self>,
        stabilise: bool,
        delay_ms: u16,
        way: usize,
    ) -> Result<Encoding, String> {
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let download = WAYS[way].1;
        // The first two ways convert through the image processor
        let name = if way < 2 {
            format!("{}0", crate::output::look::TO_ISP)
        } else {
            "rdlook_cpu".to_string()
        };
        // The CPU can't convert 1080p in time: the image processor makes it
        // 720p on the way in
        let size = if way == 2 {
            ",height=720,pixel-aspect-ratio=1/1"
        } else {
            ""
        };
        let mut cfg = crate::settings::DisplaySettings::default();
        cfg.stabilization.enabled = delay_ms > 0;
        cfg.stabilization.delay_ms = delay_ms;
        let (format, delay) = crate::output::look::delay_stage(&cfg);
        let own = if delay.is_empty() {
            ""
        } else {
            crate::output::look::OWN_BUFFERS
        };
        let description = format!(
            "appsrc name=src is-live=true format=time max-buffers=2 leaky-type=downstream ! \
             v4l2convert output-io-mode=dmabuf-import {own} ! video/x-raw,format={format}{size} ! \
             queue max-size-buffers=2 max-size-bytes=0 max-size-time=0 leaky=downstream ! \
             glupload ! {delay}rdlook name={name} ! {download} ! \
             v4l2h264enc extra-controls=\"controls,video_bitrate={BITRATE},h264_i_frame_period={KEYFRAME_EVERY},repeat_sequence_header=1\" ! \
             video/x-h264,level=(string)4,profile=(string)high ! \
             h264parse config-interval=-1 ! video/x-h264,stream-format=byte-stream,alignment=au ! \
             appsink name=sink sync=false max-buffers=4 drop=true"
        );
        let pipeline = gst::parse::launch(&description)
            .map_err(|e| format!("processed stream: {e}"))?
            .dynamic_cast::<gst::Pipeline>()
            .expect("a pipeline");
        let appsrc = pipeline
            .by_name("src")
            .and_then(|e| e.dynamic_cast::<AppSrc>().ok())
            .expect("appsrc");

        let output = self.output.lock().unwrap().clone();
        let last_out = Arc::clone(&self.last_out);
        let epoch = self.epoch;
        pipeline
            .by_name("sink")
            .and_then(|e| e.dynamic_cast::<AppSink>().ok())
            .expect("appsink")
            .set_callbacks(
                gstreamer_app::AppSinkCallbacks::builder()
                    .new_sample(move |sink| {
                        let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                        if let (Some(out), Some(map)) =
                            (&output, sample.buffer().and_then(|b| b.map_readable().ok()))
                        {
                            let mut au = Vec::with_capacity(map.len() + AUD.len());
                            au.extend_from_slice(&AUD);
                            au.extend_from_slice(map.as_slice());
                            last_out.store(epoch.elapsed().as_millis() as u64, Ordering::Relaxed);
                            out(&au);
                        }
                        Ok(gst::FlowSuccess::Ok)
                    })
                    .build(),
            );

        // A failure, or nothing out at all, moves on to the next way
        let this = Arc::downgrade(self);
        if let Some(bus) = pipeline.bus() {
            bus.set_sync_handler(move |_, msg| {
                if let gst::MessageView::Error(e) = msg.view() {
                    let why = format!("{} ({:?})", e.error(), e.debug());
                    eprintln!("[processed] {why}");
                    if let Some(this) = this.upgrade() {
                        std::thread::spawn(move || {
                            this.retry(generation, way, &why, stabilise, delay_ms)
                        });
                    }
                }
                gst::BusSyncReply::Drop
            });
        }
        {
            let this = Arc::downgrade(self);
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(FIRST_PICTURE_MS));
                if let Some(this) = this.upgrade()
                    && this.generation.load(Ordering::Relaxed) == generation
                    && this.last_out.load(Ordering::Relaxed) == 0
                {
                    eprintln!("[processed] {}: nothing came out", WAYS[way].0);
                    this.retry(
                        generation,
                        way,
                        "no picture came out of the encoder",
                        stabilise,
                        delay_ms,
                    );
                }
            });
        }
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| format!("processed stream: {e}"))?;

        let started = Instant::now();
        let mut thinner = Thinner::new(MAX_FPS);
        let subscription = self.decoder.subscribe(move |sample| {
            // Thinned on arrival; the decoder's timestamps kept, so the
            // stabilisation finds each picture's correction
            if thinner.keep(started.elapsed().as_secs_f64()) {
                let _ = appsrc.push_sample(sample);
            }
        });
        let steady = if stabilise {
            crate::output::look::stab::start(&self.decoder).ok()
        } else {
            None
        };
        match subscription {
            Ok(s) => Ok(Encoding {
                stabilise,
                delay_ms,
                pipeline,
                subscription: Some(s),
                _steady: steady,
            }),
            Err(e) => {
                let _ = pipeline.set_state(gst::State::Null);
                Err(e)
            }
        }
    }

    /// The run from start `generation` failed: the next way, unless the
    /// settings have started another run since
    fn retry(
        self: Arc<Self>,
        generation: u64,
        way: usize,
        why: &str,
        stabilise: bool,
        delay_ms: u16,
    ) {
        let mut running = self.running.lock().unwrap();
        if self.generation.load(Ordering::Relaxed) != generation || running.is_none() {
            return;
        }
        *running = None;
        self.last_out.store(0, Ordering::Relaxed);
        self.fail(way, why);
        self.run(&mut running, stabilise, delay_ms);
    }
}

/// Everything the processed picture doesn't go to
impl std::ops::Not for Targets {
    type Output = Targets;
    fn not(self) -> Targets {
        Targets {
            rtsp: !self.rtsp,
            srt: !self.srt,
            rtmp: !self.rtmp,
            udp: !self.udp,
            web: !self.web,
        }
    }
}

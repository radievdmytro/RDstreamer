//! The HDMI look (LUT, lens correction, stabilisation) in the streams and
//! browsers too, when asked for: the decoded pictures through the same GPU
//! shader, then the hardware encoder, at up to 1080p30. While it runs and
//! produces, the network outputs take its pictures instead of the goggles'
//! own (recording keeps the original); if it stops producing they get the
//! original again within a second.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
            web: s.viewer.processed,
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
    /// The GPU's picture couldn't be handed to the encoder as is: copied
    copy: AtomicBool,
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
            copy: AtomicBool::new(false),
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

    /// Whether it's producing
    pub fn active(&self) -> bool {
        self.running.lock().unwrap().is_some() && {
            let last = self.last_out.load(Ordering::Relaxed);
            let now = self.epoch.elapsed().as_millis() as u64;
            last > 0 && now.saturating_sub(last) < STALE_MS
        }
    }

    /// Runs or stops with the settings
    pub fn apply(self: &Arc<Self>, cfg: &DisplaySettings) {
        let wanted = self.targets.lock().unwrap().any()
            && (cfg.look.enabled || cfg.dewarp.enabled || cfg.stabilization.enabled)
            && self.decoder.hardware();
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
            return;
        }
        if crate::output::look::register().is_err() {
            eprintln!("[processed] no GPU stage here");
            return;
        }
        match self.start(
            cfg.stabilization.enabled,
            crate::output::look::delay_of(cfg),
        ) {
            Ok(e) => {
                println!("[processed] streams get the HDMI look, encoded again");
                *running = Some(e);
            }
            Err(e) => eprintln!("[processed] {e}"),
        }
    }

    fn start(self: &Arc<Self>, stabilise: bool, delay_ms: u16) -> Result<Encoding, String> {
        // The GPU's picture straight to the image processor, which feeds the
        // encoder; through the processor's memory when that won't take it
        let download = if self.copy.load(Ordering::Relaxed) {
            "gldownload ! video/x-raw,format=RGBA ! v4l2convert"
        } else {
            "gldownload ! video/x-raw(memory:DMABuf),format=RGBA ! v4l2convert output-io-mode=dmabuf-import"
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
             v4l2convert output-io-mode=dmabuf-import {own} ! video/x-raw,format={format} ! \
             queue max-size-buffers=2 max-size-bytes=0 max-size-time=0 leaky=downstream ! \
             glupload ! {delay}rdlook ! {download} ! video/x-raw,format=NV12 ! \
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

        // A failure here, with the picture handed over as is, is tried once
        // more copying it
        let this = Arc::downgrade(self);
        if let Some(bus) = pipeline.bus() {
            bus.set_sync_handler(move |_, msg| {
                if let gst::MessageView::Error(e) = msg.view() {
                    eprintln!("[processed] {} ({:?})", e.error(), e.debug());
                    if let Some(this) = this.upgrade()
                        && !this.copy.swap(true, Ordering::Relaxed)
                    {
                        std::thread::spawn(move || this.restart(stabilise, delay_ms));
                    }
                }
                gst::BusSyncReply::Drop
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

    fn restart(self: Arc<Self>, stabilise: bool, delay_ms: u16) {
        let mut running = self.running.lock().unwrap();
        if running.take().is_none() {
            return;
        }
        eprintln!("[processed] trying again, copying the GPU's picture");
        match self.start(stabilise, delay_ms) {
            Ok(e) => *running = Some(e),
            Err(e) => eprintln!("[processed] {e}"),
        }
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

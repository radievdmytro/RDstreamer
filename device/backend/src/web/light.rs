//! A lighter copy of the video for browsers on weak WiFi: the shared
//! decoder's pictures scaled to 720p, thinned to 30 fps and encoded again by
//! the Pi's hardware encoder at a few Mbit/s, a fraction of the goggles'.
//! HDMI, recording and the streams keep the original. It only runs while a
//! browser watches it.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app::{AppSink, AppSrc};
use tokio::sync::broadcast;

use super::rtc::AccessUnit;
use crate::input::h264;
use crate::output::decoded::{SharedDecoder, Subscription, Thinner, fit};

/// About 3.5 Mbit/s: 720p30 looks clean at it, and 2.4 GHz WiFi carries it
/// for several viewers where the original strains for one
pub const BITRATE: u32 = 3_500_000;
const MAX_HEIGHT: u32 = 720;
const MAX_FPS: u32 = 30;
/// A keyframe every second: a viewer joins or recovers within one
const KEYFRAME_EVERY: u32 = MAX_FPS;

pub struct LightStream {
    decoder: Arc<SharedDecoder>,
    tx: broadcast::Sender<AccessUnit>,
    viewers: Arc<AtomicUsize>,
    running: Mutex<Option<Encoding>>,
}

/// Held by a viewer of the light stream; the encoder stops after the last
pub struct Watching {
    stream: Arc<LightStream>,
}

impl Drop for Watching {
    fn drop(&mut self) {
        if self.stream.viewers.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.stream.running.lock().unwrap().take();
            eprintln!("[light] nobody watching, encoder stopped");
        }
    }
}

struct Encoding {
    pipeline: gst::Pipeline,
    /// Dropped first, so nothing is handed to a pipeline being stopped
    subscription: Option<Subscription>,
}

impl Drop for Encoding {
    fn drop(&mut self) {
        self.subscription.take();
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

impl LightStream {
    pub fn new(decoder: Arc<SharedDecoder>) -> Arc<Self> {
        let (tx, _) = broadcast::channel(256);
        Arc::new(Self {
            decoder,
            tx,
            viewers: Arc::new(AtomicUsize::new(0)),
            running: Mutex::new(None),
        })
    }

    /// The light stream's pictures, or why there are none (the caller then
    /// sends the original)
    pub fn watch(self: &Arc<Self>) -> Result<(broadcast::Receiver<AccessUnit>, Watching), String> {
        let rx = self.tx.subscribe();
        self.viewers.fetch_add(1, Ordering::SeqCst);
        let watching = Watching {
            stream: Arc::clone(self),
        };
        let mut running = self.running.lock().unwrap();
        if running.is_none() {
            *running = Some(self.start()?);
            eprintln!("[light] encoder started for a browser");
        }
        Ok((rx, watching))
    }

    fn start(&self) -> Result<Encoding, String> {
        if !self.decoder.hardware() {
            return Err("no hardware encoder here".into());
        }
        // The image processor scales the decoder's pictures as they are, and
        // the encoder takes the result. repeat_sequence_header: every keyframe
        // can start a viewer
        let description = format!(
            "appsrc name=src is-live=true format=time leaky-type=downstream max-buffers=2 ! \
             v4l2convert name=scale output-io-mode=dmabuf-import ! \
             capsfilter name=size caps=video/x-raw,format=NV12 ! \
             v4l2h264enc extra-controls=\"controls,video_bitrate={BITRATE},h264_i_frame_period={KEYFRAME_EVERY},repeat_sequence_header=1\" ! \
             video/x-h264,level=(string)4,profile=(string)high ! \
             h264parse config-interval=-1 ! \
             video/x-h264,stream-format=byte-stream,alignment=au ! \
             appsink name=sink sync=false max-buffers=4 drop=true"
        );
        let pipeline = gst::parse::launch(&description)
            .map_err(|e| format!("light stream: {e}"))?
            .dynamic_cast::<gst::Pipeline>()
            .expect("a pipeline");
        let appsrc = pipeline
            .by_name("src")
            .and_then(|e| e.dynamic_cast::<AppSrc>().ok())
            .expect("appsrc");

        // The size follows the video's, known only once it's decoded
        let size = pipeline.by_name("size").expect("capsfilter");
        pipeline
            .by_name("scale")
            .and_then(|s| s.static_pad("sink"))
            .expect("scaler")
            .add_probe(gst::PadProbeType::EVENT_DOWNSTREAM, move |_, info| {
                if let Some(gst::PadProbeData::Event(event)) = &info.data
                    && let gst::EventView::Caps(caps) = event.view()
                    && let Some(s) = caps.caps().structure(0)
                    && let (Ok(w), Ok(h)) = (s.get::<i32>("width"), s.get::<i32>("height"))
                {
                    let (w, h) = fit(w.max(0) as u32, h.max(0) as u32, MAX_HEIGHT);
                    size.set_property(
                        "caps",
                        gst::Caps::builder("video/x-raw")
                            .field("format", "NV12")
                            .field("width", w as i32)
                            .field("height", h as i32)
                            .build(),
                    );
                }
                gst::PadProbeReturn::Ok
            });

        let tx = self.tx.clone();
        pipeline
            .by_name("sink")
            .and_then(|e| e.dynamic_cast::<AppSink>().ok())
            .expect("appsink")
            .set_callbacks(
                gstreamer_app::AppSinkCallbacks::builder()
                    .new_sample(move |sink| {
                        let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                        if let Some(map) = sample.buffer().and_then(|b| b.map_readable().ok()) {
                            let data = bytes::Bytes::copy_from_slice(map.as_slice());
                            let _ = tx.send(AccessUnit {
                                keyframe: h264::has_keyframe(&data),
                                data,
                                assembled: Instant::now(),
                            });
                        }
                        Ok(gst::FlowSuccess::Ok)
                    })
                    .build(),
            );

        if let Some(bus) = pipeline.bus() {
            bus.set_sync_handler(|_, msg| {
                if let gst::MessageView::Error(e) = msg.view() {
                    eprintln!("[light] {} ({:?})", e.error(), e.debug());
                }
                gst::BusSyncReply::Drop
            });
        }
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| format!("light stream: {e}"))?;

        // Arrival times as timestamps, like the other decoded outputs: the
        // decoder's own count from whenever it started
        let started = Instant::now();
        let mut thinner = Thinner::new(MAX_FPS);
        let subscription = self.decoder.subscribe(move |sample| {
            let arrived = started.elapsed();
            if !thinner.keep(arrived.as_secs_f64()) {
                return;
            }
            let (Some(mut buffer), Some(caps)) = (sample.buffer_owned(), sample.caps_owned())
            else {
                return;
            };
            let stamped = buffer.make_mut();
            stamped.set_pts(gst::ClockTime::from_nseconds(arrived.as_nanos() as u64));
            stamped.set_dts(gst::ClockTime::NONE);
            let sample = gst::Sample::builder().buffer(&buffer).caps(&caps).build();
            let _ = appsrc.push_sample(&sample);
        });
        match subscription {
            Ok(s) => Ok(Encoding {
                pipeline,
                subscription: Some(s),
            }),
            Err(e) => {
                let _ = pipeline.set_state(gst::State::Null);
                Err(e)
            }
        }
    }
}

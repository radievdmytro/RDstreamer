//! A video loop behind the splash screen instead of the plain background.
//! Uploaded once and turned into a raw H.264 stream the Pi's decoder plays
//! (at most 1080p: its decoder's limit; bigger or other codecs are
//! converted), then played in a loop, paced to its own frame rate, while
//! the status and login details show over it on a display plane. Every
//! picture starts with a delimiter (inserted when converting), which is how
//! pictures are told apart, and every keyframe carries the parameter sets.

use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::input::h264::AccessUnitAssembler;

/// The decoder's limit
const MAX_WIDTH: u32 = 1920;
const MAX_HEIGHT: u32 = 1080;
/// What a converted video is encoded at
const CONVERT_BITRATE: &str = "8M";
const CONVERT_FPS: u32 = 30;
const READ_CHUNK: usize = 64 << 10;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ClipInfo {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    /// Converted (re-encoded) rather than used as it came
    pub converted: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum VideoState {
    None,
    /// Being made ready; `seconds` since it started
    Converting {
        seconds: u64,
    },
    Ready(ClipInfo),
    Failed {
        error: String,
    },
}

/// What ffprobe says about the first video stream
#[derive(Debug, PartialEq)]
pub(crate) struct Probe {
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
}

pub(crate) fn parse_probe(json: &str) -> Option<Probe> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let s = v.get("streams")?.get(0)?;
    let rate = |key: &str| {
        let text = s.get(key)?.as_str()?;
        let (n, d) = text.split_once('/').unwrap_or((text, "1"));
        let (n, d): (f64, f64) = (n.parse().ok()?, d.parse().ok()?);
        (n > 0.0 && d > 0.0).then_some(n / d)
    };
    Some(Probe {
        codec: s.get("codec_name")?.as_str()?.to_string(),
        width: s.get("width")?.as_u64()? as u32,
        height: s.get("height")?.as_u64()? as u32,
        fps: rate("avg_frame_rate")
            .or_else(|| rate("r_frame_rate"))
            .unwrap_or(30.0),
    })
}

/// Used as it is when the Pi can decode it straight away
pub(crate) fn needs_converting(p: &Probe) -> bool {
    p.codec != "h264" || p.width > MAX_WIDTH || p.height > MAX_HEIGHT || p.fps > 61.0
}

fn run(cmd: &mut Command) -> Result<String, String> {
    let out = cmd.output().map_err(|e| format!("couldn't run it: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(err
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("failed")
            .trim()
            .to_string())
    }
}

/// `input` (any video file) into `output`, a raw H.264 stream, with
/// parameter sets before every keyframe so a loop can start anywhere
pub fn convert(input: &Path, output: &Path) -> Result<ClipInfo, String> {
    let probe = run(Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=codec_name,width,height,avg_frame_rate,r_frame_rate",
            "-of",
            "json",
        ])
        .arg(input))
    .map_err(|e| format!("That doesn't look like a video ({e})"))?;
    let probe = parse_probe(&probe).ok_or("That file has no video in it")?;
    let tmp = output.with_extension("tmp");

    let info = if !needs_converting(&probe) {
        run(Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-i"])
            .arg(input)
            .args([
                "-map",
                "0:v:0",
                "-c:v",
                "copy",
                "-bsf:v",
                "h264_mp4toannexb,h264_metadata=aud=insert",
                "-f",
                "h264",
            ])
            .arg(&tmp))
        .map_err(|e| format!("Couldn't read the video: {e}"))?;
        ClipInfo {
            width: probe.width,
            height: probe.height,
            fps: probe.fps,
            converted: false,
        }
    } else {
        // To 1080p at 30 fps, by the hardware encoder; the software one if
        // that isn't there. Slow for 4K: the Pi decodes that in software
        let filter = format!(
            "scale=w={MAX_WIDTH}:h={MAX_HEIGHT}:force_original_aspect_ratio=decrease:force_divisible_by=2,fps={CONVERT_FPS},format=yuv420p"
        );
        let gop = CONVERT_FPS.to_string();
        let encode = |codec: &[&str]| {
            run(Command::new("ffmpeg")
                .args(["-v", "error", "-y", "-i"])
                .arg(input)
                .args(["-map", "0:v:0", "-vf", &filter])
                .args(codec)
                .args([
                    "-b:v",
                    CONVERT_BITRATE,
                    "-g",
                    &gop,
                    "-bsf:v",
                    "dump_extra,h264_metadata=aud=insert",
                    "-f",
                    "h264",
                ])
                .arg(&tmp))
        };
        encode(&["-c:v", "h264_v4l2m2m"])
            .or_else(|_| {
                encode(&[
                    "-c:v",
                    "libx264",
                    "-preset",
                    "veryfast",
                    "-profile:v",
                    "high",
                ])
            })
            .map_err(|e| format!("Couldn't convert the video: {e}"))?;
        let (w, h) = fit(probe.width, probe.height);
        ClipInfo {
            width: w,
            height: h,
            fps: CONVERT_FPS as f64,
            converted: true,
        }
    };
    if fs::metadata(&tmp).map_or(0, |m| m.len()) == 0 {
        let _ = fs::remove_file(&tmp);
        return Err("Nothing came out of the video".into());
    }
    fs::rename(&tmp, output).map_err(|e| e.to_string())?;
    Ok(info)
}

fn fit(w: u32, h: u32) -> (u32, u32) {
    if w <= MAX_WIDTH && h <= MAX_HEIGHT || w == 0 || h == 0 {
        return (w, h);
    }
    let scale = (MAX_WIDTH as f64 / w as f64).min(MAX_HEIGHT as f64 / h as f64);
    (
        ((w as f64 * scale) as u32) & !1,
        ((h as f64 * scale) as u32) & !1,
    )
}

/// Plays the clip in a loop, a picture at a time on its own clock
pub struct Player {
    path: PathBuf,
    reader: Option<BufReader<File>>,
    assembler: AccessUnitAssembler,
    period: Duration,
    next: Instant,
    /// Pictures since the loop last started, to restart cleanly
    pictures: u64,
}

impl Player {
    pub fn new(path: PathBuf, fps: f64) -> Self {
        Self {
            path,
            reader: None,
            assembler: AccessUnitAssembler::new(),
            period: Duration::from_secs_f64(1.0 / fps.clamp(1.0, 60.0)),
            next: Instant::now(),
            pictures: 0,
        }
    }

    /// Hands out pictures for about `span`, each when it's due
    pub fn play_for(&mut self, span: Duration, push: &impl Fn(&[u8])) -> Result<(), String> {
        let until = Instant::now() + span;
        while Instant::now() < until {
            let Some(au) = self.next_picture()? else {
                return Ok(());
            };
            let now = Instant::now();
            if self.next > now {
                std::thread::sleep(self.next - now);
            } else if now - self.next > Duration::from_millis(250) {
                // Fell behind (busy device): carry on from now, no burst
                self.next = now;
            }
            push(&au);
            self.next += self.period;
        }
        Ok(())
    }

    fn next_picture(&mut self) -> Result<Option<Vec<u8>>, String> {
        let mut chunk = vec![0u8; READ_CHUNK];
        // Bounded: a file with no picture in it ends the loop
        for _ in 0..4096 {
            if let Some(au) = self.assembler.next_au() {
                self.pictures += 1;
                return Ok(Some(au));
            }
            if self.reader.is_none() {
                let file = File::open(&self.path).map_err(|e| e.to_string())?;
                self.reader = Some(BufReader::with_capacity(READ_CHUNK, file));
            }
            let n = self
                .reader
                .as_mut()
                .expect("opened above")
                .read(&mut chunk)
                .map_err(|e| e.to_string())?;
            if n == 0 {
                self.reader = None;
                if self.pictures == 0 {
                    return Ok(None);
                }
                self.pictures = 0;
                // A delimiter closes the last picture; then from the top
                self.assembler.push(&[0, 0, 0, 1, 9, 0xf0]);
                continue;
            }
            self.assembler.push(&chunk[..n]);
        }
        Ok(None)
    }
}

#[cfg(test)]
#[path = "video.test.rs"]
mod tests;

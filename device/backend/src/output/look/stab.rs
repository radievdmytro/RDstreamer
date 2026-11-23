//! Video stabilisation from the picture alone: each decoded picture's
//! brightness, shrunk to 160×90, is compared with the one before to find
//! how far it moved (and turned, for roll). The camera's path is smoothed,
//! and the GPU shader shifts and turns each picture by the difference, a
//! little zoomed in so the moved edges stay outside the frame. Cheap enough
//! for the Pi's processor: a few thousand reads and sums a picture.
//!
//! Coordinates are the shader's: y from -1 (top) to 1, x from -aspect to
//! aspect. Motion `m` means this picture at p shows what the last one had
//! at p + m.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use gstreamer as gst;
use gstreamer_video as gst_video;
use gstreamer_video::prelude::*;

use crate::output::decoded::{SharedDecoder, Subscription};

/// Size the pictures are compared at
pub const W: usize = 160;
pub const H: usize = 90;
/// Furthest a picture is searched for between two frames, in small pixels
const SEARCH: i32 = 14;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Motion {
    pub x: f32,
    pub y: f32,
    /// Radians
    pub angle: f32,
}

/// What the shader does to one picture
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Correction {
    /// Sample at R(angle)·p/zoom + offset
    pub offset: [f32; 2],
    pub angle: f32,
    pub zoom: f32,
}

impl Correction {
    pub const NONE: Correction = Correction {
        offset: [0.0, 0.0],
        angle: 0.0,
        zoom: 1.0,
    };
}

/// A picture's brightness at 160×90, and its sums along rows and columns
pub struct Small {
    luma: Vec<f32>,
    /// Of the picture it came from
    aspect: f32,
}

impl Small {
    /// From a luma plane, averaging blocks
    pub fn from_luma(plane: &[u8], stride: usize, width: usize, height: usize) -> Option<Self> {
        if width < W || height < H || plane.len() < stride * (height - 1) + width {
            return None;
        }
        let mut luma = vec![0f32; W * H];
        // A few samples per block are plenty, and keep reads down on
        // uncached video memory
        for by in 0..H {
            for bx in 0..W {
                let mut sum = 0u32;
                for (sx, sy) in [(1, 1), (3, 1), (1, 3), (3, 3)] {
                    let x = (bx * 4 + sx) * width / (W * 4);
                    let y = (by * 4 + sy) * height / (H * 4);
                    sum += plane[y * stride + x] as u32;
                }
                luma[by * W + bx] = sum as f32 / 4.0;
            }
        }
        Some(Self {
            luma,
            aspect: width as f32 / height as f32,
        })
    }

    fn at(&self, x: usize, y: usize) -> f32 {
        self.luma[y * W + x]
    }

    /// Column sums: brightness across the picture
    fn columns(&self) -> Vec<f32> {
        self.columns_between(0, H)
    }

    /// Column sums over a band of rows
    fn columns_between(&self, from: usize, to: usize) -> Vec<f32> {
        let n = (to - from).max(1) as f32;
        (0..W)
            .map(|x| (from..to).map(|y| self.at(x, y)).sum::<f32>() / n)
            .collect()
    }

    /// Row sums over a band of columns
    fn rows(&self, from: usize, to: usize) -> Vec<f32> {
        let n = (to - from).max(1) as f32;
        (0..H)
            .map(|y| (from..to).map(|x| self.at(x, y)).sum::<f32>() / n)
            .collect()
    }
}

fn clamp_w(x: i32) -> usize {
    x.clamp(0, W as i32) as usize
}

fn clamp_h(y: i32) -> usize {
    y.clamp(0, H as i32) as usize
}

/// Shift `s` with b(i) ≈ a(i + s), to a fraction of a pixel, and how well
/// the best match fits (mean absolute difference)
fn shift(a: &[f32], b: &[f32]) -> (f32, f32) {
    let n = a.len() as i32;
    let cost = |s: i32| {
        let mut sum = 0.0;
        let mut count = 0;
        for i in 0..n {
            let j = i + s;
            if (0..n).contains(&j) {
                sum += (b[i as usize] - a[j as usize]).abs();
                count += 1;
            }
        }
        if count == 0 {
            f32::MAX
        } else {
            sum / count as f32
        }
    };
    let costs: Vec<f32> = (-SEARCH..=SEARCH).map(cost).collect();
    let (best, &c0) = costs
        .iter()
        .enumerate()
        .min_by(|x, y| x.1.total_cmp(y.1))
        .expect("not empty");
    let mut s = best as f32 - SEARCH as f32;
    // A parabola through the best and its neighbours, for the fraction
    if best > 0 && best + 1 < costs.len() {
        let (l, r) = (costs[best - 1], costs[best + 1]);
        let d = l - 2.0 * c0 + r;
        if d > 1e-6 {
            s += ((l - r) / (2.0 * d)).clamp(-0.5, 0.5);
        }
    }
    (s, c0)
}

/// How the picture moved between `prev` and `cur`; None when they don't
/// match (a cut, the splash screen, darkness)
pub fn estimate(prev: &Small, cur: &Small, roll: bool) -> Option<Motion> {
    // One small pixel in shader units, across and down
    let unit = 2.0 / H as f32;
    let unit_x = 2.0 * cur.aspect / W as f32;
    // Across first; then down, comparing each band of columns with where
    // it was in the last picture; then across again, the same way down
    let (sx0, _) = shift(&prev.columns_between(0, H), &cur.columns_between(0, H));
    let dx = sx0.round() as i32;
    let band = W / 4;
    let rows = |from: usize, to: usize| {
        let (pf, pt) = (clamp_w(from as i32 + dx), clamp_w(to as i32 + dx));
        let (cf, ct) = (clamp_w(pf as i32 - dx), clamp_w(pt as i32 - dx));
        shift(&prev.rows(pf, pt), &cur.rows(cf, ct))
    };
    let (left, fit_l) = rows(SEARCH as usize, band + SEARCH as usize);
    let (mid, fit_m) = rows(band, W - band);
    let (right, fit_r) = rows(W - band - SEARCH as usize, W - SEARCH as usize);
    let dy = mid.round() as i32;
    let (pf, pt) = (clamp_h(SEARCH + dy), clamp_h(H as i32 - SEARCH + dy));
    let (cf, ct) = (clamp_h(pf as i32 - dy), clamp_h(pt as i32 - dy));
    let (sx, fit_x) = shift(&prev.columns_between(pf, pt), &cur.columns_between(cf, ct));
    // Flat or unrelated pictures: nothing to go by
    let contrast = {
        let c = cur.columns();
        let mean = c.iter().sum::<f32>() / c.len() as f32;
        c.iter().map(|v| (v - mean).abs()).sum::<f32>() / c.len() as f32
    };
    let fit = fit_x.max(fit_m).max(fit_l).max(fit_r);
    if contrast < 1.0 || fit > 12.0 {
        return None;
    }
    // Bands centred a quarter of the width in from each side: left at
    // x = -L, right at +L. Turning by a moves them by -L·a and +L·a
    let l = (W as f32 / 2.0 - band as f32 / 2.0 - SEARCH as f32) * unit_x;
    let angle = if roll {
        (right - left) * unit / (2.0 * l)
    } else {
        0.0
    };
    Some(Motion {
        x: sx * unit_x,
        y: mid * unit,
        angle,
    })
}

/// The camera's path and its smoothed version
#[derive(Default)]
pub struct Path {
    path: Motion,
    smooth: Motion,
}

impl Path {
    /// After one picture's motion: the correction for it. `strength` 0-100
    pub fn step(&mut self, m: Option<Motion>, strength: u8) -> Correction {
        let Some(m) = m else {
            // Lost track: start again from here, without a jump
            self.smooth = self.path;
            return self.correction(strength);
        };
        self.path.x += m.x;
        self.path.y += m.y;
        self.path.angle += m.angle;
        // Stronger smoothing follows the path more slowly
        let s = strength.min(100) as f32 / 100.0;
        let follow = 0.3 * (1.0 - s) + 0.015;
        self.smooth.x += (self.path.x - self.smooth.x) * follow;
        self.smooth.y += (self.path.y - self.smooth.y) * follow;
        self.smooth.angle += (self.path.angle - self.smooth.angle) * follow;
        // Never more than the zoom leaves room for: a pan is let through
        let margin = margin(strength);
        let limit = |smooth: &mut f32, path: f32, max: f32| {
            *smooth = path + (*smooth - path).clamp(-max, max);
        };
        limit(&mut self.smooth.x, self.path.x, margin);
        limit(&mut self.smooth.y, self.path.y, margin);
        limit(&mut self.smooth.angle, self.path.angle, margin * 0.6);
        self.correction(strength)
    }

    fn correction(&self, strength: u8) -> Correction {
        Correction {
            offset: [self.smooth.x - self.path.x, self.smooth.y - self.path.y],
            angle: self.smooth.angle - self.path.angle,
            zoom: 1.0 + margin(strength) * 0.9,
        }
    }
}

/// Room for corrections, in shader units of the height (2): 5-15 %
fn margin(strength: u8) -> f32 {
    0.1 + 0.2 * strength.min(100) as f32 / 100.0
}

/// The corrections of recent pictures by timestamp, for the GPU stages to
/// find theirs; and the last one, for a picture not in the list
struct Shared {
    recent: VecDeque<(u64, Correction)>,
    last: Correction,
}

static SHARED: Mutex<Shared> = Mutex::new(Shared {
    recent: VecDeque::new(),
    last: Correction::NONE,
});

pub fn correction_for(pts: Option<u64>) -> Correction {
    let shared = SHARED.lock().unwrap();
    pts.and_then(|pts| {
        shared
            .recent
            .iter()
            .rev()
            .find(|(t, _)| *t == pts)
            .map(|(_, c)| *c)
    })
    .unwrap_or(shared.last)
}

fn publish(pts: Option<u64>, c: Correction) {
    let mut shared = SHARED.lock().unwrap();
    shared.last = c;
    if let Some(pts) = pts {
        shared.recent.push_back((pts, c));
        while shared.recent.len() > 64 {
            shared.recent.pop_front();
        }
    }
}

/// The settings it runs with, changed while running
#[derive(Clone, Copy)]
pub struct Tuning {
    pub strength: u8,
    pub roll: bool,
}

static TUNING: Mutex<Tuning> = Mutex::new(Tuning {
    strength: 50,
    roll: true,
});

pub fn tune(t: Tuning) {
    *TUNING.lock().unwrap() = t;
}

/// Analysing while held; the GPU stages hold one each
pub struct Running {
    _subscription: Subscription,
}

struct Analysis {
    users: usize,
    running: Option<Arc<Running>>,
}

static ANALYSIS: Mutex<Analysis> = Mutex::new(Analysis {
    users: 0,
    running: None,
});

/// Keeps the analysis going while held
pub struct Hold(#[allow(dead_code)] Arc<Running>);

impl Drop for Hold {
    fn drop(&mut self) {
        stop();
    }
}

/// Starts analysing the decoded pictures, or joins the analysis running
pub fn start(decoder: &Arc<SharedDecoder>) -> Result<Hold, String> {
    join(decoder).map(Hold)
}

fn join(decoder: &Arc<SharedDecoder>) -> Result<Arc<Running>, String> {
    let mut a = ANALYSIS.lock().unwrap();
    if a.users > 0
        && let Some(r) = a.running.clone()
    {
        a.users += 1;
        return Ok(r);
    }
    let mut prev: Option<Small> = None;
    let mut path = Path::default();
    let subscription = decoder.subscribe(move |sample| {
        let pts = sample.buffer().and_then(|b| b.pts()).map(|t| t.nseconds());
        let small = small_of(sample);
        let tuning = *TUNING.lock().unwrap();
        let motion = match (&prev, &small) {
            (Some(p), Some(c)) => estimate(p, c, tuning.roll),
            _ => None,
        };
        let correction = path.step(motion, tuning.strength);
        publish(pts, correction);
        prev = small;
    })?;
    let running = Arc::new(Running {
        _subscription: subscription,
    });
    a.users = 1;
    a.running = Some(Arc::clone(&running));
    println!("[stab] analysing the picture");
    Ok(running)
}

/// A GPU stage is done with it; the last stops the analysis
fn stop() {
    let mut a = ANALYSIS.lock().unwrap();
    a.users = a.users.saturating_sub(1);
    if a.users == 0 && a.running.take().is_some() {
        publish(None, Correction::NONE);
        println!("[stab] stopped");
    }
}

fn small_of(sample: &gst::Sample) -> Option<Small> {
    let info = gst_video::VideoInfo::from_caps(sample.caps()?).ok()?;
    let buffer = sample.buffer()?;
    let frame = gst_video::VideoFrameRef::from_buffer_ref_readable(buffer, &info).ok()?;
    let plane = frame.plane_data(0).ok()?;
    let stride = frame.plane_stride()[0] as usize;
    Small::from_luma(plane, stride, info.width() as usize, info.height() as usize)
}

#[cfg(test)]
#[path = "stab.test.rs"]
mod tests;

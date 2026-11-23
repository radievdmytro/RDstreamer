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
use std::sync::{Arc, LazyLock, Mutex};

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
    // Across from the top and bottom halves separately, then their mean:
    // one half's moving object or rolling-shutter skew counts for less
    let mid_p = (pf + pt) / 2;
    let mid_c = (cf + ct) / 2;
    let (sx_top, fit_t) = shift(
        &prev.columns_between(pf, mid_p),
        &cur.columns_between(cf, mid_c),
    );
    let (sx_bottom, fit_b) = shift(
        &prev.columns_between(mid_p, pt),
        &cur.columns_between(mid_c, ct),
    );
    let sx = (sx_top + sx_bottom) / 2.0;
    let fit_x = fit_t.max(fit_b);
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

/// How it's tuned, changed while running
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tuning {
    /// 0-100: how much sideways and up-down shake is smoothed
    pub shake: u8,
    /// 0-100: how much the horizon's wobble is smoothed; 0 leaves it
    pub horizon: u8,
    /// Zoom that makes room for the corrections, % (5-30)
    pub crop: u8,
    /// How far ahead it may look, by showing the picture this much later:
    /// 0 for live, where it can only follow
    pub delay_ms: u16,
}

impl Tuning {
    /// Room for corrections in shader units (the height is 2)
    fn margin(&self) -> f32 {
        // Zoomed by z, the picture's edge is at 1/z: 1 - 1/z to spare
        1.0 - 1.0 / self.zoom()
    }

    fn zoom(&self) -> f32 {
        1.0 + self.crop.clamp(2, 40) as f32 / 100.0
    }
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            shake: 60,
            horizon: 40,
            crop: 12,
            delay_ms: 0,
        }
    }
}

/// One analysed picture: where the camera had got to
#[derive(Clone, Copy, Debug)]
struct Point {
    pts: u64,
    path: Motion,
    /// Followed live (for no delay)
    live: Motion,
}

/// The camera's path, picture by picture, for a few seconds back
#[derive(Default)]
pub struct Path {
    points: VecDeque<Point>,
    path: Motion,
    live: Motion,
    /// The last few motions, to spot one that can't be real
    recent: VecDeque<Motion>,
}

/// Seconds of path kept: enough for the longest delay's window
const KEEP_NS: u64 = 4_000_000_000;

impl Path {
    /// Notes one picture's motion (None: lost track)
    pub fn step(&mut self, pts: u64, m: Option<Motion>, t: &Tuning) {
        let m = m.filter(|m| self.plausible(m));
        match m {
            Some(m) => {
                self.path.x += m.x;
                self.path.y += m.y;
                self.path.angle += m.angle;
                self.recent.push_back(m);
                if self.recent.len() > 15 {
                    self.recent.pop_front();
                }
                // Live: follows the path, more slowly the stronger it is
                let follow = |s: u8| 0.35 * (1.0 - s.min(100) as f32 / 100.0) + 0.02;
                self.live.x += (self.path.x - self.live.x) * follow(t.shake);
                self.live.y += (self.path.y - self.live.y) * follow(t.shake);
                self.live.angle += (self.path.angle - self.live.angle) * follow(t.horizon);
            }
            None => {
                // A cut or lost track: start again from here, and forget
                // the path before it so nothing smooths across it
                self.live = self.path;
                self.points.clear();
                self.recent.clear();
            }
        }
        self.points.push_back(Point {
            pts,
            path: self.path,
            live: self.live,
        });
        while self
            .points
            .front()
            .is_some_and(|p| pts.saturating_sub(p.pts) > KEEP_NS)
        {
            self.points.pop_front();
        }
    }

    /// A jolt far beyond the last few is a cut or a mismatch, not motion
    fn plausible(&self, m: &Motion) -> bool {
        if self.recent.len() < 5 {
            return true;
        }
        let typical = |f: fn(&Motion) -> f32| {
            let mut v: Vec<f32> = self.recent.iter().map(|m| f(m).abs()).collect();
            v.sort_by(f32::total_cmp);
            v[v.len() / 2]
        };
        let big = |now: f32, usual: f32, floor: f32| now.abs() > (usual * 6.0).max(floor);
        !(big(m.x, typical(|m| m.x), 0.25)
            || big(m.y, typical(|m| m.y), 0.25)
            || big(m.angle, typical(|m| m.angle), 0.05))
    }

    /// The correction for the picture at `pts`
    pub fn correction(&self, pts: u64, t: &Tuning) -> Correction {
        let Some(i) = self.points.iter().rposition(|p| p.pts == pts) else {
            return self
                .points
                .back()
                .map_or(Correction::NONE, |p| self.correct(p.path, p.live, t));
        };
        let here = self.points[i];
        if t.delay_ms == 0 {
            return self.correct(here.path, here.live, t);
        }
        // Delayed: the path is known on both sides; a gaussian average
        // around this picture, wider the stronger
        let half = t.delay_ms as f64 * 1e6;
        let smooth_over = |strength: u8| (half * strength.min(100) as f64 / 100.0).max(1.0);
        let (sx, sa) = (smooth_over(t.shake), smooth_over(t.horizon));
        let mut sum = [0f64; 3];
        let mut weight = [0f64; 2];
        for p in &self.points {
            let d = p.pts as f64 - pts as f64;
            if d.abs() > half {
                continue;
            }
            let wx = (-0.5 * (d / (sx / 2.0)).powi(2)).exp();
            let wa = (-0.5 * (d / (sa / 2.0)).powi(2)).exp();
            sum[0] += wx * p.path.x as f64;
            sum[1] += wx * p.path.y as f64;
            sum[2] += wa * p.path.angle as f64;
            weight[0] += wx;
            weight[1] += wa;
        }
        let smooth = Motion {
            x: (sum[0] / weight[0].max(1e-9)) as f32,
            y: (sum[1] / weight[0].max(1e-9)) as f32,
            angle: (sum[2] / weight[1].max(1e-9)) as f32,
        };
        self.correct(here.path, smooth, t)
    }

    fn correct(&self, path: Motion, smooth: Motion, t: &Tuning) -> Correction {
        let m = t.margin();
        // Never more than the zoom leaves room for: a pan is let through
        let horizon = if t.horizon == 0 { 0.0 } else { 1.0 };
        Correction {
            offset: [
                (smooth.x - path.x).clamp(-m, m),
                (smooth.y - path.y).clamp(-m, m),
            ],
            // Turning moves the corners furthest: about twice as far
            angle: ((smooth.angle - path.angle) * horizon).clamp(-m * 0.4, m * 0.4),
            zoom: t.zoom(),
        }
    }
}

/// The path, shared with the GPU stages that look corrections up in it
struct Shared {
    path: Path,
    tuning: Tuning,
}

static SHARED: LazyLock<Mutex<Shared>> = LazyLock::new(|| {
    Mutex::new(Shared {
        path: Path::default(),
        tuning: Tuning::default(),
    })
});

/// The correction for the picture with this timestamp
pub fn correction_for(pts: Option<u64>) -> Correction {
    let shared = SHARED.lock().unwrap();
    match pts {
        Some(pts) => shared.path.correction(pts, &shared.tuning),
        None => Correction::NONE,
    }
}

pub fn tune(t: Tuning) {
    SHARED.lock().unwrap().tuning = t;
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
    let subscription = decoder.subscribe(move |sample| {
        let Some(pts) = sample.buffer().and_then(|b| b.pts()).map(|t| t.nseconds()) else {
            return;
        };
        let small = small_of(sample);
        let roll = SHARED.lock().unwrap().tuning.horizon > 0;
        let motion = match (&prev, &small) {
            (Some(p), Some(c)) => estimate(p, c, roll),
            _ => None,
        };
        let mut shared = SHARED.lock().unwrap();
        let tuning = shared.tuning;
        shared.path.step(pts, motion, &tuning);
        drop(shared);
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
        SHARED.lock().unwrap().path = Path::default();
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

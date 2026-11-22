//! The look of the HDMI picture: a 3D LUT, graded on the GPU.
//!
//! The LUT comes from a built-in preset or an uploaded `.cube` file and is
//! handed to the GPU as a 2D texture: the blue axis laid out as N slices of
//! N × N side by side. `gl::RdLook` samples it for every pixel. Picking
//! another LUT or strength takes effect on the next frame; switching the look
//! on or off rebuilds the HDMI pipeline (`hdmi.rs`).

mod gl;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use serde::Serialize;

use crate::settings::LookSettings;

pub use gl::register;

/// What the GPU gets: the most a texture can be wide on the Pi is 4096, so
/// a 65-point LUT (65 × 65 = 4225) is resampled to this
const MAX_SIZE: usize = 64;
/// The presets' resolution
const PRESET_SIZE: usize = 33;
/// The largest `.cube` accepted, in bytes: a 65-point LUT is about 7 MB
pub const MAX_FILE: usize = 12 << 20;
pub const CUSTOM: &str = "custom";

/// Built-in looks: id and label
pub const PRESETS: [(&str, &str); 7] = [
    ("film", "Film"),
    ("warm", "Warm"),
    ("cool", "Cool"),
    ("teal_orange", "Teal & orange"),
    ("punchy", "Punchy"),
    ("vivid", "Vivid FPV"),
    ("mono", "Black & white"),
];

/// A 3D LUT: `size`³ RGB entries, red changing fastest, as in `.cube`
#[derive(Clone, Debug, PartialEq)]
pub struct Lut {
    pub size: usize,
    pub data: Vec<[f32; 3]>,
    pub title: Option<String>,
}

impl Lut {
    fn from_fn(size: usize, f: impl Fn([f32; 3]) -> [f32; 3]) -> Self {
        let n = (size - 1) as f32;
        let mut data = Vec::with_capacity(size * size * size);
        for b in 0..size {
            for g in 0..size {
                for r in 0..size {
                    let c = f([r as f32 / n, g as f32 / n, b as f32 / n]);
                    data.push(c.map(|v| v.clamp(0.0, 1.0)));
                }
            }
        }
        Self {
            size,
            data,
            title: None,
        }
    }

    /// Trilinear lookup, for resampling
    pub fn sample(&self, c: [f32; 3]) -> [f32; 3] {
        let n = self.size - 1;
        let pos = c.map(|v| v.clamp(0.0, 1.0) * n as f32);
        let i0 = pos.map(|p| (p.floor() as usize).min(n));
        let i1 = i0.map(|i| (i + 1).min(n));
        let f = [
            pos[0] - i0[0] as f32,
            pos[1] - i0[1] as f32,
            pos[2] - i0[2] as f32,
        ];
        let at =
            |r: usize, g: usize, b: usize| self.data[r + g * self.size + b * self.size * self.size];
        let lerp = |a: [f32; 3], b: [f32; 3], t: f32| {
            [
                a[0] + (b[0] - a[0]) * t,
                a[1] + (b[1] - a[1]) * t,
                a[2] + (b[2] - a[2]) * t,
            ]
        };
        let c00 = lerp(at(i0[0], i0[1], i0[2]), at(i1[0], i0[1], i0[2]), f[0]);
        let c10 = lerp(at(i0[0], i1[1], i0[2]), at(i1[0], i1[1], i0[2]), f[0]);
        let c01 = lerp(at(i0[0], i0[1], i1[2]), at(i1[0], i0[1], i1[2]), f[0]);
        let c11 = lerp(at(i0[0], i1[1], i1[2]), at(i1[0], i1[1], i1[2]), f[0]);
        lerp(lerp(c00, c10, f[1]), lerp(c01, c11, f[1]), f[2])
    }

    /// Small enough for a texture on the Pi
    fn fitted(self) -> Self {
        if self.size <= MAX_SIZE {
            return self;
        }
        let title = self.title.clone();
        Self {
            title,
            ..Self::from_fn(PRESET_SIZE, |c| self.sample(c))
        }
    }
}

/// Reads an Adobe/Resolve `.cube` 3D LUT
pub fn parse_cube(text: &str) -> Result<Lut, String> {
    let mut size = None;
    let mut title = None;
    let mut min = [0.0f32; 3];
    let mut max = [1.0f32; 3];
    let mut data = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut words = line.split_whitespace();
        let first = words.next().unwrap_or("");
        let numbers = |words: std::str::SplitWhitespace| -> Result<[f32; 3], String> {
            let v: Vec<f32> = words
                .map(|w| w.parse::<f32>())
                .collect::<Result<_, _>>()
                .map_err(|_| format!("Line {}: not a number", n + 1))?;
            v.try_into()
                .map_err(|_| format!("Line {}: expected three numbers", n + 1))
        };
        match first.to_ascii_uppercase().as_str() {
            "TITLE" => {
                title = Some(line[5..].trim().trim_matches('"').to_string());
            }
            "LUT_3D_SIZE" => {
                let s: usize = words
                    .next()
                    .and_then(|w| w.parse().ok())
                    .ok_or(format!("Line {}: LUT_3D_SIZE needs a number", n + 1))?;
                if !(2..=256).contains(&s) {
                    return Err(format!("A {s}-point LUT isn't supported"));
                }
                size = Some(s);
            }
            "LUT_1D_SIZE" => return Err("That's a 1D LUT; a 3D one is needed".into()),
            "DOMAIN_MIN" => min = numbers(words)?,
            "DOMAIN_MAX" => max = numbers(words)?,
            "LUT_3D_INPUT_RANGE" | "LUT_1D_INPUT_RANGE" => {}
            _ if first.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '.') => {
                let v = numbers(line.split_whitespace())?;
                data.push(v);
            }
            // Other keywords some tools write
            _ => {}
        }
    }
    let size = size.ok_or("Not a 3D LUT: there's no LUT_3D_SIZE line")?;
    if data.len() != size * size * size {
        return Err(format!(
            "A {size}-point LUT needs {} entries, the file has {}",
            size * size * size,
            data.len()
        ));
    }
    // Outputs outside the domain are scaled into 0-1
    let span = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
    if span.iter().any(|s| *s <= 0.0) {
        return Err("DOMAIN_MAX has to be above DOMAIN_MIN".into());
    }
    for v in &mut data {
        for i in 0..3 {
            v[i] = ((v[i] - min[i]) / span[i]).clamp(0.0, 1.0);
        }
    }
    Ok(Lut { size, data, title })
}

fn luma(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

fn saturate(c: [f32; 3], amount: f32) -> [f32; 3] {
    let l = luma(c);
    c.map(|v| l + (v - l) * amount)
}

/// An S-curve around mid grey; `amount` 0 leaves it alone
fn contrast(c: [f32; 3], amount: f32) -> [f32; 3] {
    c.map(|v| {
        let v = v.clamp(0.0, 1.0);
        let s = v * v * (3.0 - 2.0 * v);
        v + (s - v) * amount
    })
}

fn preset_fn(id: &str) -> Option<fn([f32; 3]) -> [f32; 3]> {
    Some(match id {
        "film" => |c| {
            let c = contrast(c, 0.45);
            let c = saturate(c, 0.85);
            let l = luma(c);
            // Lifted blacks, warm highlights
            [
                0.04 + c[0] * 0.95 + 0.03 * l,
                0.035 + c[1] * 0.95 + 0.01 * l,
                0.05 + c[2] * 0.92 - 0.02 * l,
            ]
        },
        "warm" => |c| contrast([c[0] * 1.06 + 0.01, c[1] * 1.01, c[2] * 0.88], 0.1),
        "cool" => |c| contrast([c[0] * 0.92, c[1] * 1.0, c[2] * 1.08 + 0.01], 0.1),
        "teal_orange" => |c| {
            let l = luma(c);
            let shadow = [0.0, 0.08, 0.1];
            let highlight = [0.1, 0.03, -0.08];
            let w = l * l * (3.0 - 2.0 * l);
            let c = [
                c[0] + shadow[0] * (1.0 - w) + highlight[0] * w,
                c[1] + shadow[1] * (1.0 - w) + highlight[1] * w,
                c[2] + shadow[2] * (1.0 - w) + highlight[2] * w,
            ];
            saturate(contrast(c, 0.25), 1.1)
        },
        "punchy" => |c| saturate(contrast(c, 0.6), 1.25),
        "vivid" => |c| saturate(contrast(c, 0.35), 1.4),
        "mono" => |c| {
            let l = luma(contrast(c, 0.3));
            [l, l, l]
        },
        _ => return None,
    })
}

pub fn preset(id: &str) -> Option<Lut> {
    preset_fn(id).map(|f| Lut::from_fn(PRESET_SIZE, f))
}

/// The LUT as the GPU takes it: N slices of N × N, side by side, RGBA
pub struct Atlas {
    pub size: usize,
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

impl Atlas {
    pub fn new(lut: &Lut) -> Self {
        let n = lut.size;
        let width = n * n;
        let mut pixels = vec![255u8; width * n * 4];
        for b in 0..n {
            for g in 0..n {
                for r in 0..n {
                    let v = lut.data[r + g * n + b * n * n];
                    let at = (g * width + b * n + r) * 4;
                    for i in 0..3 {
                        pixels[at + i] = (v[i] * 255.0).round() as u8;
                    }
                }
            }
        }
        Self {
            size: n,
            width,
            height: n,
            pixels,
        }
    }
}

/// What the GPU filter grades with now
pub(crate) struct Current {
    pub atlas: Option<Arc<Atlas>>,
    /// Changes whenever the LUT does, so the texture is made again
    pub version: u64,
    pub strength: f32,
    /// Which LUT that is, to tell a real change from the same one again
    key: Option<(String, Option<std::time::SystemTime>)>,
}

pub(crate) static CURRENT: LazyLock<Mutex<Current>> = LazyLock::new(|| {
    Mutex::new(Current {
        atlas: None,
        version: 0,
        strength: 0.0,
        key: None,
    })
});

static CUSTOM_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Where an uploaded `.cube` is kept, beside the settings
pub fn init(settings: &Path) {
    let _ = CUSTOM_PATH.set(settings.with_file_name("look.cube"));
    if let Err(e) = register() {
        eprintln!("[look] GPU grading unavailable: {e}");
    }
}

fn custom_path() -> PathBuf {
    CUSTOM_PATH
        .get()
        .cloned()
        .unwrap_or_else(|| crate::settings::config_path().with_file_name("look.cube"))
}

/// Makes `cfg`'s LUT and strength the ones in use. The same LUT again keeps
/// its texture
pub fn select(cfg: &LookSettings) {
    let path = custom_path();
    let stamp = (cfg.lut == CUSTOM)
        .then(|| fs::metadata(&path).and_then(|m| m.modified()).ok())
        .flatten();
    let key = (cfg.lut.clone(), stamp);
    let mut current = CURRENT.lock().unwrap();
    current.strength = f32::from(cfg.strength.min(100)) / 100.0;
    if current.key.as_ref() == Some(&key) {
        return;
    }
    let lut = if cfg.lut == CUSTOM {
        match fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| parse_cube(&t))
        {
            Ok(lut) => Some(lut.fitted()),
            Err(e) => {
                eprintln!("[look] custom LUT: {e}");
                None
            }
        }
    } else {
        preset(&cfg.lut)
    };
    current.atlas = lut.as_ref().map(|l| Arc::new(Atlas::new(l)));
    current.version += 1;
    current.key = Some(key);
}

#[derive(Serialize)]
pub struct CustomInfo {
    pub title: Option<String>,
    pub size: usize,
}

#[derive(Serialize)]
pub struct LookStatus {
    pub presets: Vec<Preset>,
    /// The uploaded `.cube`, if there is one
    pub custom: Option<CustomInfo>,
    /// The GPU grading couldn't start on this device, and why
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct Preset {
    pub id: &'static str,
    pub label: &'static str,
}

pub fn status(error: Option<String>) -> LookStatus {
    let custom = fs::read_to_string(custom_path())
        .ok()
        .and_then(|t| parse_cube(&t).ok())
        .map(|l| CustomInfo {
            title: l.title,
            size: l.size,
        });
    LookStatus {
        presets: PRESETS
            .iter()
            .map(|&(id, label)| Preset { id, label })
            .collect(),
        custom,
        error,
    }
}

/// Checks and keeps an uploaded `.cube`
pub fn save_custom(bytes: &[u8]) -> Result<CustomInfo, String> {
    if bytes.len() > MAX_FILE {
        return Err("That file is too large for a LUT".into());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "That isn't a .cube text file")?;
    let lut = parse_cube(text)?;
    let path = custom_path();
    let tmp = path.with_extension("cube.tmp");
    fs::write(&tmp, text).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(CustomInfo {
        title: lut.title,
        size: lut.size,
    })
}

pub fn remove_custom() -> Result<(), String> {
    match fs::remove_file(custom_path()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
#[path = "look.test.rs"]
mod tests;

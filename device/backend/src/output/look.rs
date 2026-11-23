//! The look of the HDMI picture, done on the GPU: a 3D LUT and lens
//! correction (dewarp).
//!
//! The LUT comes from a built-in preset or a `.cube` in the library (uploaded,
//! or a free pack the device downloads itself) and is handed to the GPU as a
//! 2D texture: the blue axis laid out as N slices of N × N side by side.
//! `gl::RdLook` samples it for every pixel. Picking another LUT, strength or
//! dewarp amount takes effect on the next frame; switching either on or off
//! rebuilds the HDMI pipeline (`hdmi.rs`).

mod gl;
pub mod stab;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use serde::Serialize;

use crate::settings::{DewarpSettings, LookSettings};

pub use gl::register;

/// What the GPU gets: the most a texture can be wide on the Pi is 4096, so
/// a 65-point LUT (65 × 65 = 4225) is resampled to this
const MAX_SIZE: usize = 64;
/// The presets' resolution
const PRESET_SIZE: usize = 33;
/// The largest `.cube` accepted, in bytes: a 65-point LUT is about 7 MB
pub const MAX_FILE: usize = 12 << 20;
/// A library LUT's id is this and its file name
pub const FILE: &str = "file:";
/// The most LUTs kept in the library
const MAX_FILES: usize = 100;

/// A free pack the device can fetch from its publisher: Todd Blankenship's
/// LUTs, as G'MIC hands them out (13 looks: Crisp Autumn, Hard Boost, Long
/// Beach Morning, Orange and Blue, Waves, ...). Not shipped with RDstreamer
pub const PACK_URL: &str = "https://gmic.eu/color_presets/toddblankenship/toddblankenship_cube.zip";
const PACK_MAX: u64 = 64 << 20;

/// Built-in looks: id and label
pub const PRESETS: [(&str, &str); 8] = [
    ("hlg_709", "HLG → Rec.709"),
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

/// HLG (BT.2100) to an SDR Rec.709 picture: back to scene light, onto the
/// BT.709 primaries, HLG's reference white (75 % signal) to about 90 % with a
/// soft shoulder above, then the BT.709 curve. An approximation, not DJI's LUT
fn hlg_to_709(c: [f32; 3]) -> [f32; 3] {
    const A: f32 = 0.178_832_77;
    const B: f32 = 0.284_668_92;
    const C: f32 = 0.559_910_7;
    let linear = |e: f32| {
        let e = e.clamp(0.0, 1.0);
        if e <= 0.5 {
            e * e / 3.0
        } else {
            (((e - C) / A).exp() + B) / 12.0
        }
    };
    let l = c.map(linear);
    let m = [
        [1.6605, -0.5876, -0.0728],
        [-0.1246, 1.1329, -0.0083],
        [-0.0182, -0.1006, 1.1187],
    ];
    let rgb: [f32; 3] = std::array::from_fn(|i| m[i][0] * l[0] + m[i][1] * l[1] + m[i][2] * l[2]);
    let white = linear(0.75);
    let tone = |v: f32| {
        let v = (v / white * 0.9).max(0.0);
        // Above 0.8 rolled off towards 1
        if v <= 0.8 {
            v
        } else {
            0.8 + 0.2 * (1.0 - (-(v - 0.8) / 0.2).exp())
        }
    };
    let oetf = |v: f32| {
        if v < 0.018 {
            4.5 * v
        } else {
            1.099 * v.powf(0.45) - 0.099
        }
    };
    rgb.map(|v| oetf(tone(v)))
}

fn preset_fn(id: &str) -> Option<fn([f32; 3]) -> [f32; 3]> {
    Some(match id {
        "hlg_709" => hlg_to_709,
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

/// Lens correction as the shader takes it
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dewarp {
    pub on: bool,
    /// Radial terms: the picture is sampled at r × (1 + k1 r² + k2 r⁴)
    pub k1: f32,
    pub k2: f32,
    /// Magnification: above 1 crops in
    pub scale: f32,
    /// Shift, as a fraction of the picture
    pub offset: [f32; 2],
}

impl Dewarp {
    pub const OFF: Self = Self {
        on: false,
        k1: 0.0,
        k2: 0.0,
        scale: 1.0,
        offset: [0.0, 0.0],
    };

    pub fn from(cfg: &DewarpSettings) -> Self {
        if !cfg.enabled {
            return Self::OFF;
        }
        Self {
            on: true,
            // Positive correction pulls the edges in from further out:
            // a fisheye's barrel undone
            k1: -f32::from(cfg.fisheye) / 100.0 * 0.5,
            k2: -f32::from(cfg.edge) / 100.0 * 0.3,
            scale: (1.0 + f32::from(cfg.zoom) / 100.0).max(0.5),
            offset: [
                f32::from(cfg.offset_x) / 100.0,
                f32::from(cfg.offset_y) / 100.0,
            ],
        }
    }

    /// Where an output point (0-1 across the picture) samples the input;
    /// the shader does the same, and the tests hold them to it
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn source(&self, uv: [f32; 2], aspect: f32) -> [f32; 2] {
        if !self.on {
            return uv;
        }
        let mut p = [(uv[0] * 2.0 - 1.0) * aspect, uv[1] * 2.0 - 1.0];
        p = [
            p[0] / self.scale + self.offset[0] * 2.0 * aspect,
            p[1] / self.scale + self.offset[1] * 2.0,
        ];
        let r2 = (p[0] * p[0] + p[1] * p[1]) / (1.0 + aspect * aspect);
        let f = 1.0 + self.k1 * r2 + self.k2 * r2 * r2;
        [(p[0] * f / aspect + 1.0) / 2.0, (p[1] * f + 1.0) / 2.0]
    }
}

/// What the GPU filter works with now
pub(crate) struct Current {
    pub atlas: Option<Arc<Atlas>>,
    /// Changes whenever the LUT does, so the texture is made again
    pub version: u64,
    /// 0 with the look off
    pub strength: f32,
    pub dewarp: Dewarp,
    pub stabilise: bool,
    /// Which LUT that is, to tell a real change from the same one again
    key: Option<(String, Option<std::time::SystemTime>)>,
}

/// The Pi's image processor hands the GPU RGB in video levels: black at 16,
/// white at 235. Shown like that, black was grey and the picture flat next to
/// HDMI without the GPU stage, so the shader stretches it to full range
/// The splash screen is up (status, QR code, a custom image): HDMI shows it
/// without the LUT or lens correction
pub static SPLASH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub static VIDEO_LEVELS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) static CURRENT: LazyLock<Mutex<Current>> = LazyLock::new(|| {
    Mutex::new(Current {
        atlas: None,
        version: 0,
        strength: 0.0,
        dewarp: Dewarp::OFF,
        stabilise: false,
        key: None,
    })
});

static LIBRARY: OnceLock<PathBuf> = OnceLock::new();

/// The LUT library is a folder beside the settings
pub fn init(settings: &Path) {
    let _ = LIBRARY.set(settings.with_file_name("luts"));
    if let Err(e) = register() {
        eprintln!("[look] GPU grading unavailable: {e}");
    }
}

fn library() -> PathBuf {
    LIBRARY
        .get()
        .cloned()
        .unwrap_or_else(|| crate::settings::config_path().with_file_name("luts"))
}

/// A file name for a LUT: lower case letters, digits and dashes
pub fn slug(name: &str) -> String {
    let stem = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .trim_end_matches(".cube")
        .trim_end_matches(".CUBE");
    let mut out = String::new();
    for ch in stem.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-');
    out.chars().take(60).collect()
}

fn file_path(id: &str) -> Option<PathBuf> {
    let name = id.strip_prefix(FILE)?;
    (!name.is_empty() && slug(name) == name).then(|| library().join(format!("{name}.cube")))
}

/// Makes the settings' LUT, strength and dewarp the ones in use. The same LUT
/// again keeps its texture
pub fn select(
    look: &LookSettings,
    dewarp: &DewarpSettings,
    stabilization: &crate::settings::StabilizationSettings,
) {
    stab::tune(stab::Tuning {
        strength: stabilization.strength,
        roll: stabilization.roll,
    });
    let path = file_path(&look.lut);
    let stamp = path
        .as_ref()
        .and_then(|p| fs::metadata(p).and_then(|m| m.modified()).ok());
    let key = (look.lut.clone(), stamp);
    let mut current = CURRENT.lock().unwrap();
    current.strength = if look.enabled {
        f32::from(look.strength.min(100)) / 100.0
    } else {
        0.0
    };
    current.dewarp = Dewarp::from(dewarp);
    current.stabilise = stabilization.enabled;
    if current.key.as_ref() == Some(&key) {
        return;
    }
    let lut = match &path {
        Some(path) => match fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|t| parse_cube(&t))
        {
            Ok(lut) => Some(lut.fitted()),
            Err(e) => {
                eprintln!("[look] {}: {e}", look.lut);
                None
            }
        },
        None => preset(&look.lut),
    };
    current.atlas = lut.as_ref().map(|l| Arc::new(Atlas::new(l)));
    current.version += 1;
    current.key = Some(key);
}

/// The texture the GPU grades with for `id`, for the web interface's preview
/// to grade the same way
pub fn atlas_for(id: &str) -> Result<Atlas, String> {
    let lut = match file_path(id) {
        Some(path) => parse_cube(&fs::read_to_string(path).map_err(|_| "No such LUT")?)?.fitted(),
        None => preset(id).ok_or("No such LUT")?,
    };
    Ok(Atlas::new(&lut))
}

#[derive(Serialize)]
pub struct LutFile {
    /// "file:" and the file's name
    pub id: String,
    pub name: String,
    pub size: usize,
}

#[derive(Serialize)]
pub struct LookStatus {
    pub presets: Vec<Preset>,
    /// The library, by name
    pub files: Vec<LutFile>,
    /// The GPU couldn't start on this device, and why
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct Preset {
    pub id: &'static str,
    pub label: &'static str,
}

/// A name from TITLE, or else the file's
fn display_name(stem: &str, title: Option<String>) -> String {
    title.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| {
        stem.split('-')
            .filter(|w| !w.is_empty())
            .map(|w| {
                let mut c = w.chars();
                c.next()
                    .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" ")
    })
}

/// Only the header, so listing a library of big LUTs stays quick
fn header(path: &Path) -> Option<(Option<String>, usize)> {
    use std::io::{BufRead, BufReader};
    let file = fs::File::open(path).ok()?;
    let mut title = None;
    for line in BufReader::new(file).lines().take(200) {
        let line = line.ok()?;
        let line = line.trim();
        let upper = line.to_ascii_uppercase();
        if upper.starts_with("TITLE") {
            title = Some(line[5..].trim().trim_matches('"').to_string());
        } else if upper.starts_with("LUT_3D_SIZE") {
            return Some((title, line[11..].trim().parse().ok()?));
        }
    }
    None
}

pub fn files() -> Vec<LutFile> {
    let mut list: Vec<LutFile> = fs::read_dir(library())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let stem = path.file_stem()?.to_str()?.to_string();
            if path.extension()? != "cube" || slug(&stem) != stem {
                return None;
            }
            let (title, size) = header(&path)?;
            Some(LutFile {
                id: format!("{FILE}{stem}"),
                name: display_name(&stem, title),
                size,
            })
        })
        .collect();
    list.sort_by_key(|f| f.name.to_lowercase());
    list
}

pub fn status(error: Option<String>) -> LookStatus {
    LookStatus {
        presets: PRESETS
            .iter()
            .map(|&(id, label)| Preset { id, label })
            .collect(),
        files: files(),
        error,
    }
}

/// Checks a `.cube` and adds it to the library, under its file name
pub fn save(name: &str, bytes: &[u8]) -> Result<LutFile, String> {
    if bytes.len() > MAX_FILE {
        return Err("That file is too large for a LUT".into());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "That isn't a .cube text file")?;
    let lut = parse_cube(text)?;
    let stem = slug(name);
    if stem.is_empty() {
        return Err("The file needs a name".into());
    }
    let dir = library();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{stem}.cube"));
    if !path.exists() && files().len() >= MAX_FILES {
        return Err(format!(
            "The library holds {MAX_FILES} LUTs; remove some first"
        ));
    }
    // Without a TITLE of its own, the file's name as it came is kept as one
    let original = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .trim_end_matches(".cube")
        .trim_end_matches(".CUBE")
        .replace(['"', '\n', '\r'], "");
    let title = lut.title.clone().filter(|t| !t.trim().is_empty());
    let (text, title) = match title {
        Some(t) => (text.to_string(), Some(t)),
        None => (format!("TITLE \"{original}\"\n{text}"), Some(original)),
    };
    let tmp = dir.join(format!(".{stem}.tmp"));
    fs::write(&tmp, &text).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(LutFile {
        id: format!("{FILE}{stem}"),
        name: display_name(&stem, title),
        size: lut.size,
    })
}

pub fn remove(id: &str) -> Result<(), String> {
    let path = file_path(id).ok_or("No such LUT")?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Every `.cube` in a zip into the library; how many
pub fn import_zip(bytes: &[u8]) -> Result<usize, String> {
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("Not a zip: {e}"))?;
    let mut added = 0;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().map_err(|e| e.to_string())?.to_string();
        if !name.to_ascii_lowercase().ends_with(".cube") || entry.size() > MAX_FILE as u64 {
            continue;
        }
        let mut data = Vec::with_capacity(entry.size() as usize);
        std::io::Read::read_to_end(&mut entry, &mut data).map_err(|e| e.to_string())?;
        match save(&name, &data) {
            Ok(_) => added += 1,
            Err(e) => eprintln!("[look] {name}: {e}"),
        }
    }
    Ok(added)
}

/// Fetches the free pack from its publisher onto this device
pub async fn download_pack() -> Result<usize, String> {
    let tmp = std::env::temp_dir().join("rdstreamer-luts.zip");
    let max = PACK_MAX.to_string();
    crate::net::network::run(
        "curl",
        &[
            "-fsSL",
            "--max-time",
            "120",
            "--max-filesize",
            &max,
            "-o",
            tmp.to_str().unwrap_or_default(),
            PACK_URL,
        ],
    )
    .await
    .map_err(|e| format!("Couldn't download the LUTs ({e}). Is the device online?"))?;
    let bytes = tokio::fs::read(&tmp).await.map_err(|e| e.to_string())?;
    let _ = tokio::fs::remove_file(&tmp).await;
    tokio::task::spawn_blocking(move || import_zip(&bytes))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
#[path = "look.test.rs"]
mod tests;

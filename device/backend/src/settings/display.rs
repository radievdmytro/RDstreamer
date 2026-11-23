use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct DisplaySettings {
    pub hdmi_output: bool,
    /// `WxH@Hz` as listed by the screen, or "auto" for its preferred mode.
    /// 1080p60 by default: 4K screens prefer 4K, which the Pi can't scan out
    /// at 60 fps
    pub mode: String,
    pub scaling: Scaling,
    pub rotate_180: bool,
    pub mirror: bool,
    /// Colour grading on the HDMI picture
    pub look: LookSettings,
    /// Lens correction on the HDMI picture
    pub dewarp: DewarpSettings,
    /// Shake taken out of the picture, from the picture itself
    pub stabilization: StabilizationSettings,
    /// LUT, lens correction and stabilisation in the streams and browsers
    /// too, encoded again at up to 1080p30; otherwise they get the goggles'
    /// own video
    pub processed_streams: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct StabilizationSettings {
    pub enabled: bool,
    /// 0-100: how smooth, and how much the picture is zoomed to make room
    pub strength: u8,
    /// Level out turning too, not just shifting
    pub roll: bool,
}

impl StabilizationSettings {
    pub fn validate(&self) -> Result<(), String> {
        if self.strength > 100 {
            return Err("Stabilisation strength is 0 to 100".into());
        }
        Ok(())
    }
}

impl Default for StabilizationSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            strength: 60,
            roll: true,
        }
    }
}

/// Straightens a fisheye picture on the GPU, for HDMI
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct DewarpSettings {
    pub enabled: bool,
    /// -100 to 100 %: positive straightens a fisheye's barrel
    pub fisheye: i8,
    /// -100 to 100 %: the same, mostly at the edges
    pub edge: i8,
    /// -50 to 100 %: in, to crop away what the correction leaves empty
    pub zoom: i8,
    /// -50 to 50 % of the picture
    pub offset_x: i8,
    pub offset_y: i8,
}

impl Default for DewarpSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            fisheye: 30,
            edge: 0,
            zoom: 10,
            offset_x: 0,
            offset_y: 0,
        }
    }
}

impl DewarpSettings {
    pub fn validate(&self) -> Result<(), String> {
        let within = |v: i8, lo: i8, hi: i8, what: &str| {
            if (lo..=hi).contains(&v) {
                Ok(())
            } else {
                Err(format!("{what} must be {lo} to {hi}%"))
            }
        };
        within(self.fisheye, -100, 100, "Fisheye correction")?;
        within(self.edge, -100, 100, "Edge correction")?;
        within(self.zoom, -50, 100, "Zoom")?;
        within(self.offset_x, -50, 50, "Offset X")?;
        within(self.offset_y, -50, 50, "Offset Y")
    }
}

/// A 3D LUT on the HDMI picture, graded on the GPU
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct LookSettings {
    pub enabled: bool,
    /// A preset's id, or "file:" and the name of a `.cube` in the library
    pub lut: String,
    /// How much of the LUT, 0-100 %
    pub strength: u8,
    pub quality: LookQuality,
}

impl Default for LookSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            lut: "film".into(),
            strength: 100,
            quality: LookQuality::Speed,
        }
    }
}

impl LookSettings {
    pub fn validate(&self) -> Result<(), String> {
        let known = self.lut.starts_with(crate::output::look::FILE)
            || crate::output::look::PRESETS
                .iter()
                .any(|(id, _)| *id == self.lut);
        if !known {
            return Err(format!("There's no LUT called '{}'", self.lut));
        }
        if self.strength > 100 {
            return Err("LUT strength must be 0-100%".into());
        }
        Ok(())
    }
}

/// Where the GPU works (the look and dewarp both): smaller is smooth at
/// 60 fps on a Pi 4, full size is sharper but manages about 48
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LookQuality {
    /// 720p, scaled up by the display hardware
    #[default]
    Speed,
    /// The video's own size
    Quality,
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            hdmi_output: true,
            mode: "1920x1080@60.00".into(),
            scaling: Scaling::Fit,
            rotate_180: false,
            mirror: false,
            look: LookSettings::default(),
            dewarp: DewarpSettings::default(),
            stabilization: StabilizationSettings::default(),
            processed_streams: false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Scaling {
    #[default]
    Fit,
    Fill,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct OverlaySettings {
    pub enabled: bool,
    pub x_percent: f32,
    pub y_percent: f32,
    pub size_percent: f32,
    pub opacity_percent: u8,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            x_percent: 97.0,
            y_percent: 5.0,
            size_percent: 15.0,
            opacity_percent: 100,
        }
    }
}

impl OverlaySettings {
    pub fn validate(&self) -> Result<(), String> {
        if !(2.0..=100.0).contains(&self.size_percent) {
            return Err("Logo size must be 2-100% of the picture width".into());
        }
        if !(0.0..=100.0).contains(&self.x_percent) || !(0.0..=100.0).contains(&self.y_percent) {
            return Err("Logo position must be within the picture".into());
        }
        if !(10..=100).contains(&self.opacity_percent) {
            return Err("Logo opacity must be 10-100%".into());
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct SplashSettings {
    pub freeze_last_frame: bool,
    pub grayscale_freeze: bool,
    pub status_over_image: bool,
    pub image_scaling: Scaling,
    pub show_ap_password: bool,
    /// Play the uploaded background video behind the status
    pub background_video: bool,
}

impl Default for SplashSettings {
    fn default() -> Self {
        Self {
            freeze_last_frame: false,
            grayscale_freeze: true,
            status_over_image: true,
            image_scaling: Scaling::Fit,
            show_ap_password: true,
            background_video: true,
        }
    }
}

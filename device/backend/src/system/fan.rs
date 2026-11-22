//! The Argon ONE case fan. Argon40's argononed service drives it over I2C
//! from a curve in /etc/argononed.conf; this reads that curve, rewrites it
//! and restarts the service, going back to the old curve if the service
//! doesn't come up with the new one. Without argononed there's nothing to
//! show, and the web interface leaves the card out

use std::sync::LazyLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tokio::sync::Mutex;

use crate::net::network::run;

const UNIT: &str = "argononed.service";
const CONF: &str = "/etc/argononed.conf";
const THERMAL: &str = "/sys/class/thermal/thermal_zone0/temp";

pub const MAX_POINTS: usize = 8;
pub const MIN_C: u8 = 30;
pub const MAX_C: u8 = 85;
/// A curve has to reach full speed by this, so nothing set here can let the
/// Pi cook: it starts throttling at 80 °C
pub const FULL_BY_C: u8 = 75;

/// Tried and tested on the case: off until 45 °C, then quiet
pub const QUIET: [Point; 4] = [
    Point {
        temp_c: 45,
        speed: 5,
    },
    Point {
        temp_c: 55,
        speed: 25,
    },
    Point {
        temp_c: 60,
        speed: 55,
    },
    Point {
        temp_c: 65,
        speed: 100,
    },
];

/// One at a time: two saves racing would leave either curve, but restart the
/// service twice and could roll back the wrong one
static WRITING: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

/// From this temperature up, this speed in percent
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Point {
    pub temp_c: u8,
    pub speed: u8,
}

#[derive(Serialize)]
pub struct Limits {
    pub max_points: usize,
    pub min_c: u8,
    pub max_c: u8,
    pub full_by_c: u8,
    pub quiet: [Point; 4],
}

#[derive(Serialize)]
pub struct FanStatus {
    /// argononed is installed
    pub available: bool,
    pub active: bool,
    pub curve: Vec<Point>,
    pub temp_c: Option<f32>,
    /// What the curve asks of the fan at that temperature. The controller
    /// can't be read back, so this is the intent, not a measurement
    pub speed: Option<u8>,
    pub limits: Limits,
}

/// stdout whatever the exit code: `is-active` answers "inactive" with a
/// failure
async fn output(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .output()
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

async fn installed() -> bool {
    output("systemctl", &["show", "-p", "LoadState", "--value", UNIT]).await == "loaded"
}

async fn active() -> bool {
    output("systemctl", &["is-active", UNIT]).await == "active"
}

fn temperature() -> Option<f32> {
    std::fs::read_to_string(THERMAL)
        .ok()?
        .trim()
        .parse::<f32>()
        .ok()
        .map(|m| (m / 100.0).round() / 10.0)
}

pub async fn status() -> FanStatus {
    let available = installed().await;
    let curve = std::fs::read_to_string(CONF)
        .map(|text| parse(&text))
        .unwrap_or_default();
    let temp_c = temperature();
    FanStatus {
        available,
        active: available && active().await,
        speed: temp_c.map(|t| speed_at(&curve, t)),
        curve,
        temp_c,
        limits: Limits {
            max_points: MAX_POINTS,
            min_c: MIN_C,
            max_c: MAX_C,
            full_by_c: FULL_BY_C,
            quiet: QUIET,
        },
    }
}

/// The `temperature=speed` lines argononed reads; anything else (comments,
/// a stray line) is passed over as argononed does
pub fn parse(text: &str) -> Vec<Point> {
    let mut points: Vec<Point> = text
        .lines()
        .filter_map(|line| {
            let line = line.split('#').next()?.trim();
            let (t, s) = line.split_once('=')?;
            Some(Point {
                temp_c: t.trim().parse().ok()?,
                speed: s.trim().parse().ok()?,
            })
        })
        .collect();
    points.sort_by_key(|p| p.temp_c);
    points
}

pub fn render(points: &[Point]) -> String {
    let mut text = String::from(
        "# Written by YStreamer (System → General → Cooling fan)\n\
         # temperature (°C)=fan speed (%)\n",
    );
    for p in points {
        text.push_str(&format!("{}={}\n", p.temp_c, p.speed));
    }
    text
}

pub fn validate(points: &[Point]) -> Result<(), String> {
    if points.is_empty() || points.len() > MAX_POINTS {
        return Err(format!("Use 1 to {MAX_POINTS} points"));
    }
    for p in points {
        if !(MIN_C..=MAX_C).contains(&p.temp_c) {
            return Err(format!("{} °C is outside {MIN_C}–{MAX_C} °C", p.temp_c));
        }
        if p.speed > 100 {
            return Err(format!("{}% is more than 100%", p.speed));
        }
    }
    for pair in points.windows(2) {
        if pair[1].temp_c <= pair[0].temp_c {
            return Err("Each point needs a higher temperature than the one before".into());
        }
        if pair[1].speed < pair[0].speed {
            return Err("The fan can't get slower as it gets hotter".into());
        }
    }
    if !points
        .iter()
        .any(|p| p.temp_c <= FULL_BY_C && p.speed == 100)
    {
        return Err(format!("The fan has to reach 100% by {FULL_BY_C} °C"));
    }
    Ok(())
}

/// What argononed sets: the speed of the highest point at or below `temp`,
/// off below the first
pub fn speed_at(points: &[Point], temp: f32) -> u8 {
    points
        .iter()
        .filter(|p| temp >= f32::from(p.temp_c))
        .map(|p| p.speed)
        .next_back()
        .unwrap_or(0)
}

/// Replaces the file whole, so argononed never reads half a curve
fn write(text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = format!("{CONF}.ystreamer");
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    std::fs::rename(&tmp, CONF)
}

async fn restart() -> bool {
    if let Err(e) = run("systemctl", &["restart", UNIT]).await {
        eprintln!("[fan] systemctl restart {UNIT}: {e}");
    }
    // It exits within a second when it can't reach the controller
    tokio::time::sleep(Duration::from_secs(2)).await;
    active().await
}

pub async fn set(points: &[Point]) -> Result<(), String> {
    validate(points)?;
    let _one = WRITING.lock().await;

    let before = std::fs::read_to_string(CONF).ok();
    if before.as_deref().map(parse).as_deref() == Some(points) && active().await {
        return Ok(());
    }

    write(&render(points)).map_err(|e| format!("Couldn't write {CONF}: {e}"))?;
    if restart().await {
        eprintln!(
            "[fan] curve set: {}",
            render(points).lines().skip(2).collect::<Vec<_>>().join(" ")
        );
        return Ok(());
    }

    eprintln!("[fan] argononed didn't start with the new curve; putting the old one back");
    let undone = match &before {
        Some(text) => write(text),
        None => std::fs::remove_file(CONF),
    };
    let back = undone.is_ok() && restart().await;
    Err(if back {
        "The fan service didn't start with that curve, so the previous one is back".into()
    } else {
        "The fan service didn't start, even with the previous curve. The Logs page may say why"
            .into()
    })
}

#[cfg(test)]
#[path = "fan.test.rs"]
mod tests;

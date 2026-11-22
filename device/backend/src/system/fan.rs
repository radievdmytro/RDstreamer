//! The Argon ONE case, run from here instead of by Argon40's argononed:
//!
//! - the fan: a byte 0-100 (percent) to the case's controller, at 0x1a on
//!   I2C bus 1, following a temperature curve
//! - the power button: the controller pulses GPIO4 high, 20-30 ms for a
//!   double tap (reboot) and 40-50 ms for a press held 3 s (power off)
//! - the power cut: 0xff to the controller as the Pi powers off, so the case
//!   switches the power off once it's down. The system-shutdown hook does
//!   that, through `ystreamer argon-shutdown`
//!
//! While argononed runs it does all of this itself, and this stays out of
//! its way until it's switched off (`take_over`)

use std::fs;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::settings::SettingsStore;

const BUS: &str = "/dev/i2c-1";
const ADDRESS: u16 = 0x1a;
const POWER_CUT: u8 = 0xff;
const BUTTON_LINE: u32 = 4;
const ARGONONED: &str = "argononed.service";
const ARGONONED_CONF: &str = "/etc/argononed.conf";
const THERMAL: &str = "/sys/class/thermal/thermal_zone0/temp";
/// Tells the shutdown hook that this was running the case. /run is still
/// there when the hook runs
pub const MARKER: &str = "/run/ystreamer-argon";

const TICK: Duration = Duration::from_secs(2);
/// How often to look for the case and for argononed again
const RECHECK: Duration = Duration::from_secs(30);
/// A lower speed has to be wanted this long before the fan slows down, so
/// it doesn't hunt around a threshold (argononed waits the same)
const SLOW_DOWN_AFTER: Duration = Duration::from_secs(30);
/// The speed is sent again now and then, in case the controller restarted
const RESEND: Duration = Duration::from_secs(300);

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

static FAN: OnceLock<Arc<Fan>> = OnceLock::new();

/// From this temperature up, this speed in percent
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Point {
    pub temp_c: u8,
    pub speed: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Run the case when one answers and nothing says it's something else
    #[default]
    Auto,
    /// Run whatever answers at the case's address
    On,
    /// Leave the case alone
    Off,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FanConfig {
    pub mode: Mode,
    pub curve: Vec<Point>,
    /// Reboot on a double tap, power off on a 3 s hold
    pub power_button: bool,
    /// Have the case switch the power off after the Pi powers off
    pub power_cut: bool,
    /// Full speed for a second before a slow speed, for fans that don't
    /// start slowly from standstill
    pub spin_up: bool,
}

impl Default for FanConfig {
    fn default() -> Self {
        Self {
            mode: Mode::Auto,
            curve: QUIET.to_vec(),
            power_button: true,
            power_cut: true,
            spin_up: false,
        }
    }
}

/// Who's running the fan now
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Control {
    /// This is
    Driving,
    /// Argon40's own service is, and this waits for it to be switched off
    Argononed,
    /// Switched off here
    Off,
    /// Auto, but a HAT's on the pins, which may be what answers at 0x1a
    Hat,
    /// Nothing answers at the case's address
    NoCase,
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
    pub control: Control,
    pub config: FanConfig,
    pub temp_c: Option<f32>,
    /// What was last sent to the fan, while this runs it. The controller
    /// can't report its speed back
    pub speed: Option<u8>,
    /// The power button is being listened to
    pub power_button: bool,
    pub error: Option<String>,
    pub limits: Limits,
}

#[derive(Clone)]
struct State {
    control: Control,
    speed: Option<u8>,
    button: bool,
    error: Option<String>,
}

pub struct Fan {
    path: PathBuf,
    config: Mutex<FanConfig>,
    state: Mutex<State>,
    /// Look for the case and argononed again on the next tick
    recheck: AtomicBool,
    settings: Arc<SettingsStore>,
}

impl Fan {
    pub fn start(settings: Arc<SettingsStore>) -> Arc<Self> {
        let path = settings.path().with_file_name("fan.json");
        let fan = Arc::new(Self {
            config: Mutex::new(load_config(&path)),
            path,
            state: Mutex::new(State {
                control: Control::NoCase,
                speed: None,
                button: false,
                error: None,
            }),
            recheck: AtomicBool::new(true),
            settings,
        });
        let _ = FAN.set(Arc::clone(&fan));
        let this = Arc::clone(&fan);
        thread::spawn(move || this.run());
        let this = Arc::clone(&fan);
        thread::spawn(move || this.listen());
        fan
    }

    pub fn config(&self) -> FanConfig {
        self.config.lock().unwrap().clone()
    }

    pub fn status(&self) -> FanStatus {
        let state = self.state.lock().unwrap().clone();
        FanStatus {
            control: state.control,
            config: self.config(),
            temp_c: temperature(),
            speed: state.speed,
            power_button: state.button,
            error: state.error,
            limits: Limits {
                max_points: MAX_POINTS,
                min_c: MIN_C,
                max_c: MAX_C,
                full_by_c: FULL_BY_C,
                quiet: QUIET,
            },
        }
    }

    pub fn set_config(&self, config: FanConfig) -> Result<(), String> {
        validate(&config.curve)?;
        save_config(&self.path, &config).map_err(|e| format!("Couldn't save: {e}"))?;
        *self.config.lock().unwrap() = config;
        self.recheck.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Switches argononed off for good and runs the case from here, with
    /// argononed's curve if it's one this accepts
    pub async fn take_over(&self) -> Result<(), String> {
        use crate::net::network::run;
        run("systemctl", &["disable", "--now", ARGONONED]).await?;
        let mut config = self.config();
        if let Ok(text) = fs::read_to_string(ARGONONED_CONF) {
            let curve = parse(&text);
            if validate(&curve).is_ok() {
                config.curve = curve;
            }
        }
        if config.mode == Mode::Off {
            config.mode = Mode::Auto;
        }
        self.set_config(config)?;
        // Let the next tick find the case free
        tokio::time::sleep(TICK + Duration::from_millis(500)).await;
        Ok(())
    }

    pub fn current_speed(&self) -> Option<u8> {
        let state = self.state.lock().unwrap();
        (state.control == Control::Driving)
            .then_some(state.speed)
            .flatten()
    }

    fn set_state(&self, change: impl FnOnce(&mut State)) {
        change(&mut self.state.lock().unwrap());
    }

    fn run(self: Arc<Self>) {
        let mut governor = Governor::default();
        let mut bus: Option<I2c> = None;
        let mut checked: Option<Instant> = None;
        let (mut found, mut argononed, mut hat) = (false, false, false);
        let mut reported = None;

        loop {
            let config = self.config();
            if self.recheck.swap(false, Ordering::Relaxed)
                || checked.is_none_or(|at| at.elapsed() >= RECHECK)
            {
                argononed = service_active(ARGONONED);
                hat = Path::new("/proc/device-tree/hat").exists();
                if bus.is_none() {
                    bus = I2c::open(ADDRESS).ok();
                }
                found = bus.as_ref().is_some_and(I2c::answers);
                checked = Some(Instant::now());
            }

            let control = decide(config.mode, found, argononed, hat);
            if reported != Some(control) {
                println!("[fan] {}", describe(control));
                reported = Some(control);
            }

            let mut error = None;
            if control == Control::Driving {
                let _ = fs::write(MARKER, b"");
                let before = governor.current;
                if let Some(speed) = governor.next(&config.curve, temperature(), Instant::now()) {
                    let from_standstill = before.unwrap_or(0) == 0;
                    let sent = bus.as_ref().map(|b| {
                        if config.spin_up && speed > 0 && speed < 100 && from_standstill {
                            b.send(100)?;
                            thread::sleep(Duration::from_secs(1));
                        }
                        b.send(speed)
                    });
                    match sent {
                        Some(Ok(())) => {}
                        Some(Err(e)) => {
                            error = Some(format!("Couldn't set the fan: {e}"));
                            eprintln!("[fan] {}", error.as_deref().unwrap_or_default());
                            // Open the bus again and send the speed again
                            bus = None;
                            governor = Governor::default();
                            self.recheck.store(true, Ordering::Relaxed);
                        }
                        None => governor = Governor::default(),
                    }
                }
            } else {
                let _ = fs::remove_file(MARKER);
                governor = Governor::default();
            }

            self.set_state(|s| {
                s.control = control;
                s.speed = if control == Control::Driving {
                    governor.current
                } else {
                    None
                };
                s.error = error;
            });
            thread::sleep(TICK);
        }
    }

    /// The button counts while this runs the case and nothing else has GPIO4
    fn button_wanted(&self) -> bool {
        if !self.config().power_button || self.state.lock().unwrap().control != Control::Driving {
            return false;
        }
        let s = self.settings.get();
        let uses = |enabled: bool, pin: u8, led: Option<u8>| {
            enabled && (pin == BUTTON_LINE as u8 || led == Some(BUTTON_LINE as u8))
        };
        !uses(
            s.recording.button.enabled,
            s.recording.button.pin,
            s.recording.button.led_pin,
        ) && !uses(s.input.button.enabled, s.input.button.pin, None)
    }

    fn listen(self: Arc<Self>) {
        let mut complained = false;
        loop {
            if !self.button_wanted() {
                thread::sleep(TICK);
                continue;
            }
            match self.watch_button() {
                Ok(()) => complained = false,
                Err(e) => {
                    if !complained {
                        eprintln!("[fan] power button: {e}");
                        complained = true;
                    }
                    self.set_state(|s| s.button = false);
                    thread::sleep(RECHECK);
                }
            }
        }
    }

    /// Returns once the button is no longer wanted
    fn watch_button(&self) -> Result<(), String> {
        use gpiocdev::Request;
        use gpiocdev::line::{EdgeDetection, EdgeKind};

        let line = Request::builder()
            .on_chip(super::button::CHIP)
            .with_consumer("ystreamer-argon-power")
            .with_line(BUTTON_LINE)
            .as_input()
            .with_edge_detection(EdgeDetection::BothEdges)
            .request()
            .map_err(|e| format!("GPIO{BUTTON_LINE} isn't free: {e}"))?;
        println!("[fan] listening to the case's power button");
        self.set_state(|s| s.button = true);

        let mut rose: Option<u64> = None;
        while self.button_wanted() {
            if !line
                .wait_edge_event(Duration::from_secs(1))
                .map_err(|e| e.to_string())?
            {
                continue;
            }
            let event = line.read_edge_event().map_err(|e| e.to_string())?;
            match event.kind {
                EdgeKind::Rising => rose = Some(event.timestamp_ns),
                EdgeKind::Falling => {
                    let Some(start) = rose.take() else { continue };
                    let ms = event.timestamp_ns.saturating_sub(start) / 1_000_000;
                    if let Some(press) = classify(ms) {
                        let action = match press {
                            Press::Reboot => super::Action::Reboot,
                            Press::Poweroff => super::Action::Poweroff,
                        };
                        println!("[fan] power button: {press:?} ({ms} ms pulse)");
                        if let Err(e) = super::schedule(action) {
                            eprintln!("[fan] power button: {e}");
                        }
                    }
                }
            }
        }
        self.set_state(|s| s.button = false);
        Ok(())
    }
}

/// The speed being sent to the fan, for the Performance page
pub fn current_speed() -> Option<u8> {
    FAN.get().and_then(|f| f.current_speed())
}

pub fn decide(mode: Mode, found: bool, argononed: bool, hat: bool) -> Control {
    match mode {
        Mode::Off => Control::Off,
        _ if argononed => Control::Argononed,
        _ if !found => Control::NoCase,
        Mode::Auto if hat => Control::Hat,
        _ => Control::Driving,
    }
}

fn describe(control: Control) -> &'static str {
    match control {
        Control::Driving => "running the Argon ONE case's fan",
        Control::Argononed => "argononed runs the fan; leaving it alone",
        Control::Off => "fan control is off",
        Control::Hat => "a HAT is fitted, so 0x1a may not be a case; leaving it alone",
        Control::NoCase => "no Argon ONE case found",
    }
}

/// Decides what to send the fan, and when
#[derive(Default)]
pub struct Governor {
    /// What was sent last
    pub current: Option<u8>,
    /// What was sent before that
    pub previous: Option<u8>,
    lower_since: Option<Instant>,
    sent_at: Option<Instant>,
}

impl Governor {
    /// A speed to send now, if any
    pub fn next(&mut self, curve: &[Point], temp: Option<f32>, now: Instant) -> Option<u8> {
        // Without a temperature, err on the side of cooling
        let target = match temp {
            Some(t) => speed_at(curve, t),
            None => self.current.unwrap_or(100).max(50),
        };
        let Some(current) = self.current else {
            return Some(self.send(target, now));
        };
        if target > current {
            return Some(self.send(target, now));
        }
        if target < current {
            let since = *self.lower_since.get_or_insert(now);
            return (now.duration_since(since) >= SLOW_DOWN_AFTER).then(|| self.send(target, now));
        }
        self.lower_since = None;
        let stale = self
            .sent_at
            .is_none_or(|at| now.duration_since(at) >= RESEND);
        stale.then(|| self.send(target, now))
    }

    fn send(&mut self, speed: u8, now: Instant) -> u8 {
        if self.current != Some(speed) {
            self.previous = self.current;
        }
        self.current = Some(speed);
        self.lower_since = None;
        self.sent_at = Some(now);
        speed
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Press {
    Reboot,
    Poweroff,
}

/// The pulse lengths Argon40 documents, with room for a slow read
pub fn classify(ms: u64) -> Option<Press> {
    match ms {
        15..=39 => Some(Press::Reboot),
        40..=59 => Some(Press::Poweroff),
        _ => None,
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

/// The speed of the highest point at or below `temp`, off below the first
pub fn speed_at(points: &[Point], temp: f32) -> u8 {
    points
        .iter()
        .filter(|p| temp >= f32::from(p.temp_c))
        .map(|p| p.speed)
        .next_back()
        .unwrap_or(0)
}

/// What's saved, or the defaults; a curve that doesn't pass is replaced
pub fn load_config(path: &Path) -> FanConfig {
    let mut config: FanConfig = fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    if validate(&config.curve).is_err() {
        config.curve = QUIET.to_vec();
    }
    config
}

fn save_config(path: &Path, config: &FanConfig) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let mut file = fs::File::create(&tmp)?;
    file.write_all(serde_json::to_string_pretty(config)?.as_bytes())?;
    file.sync_all()?;
    fs::rename(&tmp, path)
}

fn temperature() -> Option<f32> {
    fs::read_to_string(THERMAL)
        .ok()?
        .trim()
        .parse::<f32>()
        .ok()
        .map(|m| (m / 100.0).round() / 10.0)
}

fn service_active(unit: &str) -> bool {
    std::process::Command::new("systemctl")
        .args(["is-active", "--quiet", unit])
        .status()
        .is_ok_and(|s| s.success())
}

/// One device on the I2C bus, through the kernel's i2c-dev
struct I2c {
    file: fs::File,
}

impl I2c {
    const I2C_SLAVE: libc::c_ulong = 0x0703;
    const I2C_SMBUS: libc::c_ulong = 0x0720;

    fn open(address: u16) -> std::io::Result<Self> {
        let file = fs::OpenOptions::new().read(true).write(true).open(BUS)?;
        // SAFETY: an ioctl on a descriptor we own, with a plain integer
        if unsafe {
            libc::ioctl(
                file.as_raw_fd(),
                Self::I2C_SLAVE,
                libc::c_ulong::from(address),
            )
        } < 0
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self { file })
    }

    /// Whether something acknowledges its address: an SMBus "quick write",
    /// as i2cdetect does, which carries no data at all
    fn answers(&self) -> bool {
        #[repr(C)]
        struct SmbusIoctl {
            read_write: u8,
            command: u8,
            size: u32,
            data: *mut u8,
        }
        let mut args = SmbusIoctl {
            read_write: 0, // write
            command: 0,
            size: 0, // I2C_SMBUS_QUICK
            data: std::ptr::null_mut(),
        };
        // SAFETY: the struct matches the kernel's i2c_smbus_ioctl_data and
        // a quick write reads nothing through `data`
        unsafe { libc::ioctl(self.file.as_raw_fd(), Self::I2C_SMBUS, &mut args) == 0 }
    }

    fn send(&self, byte: u8) -> std::io::Result<()> {
        use std::io::Write;
        (&self.file).write_all(&[byte])
    }
}

/// `ystreamer argon-shutdown <poweroff|halt|reboot|kexec>`, run by the
/// system-shutdown hook as the very last thing. Stops the fan, and on
/// power-off has the case cut the power once the Pi is down
pub fn shutdown_hook(kind: &str) -> Result<(), String> {
    if !Path::new(MARKER).exists() {
        return Ok(());
    }
    let config = load_config(&crate::settings::config_path().with_file_name("fan.json"));
    let bus = I2c::open(ADDRESS).map_err(|e| e.to_string())?;
    bus.send(0).map_err(|e| e.to_string())?;
    if matches!(kind, "poweroff" | "halt") && config.power_cut {
        bus.send(POWER_CUT).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "fan.test.rs"]
mod tests;

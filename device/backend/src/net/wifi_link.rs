//! What the WiFi radio is doing right now, for the status bar, and a survey
//! of the 2.4 GHz networks around, for picking a channel. Both read `iw`
//! directly: it answers in milliseconds and, unlike NetworkManager, scans
//! while the radio is an access point.

use std::time::Duration;

use serde::Serialize;

use super::network::run;

const IFACE: &str = "wlan0";
/// NetworkManager's dnsmasq, for the access point's clients' names
const LEASES: &str = "/var/lib/NetworkManager/dnsmasq-wlan0.leases";
/// 2.4 GHz channels the access point may use (settings: 1-11)
const AP_CHANNELS: std::ops::RangeInclusive<u32> = 1..=11;
/// Channels shown in the survey
const SURVEY_CHANNELS: std::ops::RangeInclusive<u32> = 1..=13;

#[derive(Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Broadcasting its own network
    Ap,
    /// Joined someone else's
    Client,
    #[default]
    Off,
}

#[derive(Debug, Default, Serialize)]
pub struct Link {
    pub role: Role,
    /// The network's name: ours as an access point, the joined one as a client
    pub ssid: Option<String>,
    pub channel: Option<u32>,
    /// As a client: the router's signal here
    pub signal_dbm: Option<i32>,
    /// As a client: how fast the link to the router runs
    pub bitrate_mbps: Option<f32>,
    /// As an access point: who is connected, and how well
    pub stations: Vec<Station>,
    /// Where streams to the internet leave: wlan0, eth0, usb0...
    pub uplink: Option<String>,
}

#[derive(Debug, Default, Serialize, PartialEq)]
pub struct Station {
    pub mac: String,
    pub name: Option<String>,
    pub ip: Option<String>,
    pub signal_dbm: Option<i32>,
    pub bitrate_mbps: Option<f32>,
}

#[derive(Debug, Default, Serialize, PartialEq)]
pub struct Bss {
    pub bssid: String,
    /// Empty for a hidden network
    pub ssid: String,
    pub channel: u32,
    pub freq: u32,
    pub signal_dbm: i32,
}

#[derive(Debug, Serialize)]
pub struct ChannelLoad {
    pub channel: u32,
    /// Networks on this very channel
    pub networks: usize,
    /// How much other 2.4 GHz traffic reaches it, overlap and strength
    /// counted: 0 is clear, about 1 is one strong network right on it
    pub load: f32,
}

#[derive(Debug, Serialize)]
pub struct Survey {
    /// 2.4 GHz networks, strongest first
    pub networks: Vec<Bss>,
    /// How many 5 GHz networks were seen (not shown: RDstreamer stays off
    /// 5 GHz, where the goggles' video link is)
    pub networks_5ghz: usize,
    pub channels: Vec<ChannelLoad>,
    /// The clearest of 1, 6 and 11, the channels that don't overlap
    pub best: u32,
    pub current: Option<u32>,
}

pub async fn link() -> Link {
    let info = run("iw", &["dev", IFACE, "info"]).await.unwrap_or_default();
    let mut link = parse_info(&info);
    match link.role {
        Role::Client => {
            let out = run("iw", &["dev", IFACE, "link"]).await.unwrap_or_default();
            let (signal, bitrate) = parse_link(&out);
            link.signal_dbm = signal;
            link.bitrate_mbps = bitrate;
        }
        Role::Ap => {
            let dump = run("iw", &["dev", IFACE, "station", "dump"])
                .await
                .unwrap_or_default();
            let leases = std::fs::read_to_string(LEASES).unwrap_or_default();
            let arp = std::fs::read_to_string("/proc/net/arp").unwrap_or_default();
            link.stations = parse_stations(&dump, &leases, &arp);
        }
        Role::Off => {}
    }
    link.uplink = run("ip", &["route", "show", "default"])
        .await
        .ok()
        .and_then(|out| parse_default_route(&out));
    link
}

pub async fn survey() -> Result<Survey, String> {
    let info = parse_info(&run("iw", &["dev", IFACE, "info"]).await.unwrap_or_default());
    let current = info.channel;
    let out = if info.role == Role::Ap {
        // The Pi's chip refuses to scan on the interface that broadcasts
        // (-52 or -95, even with ap-force), but it runs a second, ordinary
        // interface beside the access point, and that one may
        match scan(IFACE, true).await {
            Ok(out) => Ok(out),
            Err(first) => match scan_beside().await {
                Ok(out) => Ok(out),
                Err(e) => {
                    eprintln!("[wifi] scan: {first}; beside the access point: {e}");
                    Err(e)
                }
            },
        }
    } else {
        scan(IFACE, false).await
    };
    out.map(|out| summarise(parse_scan(&out), current))
        .map_err(|e| scan_error(&e))
}

/// One scan; the radio answers "busy" while it finishes something else, and
/// a moment later it doesn't
async fn scan(iface: &str, ap_force: bool) -> Result<String, String> {
    let mut args = vec!["dev", iface, "scan"];
    if ap_force {
        args.push("ap-force");
    }
    let mut last = String::new();
    for attempt in 0..4 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(1500)).await;
        }
        match run("iw", &args).await {
            Ok(out) => return Ok(out),
            Err(e) if e.contains("busy") || e.contains("-16") => last = e,
            Err(e) => return Err(e),
        }
    }
    Err(last)
}

/// A temporary second interface that only listens, removed afterwards
async fn scan_beside() -> Result<String, String> {
    const SCAN_IFACE: &str = "rdscan0";
    let _ = run("iw", &["dev", SCAN_IFACE, "del"]).await;
    run(
        "iw",
        &[
            "dev",
            IFACE,
            "interface",
            "add",
            SCAN_IFACE,
            "type",
            "managed",
        ],
    )
    .await?;
    let result = async {
        // NetworkManager would try to use it
        let _ = run("nmcli", &["dev", "set", SCAN_IFACE, "managed", "no"]).await;
        // Two interfaces with one address won't both come up
        if let Some(mac) = std::fs::read_to_string(format!("/sys/class/net/{IFACE}/address"))
            .ok()
            .and_then(|m| local_address(m.trim()))
        {
            let _ = run("ip", &["link", "set", "dev", SCAN_IFACE, "address", &mac]).await;
        }
        run("ip", &["link", "set", "dev", SCAN_IFACE, "up"]).await?;
        scan(SCAN_IFACE, false).await
    }
    .await;
    let _ = run("ip", &["link", "set", "dev", SCAN_IFACE, "down"]).await;
    let _ = run("iw", &["dev", SCAN_IFACE, "del"]).await;
    result
}

/// The same address marked locally administered, so it differs from the
/// radio's own
pub(crate) fn local_address(mac: &str) -> Option<String> {
    let mut octets: Vec<u8> = mac
        .split(':')
        .map(|o| u8::from_str_radix(o, 16).ok())
        .collect::<Option<_>>()?;
    if octets.len() != 6 {
        return None;
    }
    octets[0] = (octets[0] | 0x02) & !0x01;
    if format!("{:02x}", octets[0]) == mac[..2].to_lowercase() {
        // Already local: change the last octet instead
        octets[5] ^= 0x01;
    }
    Some(
        octets
            .iter()
            .map(|o| format!("{o:02x}"))
            .collect::<Vec<_>>()
            .join(":"),
    )
}

fn scan_error(e: &str) -> String {
    if e.contains("Operation not supported")
        || e.contains("-95")
        || e.contains("Invalid exchange")
        || e.contains("-52")
    {
        "The WiFi chip can't scan while it broadcasts the access point. \
         Join a network (or turn the access point off) for a moment to scan."
            .into()
    } else if e.contains("busy") || e.contains("-16") {
        "The WiFi radio stayed busy. Try again in a few seconds.".into()
    } else if e.contains("Network is down") || e.contains("-100") {
        "WiFi is off.".into()
    } else {
        format!("Scan failed: {e}")
    }
}

fn summarise(all: Vec<Bss>, current: Option<u32>) -> Survey {
    let (mut networks, five): (Vec<Bss>, Vec<Bss>) = all.into_iter().partition(|b| b.freq < 3000);
    networks.sort_by_key(|b| std::cmp::Reverse(b.signal_dbm));
    let channels: Vec<ChannelLoad> = SURVEY_CHANNELS
        .map(|channel| ChannelLoad {
            channel,
            networks: networks.iter().filter(|b| b.channel == channel).count(),
            load: (load_on(channel, &networks) * 100.0).round() / 100.0,
        })
        .collect();
    let best = [1, 6, 11]
        .into_iter()
        .filter(|c| AP_CHANNELS.contains(c))
        .min_by(|a, b| load_on(*a, &networks).total_cmp(&load_on(*b, &networks)))
        .unwrap_or(6);
    Survey {
        networks,
        networks_5ghz: five.len(),
        channels,
        best,
        current,
    }
}

/// A 2.4 GHz network spreads over its channel and two either side, fading
/// out by the fifth; a strong one counts for more than a faint one
fn load_on(channel: u32, networks: &[Bss]) -> f32 {
    networks
        .iter()
        .map(|b| {
            let distance = channel.abs_diff(b.channel) as f32;
            let overlap = (1.0 - distance / 5.0).max(0.0);
            // -90 dBm barely there, -40 dBm right next to it
            let strength = ((b.signal_dbm as f32 + 90.0) / 50.0).clamp(0.05, 1.0);
            overlap * strength
        })
        .sum()
}

fn channel_of(freq: u32) -> u32 {
    match freq {
        2484 => 14,
        2412..=2472 => (freq - 2407) / 5,
        5000..=5895 => (freq - 5000) / 5,
        _ => 0,
    }
}

/// `iw dev wlan0 info`: the radio's role, network and channel
pub(crate) fn parse_info(out: &str) -> Link {
    let mut link = Link::default();
    for line in out.lines().map(str::trim) {
        if let Some(kind) = line.strip_prefix("type ") {
            link.role = match kind.trim() {
                "AP" => Role::Ap,
                "managed" => Role::Client,
                _ => Role::Off,
            };
        } else if let Some(ssid) = line.strip_prefix("ssid ") {
            link.ssid = Some(ssid.to_string());
        } else if let Some(rest) = line.strip_prefix("channel ") {
            link.channel = rest.split_whitespace().next().and_then(|c| c.parse().ok());
        }
    }
    // A client that isn't connected has no network yet
    if link.role == Role::Client && link.ssid.is_none() {
        link.role = Role::Off;
    }
    link
}

/// `iw dev wlan0 link`: the router's signal and the link's speed
pub(crate) fn parse_link(out: &str) -> (Option<i32>, Option<f32>) {
    let mut signal = None;
    let mut bitrate = None;
    for line in out.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("signal:") {
            signal = dbm(rest);
        } else if let Some(rest) = line.strip_prefix("tx bitrate:") {
            bitrate = mbps(rest);
        }
    }
    (signal, bitrate)
}

/// `iw dev wlan0 station dump`, with names from the DHCP leases and
/// addresses from the neighbour table
pub(crate) fn parse_stations(dump: &str, leases: &str, arp: &str) -> Vec<Station> {
    let mut stations: Vec<Station> = Vec::new();
    for line in dump.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Station ") {
            let mac = rest.split_whitespace().next().unwrap_or("").to_lowercase();
            stations.push(Station {
                mac,
                ..Default::default()
            });
        } else if let Some(st) = stations.last_mut() {
            if let Some(rest) = trimmed.strip_prefix("signal:") {
                st.signal_dbm = dbm(rest);
            } else if let Some(rest) = trimmed.strip_prefix("tx bitrate:") {
                st.bitrate_mbps = mbps(rest);
            }
        }
    }
    for st in &mut stations {
        // expiry mac ip name client-id
        if let Some(fields) = leases
            .lines()
            .map(|l| l.split_whitespace().collect::<Vec<_>>())
            .find(|f| f.len() >= 4 && f[1].eq_ignore_ascii_case(&st.mac))
        {
            st.ip = Some(fields[2].to_string());
            if fields[3] != "*" {
                st.name = Some(fields[3].to_string());
            }
        }
        if st.ip.is_none() {
            // IP address  HW type  Flags  HW address  Mask  Device
            st.ip = arp
                .lines()
                .skip(1)
                .map(|l| l.split_whitespace().collect::<Vec<_>>())
                .find(|f| f.len() >= 4 && f[3].eq_ignore_ascii_case(&st.mac))
                .map(|f| f[0].to_string());
        }
    }
    stations
}

/// `iw dev wlan0 scan`: one entry per access point heard
pub(crate) fn parse_scan(out: &str) -> Vec<Bss> {
    let mut found: Vec<Bss> = Vec::new();
    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("BSS ") {
            let bssid = rest
                .split(|c: char| c == '(' || c.is_whitespace())
                .next()
                .unwrap_or("")
                .to_lowercase();
            found.push(Bss {
                bssid,
                ..Default::default()
            });
            continue;
        }
        let Some(bss) = found.last_mut() else {
            continue;
        };
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("freq:") {
            bss.freq = rest
                .trim()
                .split('.')
                .next()
                .and_then(|f| f.parse().ok())
                .unwrap_or(0);
            bss.channel = channel_of(bss.freq);
        } else if let Some(rest) = t.strip_prefix("signal:") {
            bss.signal_dbm = dbm(rest).unwrap_or(-100);
        } else if let Some(rest) = t.strip_prefix("SSID:") {
            let ssid = rest.trim();
            // Hidden networks send an empty name or zero bytes
            if !ssid.chars().all(|c| c == '\0') && !ssid.starts_with("\\x00") {
                bss.ssid = ssid.to_string();
            }
        }
    }
    found.retain(|b| b.freq > 0);
    found
}

/// `ip route show default`: the device the first default route leaves by
pub(crate) fn parse_default_route(out: &str) -> Option<String> {
    let line = out.lines().next()?;
    let mut words = line.split_whitespace();
    while let Some(w) = words.next() {
        if w == "dev" {
            return words.next().map(str::to_string);
        }
    }
    None
}

/// "-52 dBm", "-40 [-40, -42] dBm", "-61.00 dBm"
fn dbm(text: &str) -> Option<i32> {
    let first = text.split_whitespace().next()?;
    first.parse::<f32>().ok().map(|v| v.round() as i32)
}

/// "72.2 MBit/s VHT-MCS 7 ...", "1.0 MBit/s"
fn mbps(text: &str) -> Option<f32> {
    text.split_whitespace().next()?.parse().ok()
}

#[cfg(test)]
#[path = "wifi_link.test.rs"]
mod tests;

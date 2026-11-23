//! Opens the viewer by itself on a phone that has just joined the access
//! point. Phones check for the internet by fetching a known page (Apple's
//! hotspot-detect, Google's generate_204, ...). The access point's DNS
//! sends those names here, and the first answer is the viewer instead of
//! the expected one, so the phone shows it as a sign-in page. Once a phone
//! has seen it, it gets the expected answer, but only while RDstreamer
//! itself has the internet: otherwise the phone would think the WiFi has
//! it and stop using mobile data.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{ConnectInfo, Request};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// What NetworkManager's dnsmasq reads for the access point
pub const DNSMASQ_CONF: &str = "/etc/NetworkManager/dnsmasq-shared.d/rdstreamer-portal.conf";

/// The names phones and computers check the internet with
pub const PROBE_HOSTS: &[&str] = &[
    "captive.apple.com",
    "www.apple.com",
    "connectivitycheck.gstatic.com",
    "connectivitycheck.android.com",
    "clients3.google.com",
    "clients1.google.com",
    "www.google.com",
    "www.msftconnecttest.com",
    "www.msftncsi.com",
    "detectportal.firefox.com",
    "nmcheck.gnome.org",
];

/// How long a phone that has seen the viewer isn't shown it again
const SEEN_FOR: Duration = Duration::from_secs(12 * 3600);

static SEEN: LazyLock<Mutex<HashMap<IpAddr, Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The dnsmasq lines that send the check names to `address`
pub fn dnsmasq_conf(address: &str) -> String {
    let mut s = String::from("# RDstreamer: open the viewer on phones that join\n");
    for host in PROBE_HOSTS {
        s.push_str(&format!("address=/{host}/{address}\n"));
    }
    s
}

pub(crate) fn is_probe_host(host: &str) -> bool {
    let host = host.split(':').next().unwrap_or(host).to_ascii_lowercase();
    PROBE_HOSTS.contains(&host.as_str())
}

/// What each kind of device expects when there is internet
pub(crate) fn expected(path: &str) -> Response {
    let text = |body: &'static str| {
        (
            [(header::CONTENT_TYPE, HeaderValue::from_static("text/html"))],
            body,
        )
            .into_response()
    };
    match path {
        "/generate_204" | "/gen_204" => StatusCode::NO_CONTENT.into_response(),
        "/connecttest.txt" => text("Microsoft Connect Test"),
        "/ncsi.txt" => text("Microsoft NCSI"),
        "/success.txt" => text("success\n"),
        "/canonical.html" => text(
            "<meta http-equiv=\"refresh\" content=\"0;url=https://support.mozilla.org/kb/captive-portal\"/>",
        ),
        // Apple's, and anything else
        _ => text("<HTML><HEAD><TITLE>Success</TITLE></HEAD><BODY>Success</BODY></HTML>"),
    }
}

/// Before everything else: answers the internet checks, and notes the
/// phones that have opened the viewer
pub async fn middleware(
    axum::extract::State(store): axum::extract::State<
        std::sync::Arc<crate::settings::SettingsStore>,
    >,
    req: Request,
    next: Next,
) -> Response {
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip());
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_string();

    if !is_probe_host(&host) {
        // The viewer itself: this phone has now seen it
        if let Some(ip) = peer
            && req.uri().path() == "/"
        {
            SEEN.lock().unwrap().insert(ip, Instant::now());
        }
        return next.run(req).await;
    }

    let seen = peer.is_some_and(|ip| {
        let mut seen = SEEN.lock().unwrap();
        seen.retain(|_, t| t.elapsed() < SEEN_FOR);
        seen.contains_key(&ip)
    });
    if seen && has_internet().await {
        return expected(req.uri().path());
    }
    // Anything but the expected answer makes it a sign-in page; this one
    // is the viewer at the device's own address
    let local = store.get().wifi.ap.address;
    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, format!("http://{local}/"))
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::empty())
        .unwrap()
}

/// A default route, i.e. somewhere for the internet to go
async fn has_internet() -> bool {
    crate::net::network::run("ip", &["route", "show", "default"])
        .await
        .is_ok_and(|out| !out.trim().is_empty())
}

#[cfg(test)]
#[path = "portal.test.rs"]
mod tests;

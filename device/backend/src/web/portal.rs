//! Opens the viewer by itself on a phone that has just joined the access
//! point. Phones check for the internet by fetching a known page (Apple's
//! hotspot-detect, Google's generate_204, ...). The access point's DNS
//! sends those names here, and the first answer is the viewer instead of
//! the expected one, so the phone shows it as a sign-in page.

use axum::body::Body;
use axum::extract::Request;
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::Response;

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

/// Before everything else: an internet check is answered with the viewer,
/// so the phone shows it as a sign-in page
pub async fn middleware(
    axum::extract::State(store): axum::extract::State<
        std::sync::Arc<crate::settings::SettingsStore>,
    >,
    req: Request,
    next: Next,
) -> Response {
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    if !is_probe_host(host) {
        return next.run(req).await;
    }
    // Never the expected answer, or the phone closes the page and shows
    // "Success" instead. The phone then takes the WiFi for one without
    // internet and keeps using mobile data for it, which is what a viewer
    // wants anyway
    let local = store.get().wifi.ap.address;
    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, format!("http://{local}/"))
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::empty())
        .unwrap()
}

#[cfg(test)]
#[path = "portal.test.rs"]
mod tests;

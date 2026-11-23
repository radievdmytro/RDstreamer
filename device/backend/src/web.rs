pub mod api;
pub mod auth;
mod control;
mod light;
mod rtc;
pub mod terminal;
mod ui;
mod video_ws;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::routing::{get, post};
use tokio::sync::{Mutex, broadcast, mpsc};

use webrtc::api::API;
use webrtc::peer_connection::RTCPeerConnection;

use crate::input::duml::camera::{CameraCommands, CameraState};
use rtc::AccessUnit;

#[derive(Clone)]
struct AppState {
    api: Arc<API>,
    au_tx: broadcast::Sender<AccessUnit>,
    /// The lighter copy for browsers on weak WiFi
    light: Arc<light::LightStream>,
    peers: Arc<Mutex<Vec<Arc<RTCPeerConnection>>>>,
    socket_viewers: Arc<AtomicUsize>,
    camera: Arc<CameraCommands>,
    control_tx: broadcast::Sender<String>,
    services: crate::web::api::Services,
}

/// What a browser asks for: `?quality=light` for the lighter copy
#[derive(serde::Deserialize, Default)]
struct Want {
    #[serde(default)]
    quality: Option<String>,
}

impl AppState {
    /// The video for one browser: the light stream when asked for and it
    /// runs, else the original. The guard keeps the light encoder going
    fn video_for(&self, want: &Want) -> (broadcast::Receiver<AccessUnit>, Option<light::Watching>) {
        if want.quality.as_deref() == Some("light") {
            match self.light.watch() {
                Ok((rx, watching)) => return (rx, Some(watching)),
                Err(e) => eprintln!("[light] {e}; sending the original"),
            }
        }
        (self.au_tx.subscribe(), None)
    }

    /// The limit covers both transports together, since both cost CPU
    async fn viewer_limit_reached(&self) -> bool {
        let max = self.services.store.get().viewer.max_viewers as usize;
        let viewers = self.peers.lock().await.len() + self.socket_viewers.load(Ordering::Relaxed);
        max > 0 && viewers >= max
    }
}

pub fn bind() -> std::io::Result<std::net::TcpListener> {
    let listener = std::net::TcpListener::bind(("0.0.0.0", 80)).or_else(|e| {
        eprintln!("[web] port 80 unavailable ({e}), using 8080");
        std::net::TcpListener::bind(("0.0.0.0", 8080))
    })?;
    listener.set_nonblocking(true)?;
    limit_send_buffer(&listener);
    Ok(listener)
}

/// Connections take these from the listener. Left alone, Linux grew a
/// browser's send buffer to megabytes, and over WiFi seconds of video sat
/// there unseen: the viewer was 1-2 s behind and the server never noticed
/// to skip ahead. This much is a few hundred ms of the goggles' video, still
/// plenty for page loads and downloads. No Nagle delay on small messages
fn limit_send_buffer(listener: &std::net::TcpListener) {
    use std::os::fd::AsRawFd;
    const SEND_BUFFER: libc::c_int = 256 << 10;
    const ON: libc::c_int = 1;
    let fd = listener.as_raw_fd();
    // Safety: a valid socket and int options
    unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_SNDBUF,
            (&SEND_BUFFER as *const libc::c_int).cast(),
            size_of::<libc::c_int>() as libc::socklen_t,
        );
        libc::setsockopt(
            fd,
            libc::IPPROTO_TCP,
            libc::TCP_NODELAY,
            (&ON as *const libc::c_int).cast(),
            size_of::<libc::c_int>() as libc::socklen_t,
        );
    }
}

/// Start the web server. Returns an unbounded sender; push raw H.264 (Annex B)
/// chunks into it
/// `chunk_rx` carries raw H.264 (Annex B) for the browsers, `state_rx` the
/// goggles' camera state
pub fn start(
    listener: std::net::TcpListener,
    services: crate::web::api::Services,
    chunk_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    state_rx: mpsc::UnboundedReceiver<CameraState>,
    input_rx: mpsc::UnboundedReceiver<crate::system::button::InputRequest>,
) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to build Tokio runtime");

        rt.block_on(async move {
            let api = Arc::new(rtc::build_api().expect("Failed to build webrtc API"));
            let (au_tx, _keep) = broadcast::channel::<AccessUnit>(1024);
            tokio::spawn(rtc::assemble_loop(chunk_rx, au_tx.clone()));

            // Broadcast of camera-state JSON to all /control clients
            let (control_tx, _control_keep) = broadcast::channel::<String>(64);
            // Drain state updates from main, serialize, and broadcast
            {
                let control_tx = control_tx.clone();
                let mut state_rx = state_rx;
                tokio::spawn(async move {
                    while let Some(st) = state_rx.recv().await {
                        #[derive(serde::Serialize)]
                        struct StateMsg<'a> {
                            #[serde(rename = "type")]
                            typ: &'a str,
                            #[serde(flatten)]
                            state: &'a CameraState,
                        }
                        let msg = StateMsg {
                            typ: "state",
                            state: &st,
                        };
                        if let Ok(json) = serde_json::to_string(&msg) {
                            let _ = control_tx.send(json);
                        }
                    }
                });
            }

            services.network.start(services.store.get().wifi);
            services.network.ensure_applied(&services.store).await;
            if crate::system::reset::happened() {
                let cfg = services.store.get();
                services.network.apply_later(cfg.wifi);
                services.network.apply_ethernet_later(cfg.ethernet);
            }
            crate::system::ensure_hostname(&services.store.get().device.hostname).await;
            crate::system::adopt_timezone(&services.store);
            tokio::spawn(link_watch(services.clone(), control_tx.clone()));
            tokio::spawn(crate::web::api::input_button_requests(
                services.clone(),
                input_rx,
            ));
            tokio::spawn(crate::system::drives::watch());
            tokio::spawn(crate::system::update::watch(
                Arc::clone(&services.store),
                Arc::clone(&services.metrics),
            ));
            tokio::spawn(finish_recording_on_exit(Arc::clone(&services.recorder)));
            tokio::spawn(reload_login_on_hangup(Arc::clone(&services.auth)));
            tokio::spawn(recording_watch(
                Arc::clone(&services.recorder),
                control_tx.clone(),
            ));

            let state = AppState {
                api,
                au_tx,
                light: light::LightStream::new(Arc::clone(&services.decoder)),
                peers: Arc::new(Mutex::new(Vec::new())),
                socket_viewers: Arc::new(AtomicUsize::new(0)),
                camera: Arc::new(CameraCommands::new()),
                control_tx,
                services: services.clone(),
            };

            // Browsers watching, for the performance record
            {
                let state = state.clone();
                tokio::spawn(async move {
                    loop {
                        let viewers = state.peers.lock().await.len()
                            + state.socket_viewers.load(Ordering::Relaxed);
                        state
                            .services
                            .metrics
                            .viewers
                            .store(viewers, Ordering::Relaxed);
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                });
            }

            let watching = Router::new()
                .route("/offer", post(rtc::offer_handler))
                .route("/video", get(video_ws::video_ws_handler))
                .route("/control", get(control::control_ws_handler))
                .route("/overlay.png", get(overlay_image))
                .route_layer(axum::middleware::from_fn_with_state(
                    Arc::clone(&services.auth),
                    crate::web::auth::viewers_only,
                ));
            let app = Router::new()
                .route("/health", get(health))
                .route("/time", get(time))
                .route("/time/adjust", axum::routing::put(time_adjust))
                .nest("/api", crate::web::api::router(services).merge(watching))
                .fallback_service(
                    get(ui::serve).layer(tower_http::compression::CompressionLayer::new()),
                )
                .with_state(state);

            eprintln!(
                "[web] listening on http://{}",
                listener.local_addr().unwrap()
            );
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            axum::serve(listener, app).await.unwrap();
        });
    });
}

pub(crate) fn link_msg(state: crate::input::goggles::LinkState) -> String {
    serde_json::json!({ "type": "link", "state": state }).to_string()
}

/// `systemctl reload`, which `ystreamer password` uses
async fn reload_login_on_hangup(auth: Arc<auth::Auth>) {
    use tokio::signal::unix::{SignalKind, signal};
    let Ok(mut hangup) = signal(SignalKind::hangup()) else {
        return;
    };
    while hangup.recv().await.is_some() {
        auth.reload();
    }
}

async fn finish_recording_on_exit(recorder: Arc<crate::output::recording::Recorder>) {
    use tokio::signal::unix::{SignalKind, signal};
    let (Ok(mut term), Ok(mut int)) = (
        signal(SignalKind::terminate()),
        signal(SignalKind::interrupt()),
    ) else {
        return;
    };
    tokio::select! {
        _ = term.recv() => {}
        _ = int.recv() => {}
    }
    if recorder.is_active() {
        eprintln!("[rec] stopping to exit");
        let _ = tokio::task::spawn_blocking(move || recorder.stop()).await;
    }
    std::process::exit(0);
}

pub(crate) fn input_msg(services: &crate::web::api::Services) -> String {
    serde_json::json!({
        "type": "input",
        "mode": services.input.mode(),
        // Changes when the input button or the API switches, so open pages
        // can pick up the saved settings
        "saved_mode": services.store.get().input.mode,
        "uvc": services.uvc.status(),
        "legacy": services.legacy.status(),
        "rtmp": services.rtmp_input.status(),
    })
    .to_string()
}

/// Where browsers draw the logo over the player, or that they shouldn't
pub(crate) fn overlay_msg(services: &crate::web::api::Services) -> String {
    let overlay = &services.overlay;
    let cfg = overlay.cfg();
    serde_json::json!({
        "type": "overlay",
        "visible": cfg.enabled && overlay.logo().is_some() && !overlay.burned(),
        "version": overlay.version(),
        "x_percent": cfg.x_percent,
        "y_percent": cfg.y_percent,
        "size_percent": cfg.size_percent,
        "opacity_percent": cfg.opacity_percent,
    })
    .to_string()
}

/// The logo itself, for anyone allowed to watch
async fn overlay_image(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> axum::response::Response {
    use axum::http::{StatusCode, header};
    use axum::response::IntoResponse;
    match tokio::fs::read(state.services.overlay.path()).await {
        Ok(png) => (
            [
                (header::CONTENT_TYPE, "image/png"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            png,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

pub(crate) fn devices_msg(devices: &[crate::input::duml::devices::Device]) -> String {
    serde_json::json!({ "type": "devices", "devices": devices }).to_string()
}

/// Tell browsers when the goggles and air unit come and go, so they drop
/// stale readings like the battery level
async fn link_watch(services: crate::web::api::Services, control_tx: broadcast::Sender<String>) {
    let mut last_state = None;
    let mut last_devices = None;
    let mut last_input = None;
    let mut last_overlay = None;
    loop {
        let state = services.goggles.state();
        if last_state != Some(state) {
            let _ = control_tx.send(link_msg(state));
            last_state = Some(state);
        }
        let devices = services.session.devices();
        if last_devices.as_ref() != Some(&devices) {
            let _ = control_tx.send(devices_msg(&devices));
            last_devices = Some(devices);
        }
        let input = input_msg(&services);
        if last_input.as_ref() != Some(&input) {
            let _ = control_tx.send(input.clone());
            last_input = Some(input);
        }
        let overlay = overlay_msg(&services);
        if last_overlay.as_ref() != Some(&overlay) {
            let _ = control_tx.send(overlay.clone());
            last_overlay = Some(overlay);
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

/// Watch the recorder: stop it if the disk fills or the pipeline fails, and
/// push status to the browsers so the REC indicator and timer stay live
/// without polling
async fn recording_watch(
    recorder: Arc<crate::output::recording::Recorder>,
    control_tx: broadcast::Sender<String>,
) {
    #[derive(serde::Serialize)]
    struct RecordingMsg<'a> {
        #[serde(rename = "type")]
        typ: &'a str,
        recording: &'a crate::output::recording::RecordingStatus,
    }

    let mut last: Option<crate::output::recording::RecordingStatus> = None;
    let mut ticks: u32 = 0;
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        recorder.tick();
        let status = recorder.status();
        // Resend every 10s regardless, so a browser that just connected gets
        // the state even while nothing changes
        ticks += 1;
        if last.as_ref() == Some(&status) && !ticks.is_multiple_of(10) {
            continue;
        }
        if let Ok(json) = serde_json::to_string(&RecordingMsg {
            typ: "recording",
            recording: &status,
        }) {
            let _ = control_tx.send(json);
        }
        last = Some(status);
    }
}

/// The updater reads the version to tell that a new one has come up
/// A correction the sync clock page adds to the device's time, shared by
/// every screen showing it: set by hand when the Pi had no internet to set
/// its clock. Gone on restart
static CLOCK_ADJUST_MS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

#[derive(serde::Deserialize)]
struct Adjust {
    ms: i64,
}

async fn time_adjust(axum::Json(adjust): axum::Json<Adjust>) -> impl axum::response::IntoResponse {
    // A week either way is plenty for a clock that's merely off
    const LIMIT: i64 = 7 * 24 * 3600 * 1000;
    CLOCK_ADJUST_MS.store(
        adjust.ms.clamp(-LIMIT, LIMIT),
        std::sync::atomic::Ordering::Relaxed,
    );
    time().await
}

/// The device's clock, for the sync clock page to show the same time on
/// every screen: Unix ms, and whether it was set from the internet (the Pi
/// has no clock of its own, so offline it may be off)
async fn time() -> impl axum::response::IntoResponse {
    let synced =
        crate::net::network::run("timedatectl", &["show", "-p", "NTPSynchronized", "--value"])
            .await
            .is_ok_and(|v| v.trim() == "yes");
    // Read last, as close to the answer leaving as possible
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    (
        [
            (axum::http::header::CONTENT_TYPE, "application/json"),
            (axum::http::header::CACHE_CONTROL, "no-store"),
        ],
        format!(
            r#"{{"ms":{ms},"synced":{synced},"adjust_ms":{}}}"#,
            CLOCK_ADJUST_MS.load(std::sync::atomic::Ordering::Relaxed)
        ),
    )
}

async fn health() -> impl axum::response::IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        concat!(
            r#"{"status":"ok","version":""#,
            env!("CARGO_PKG_VERSION"),
            r#""}"#
        ),
    )
}

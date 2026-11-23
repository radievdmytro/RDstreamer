use crate::output::processed::Targets;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Serialize;

use crate::settings::{
    InputMode, RtmpInputSettings, Settings, validate_hostname, validate_timezone,
};
use crate::system;

use super::display::overlay_changed;
use super::{ApiError, Services, bad, failed};

#[derive(Serialize)]
pub(super) struct Saved {
    pub(super) settings: Settings,
    /// Something the user should know, e.g. that WiFi is reconnecting
    notice: Option<String>,
}

/// Held from reading the settings to saving them, so a change from the input
/// button can't land between and be lost, or lose the page's
pub(super) static SAVING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(super) async fn get_settings(State(sv): State<Services>) -> Json<Settings> {
    Json(sv.store.get())
}

pub(super) async fn put_all(
    State(sv): State<Services>,
    Json(next): Json<Settings>,
) -> Result<Json<Saved>, ApiError> {
    let _saving = SAVING.lock().await;
    commit(&sv, next).await
}

pub(super) async fn reset(State(sv): State<Services>) -> Result<Json<Saved>, ApiError> {
    let _saving = SAVING.lock().await;
    commit(&sv, Settings::default()).await
}

pub(super) async fn put_section(
    State(sv): State<Services>,
    Path(section): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<Saved>, ApiError> {
    let _saving = SAVING.lock().await;
    // Splice the section into the current settings as JSON and parse the
    // whole thing back, so every section shares one code path
    let mut all = serde_json::to_value(sv.store.get()).expect("settings serialize");
    match all.get_mut(&section) {
        Some(slot) => *slot = body,
        None => {
            return Err(ApiError(
                StatusCode::NOT_FOUND,
                format!("No settings section '{section}'"),
            ));
        }
    }
    let next: Settings =
        serde_json::from_value(all).map_err(|e| bad(format!("Invalid {section} settings: {e}")))?;
    commit(&sv, next).await
}

pub(super) fn validate(s: &Settings) -> Result<(), String> {
    validate_hostname(&s.device.hostname)?;
    validate_timezone(&s.device.timezone)?;
    s.input.uvc.validate()?;
    s.input.rtmp.validate()?;
    s.input.button.validate()?;
    let record = &s.recording.button;
    if s.input.button.enabled
        && record.enabled
        && (record.pin == s.input.button.pin || record.led_pin == Some(s.input.button.pin))
    {
        return Err(format!(
            "GPIO{} is already the record button's; pick another for the input button",
            s.input.button.pin
        ));
    }
    if s.input.mode == InputMode::Rtmp && s.rtsp.enabled && s.rtsp.port == s.input.rtmp.port {
        return Err(format!(
            "Port {} is the RTSP server's; pick another for the RTMP input",
            s.rtsp.port
        ));
    }
    s.overlay.validate()?;
    s.display.look.validate()?;
    s.display.dewarp.validate()?;
    s.display.stabilization.validate()?;
    s.wifi.validate()?;
    s.wifi2.validate()?;
    s.ethernet.validate()?;
    s.rtsp.validate()?;
    s.srt.validate()?;
    s.rtmp.validate()?;
    s.udp.validate()?;
    s.ndi.validate("NDI")?;
    s.omt.validate("OMT")?;
    s.recording.validate()?;
    Ok(())
}

pub(super) async fn commit(sv: &Services, next: Settings) -> Result<Json<Saved>, ApiError> {
    // Also covers reset and import, which bring "System" modes along
    let mut next = next.for_install();
    // Picking RTMP makes up its key, so the input is never open to anyone
    if next.input.mode == InputMode::Rtmp && next.input.rtmp.key.is_empty() {
        next.input.rtmp.key = RtmpInputSettings::new_key();
    }
    validate(&next).map_err(bad)?;
    let prev = sv.store.get();
    let mut notice = None;
    // Which picture an output takes isn't the output's business: switching
    // it mustn't restart the server and drop its viewers
    let was = {
        let mut w = prev.clone();
        w.rtsp.processed = next.rtsp.processed;
        w.srt.processed = next.srt.processed;
        w.rtmp.processed = next.rtmp.processed;
        w.udp.processed = next.udp.processed;
        w
    };

    if next.rtsp != was.rtsp {
        sv.rtsp.apply(&next.rtsp).map_err(|e| {
            let _ = sv.rtsp.apply(&prev.rtsp);
            failed(format!("RTSP server: {e}"))
        })?;
    }

    if next.srt != was.srt
        && let Err(e) = sv.srt.apply(&next.srt)
    {
        if next.rtsp != was.rtsp {
            let _ = sv.rtsp.apply(&prev.rtsp);
        }
        let _ = sv.srt.apply(&prev.srt);
        return Err(failed(format!("SRT: {e}")));
    }

    if next.udp != was.udp
        && let Err(e) = sv.udp.apply(&next.udp)
    {
        if next.rtsp != was.rtsp {
            let _ = sv.rtsp.apply(&prev.rtsp);
        }
        if next.srt != was.srt {
            let _ = sv.srt.apply(&prev.srt);
        }
        let _ = sv.udp.apply(&prev.udp);
        return Err(failed(format!("UDP: {e}")));
    }

    // Both refuse to start without what they need on the device: the NDI
    // runtime, or a free port for OMT
    let undo_streams = |sv: &Services| {
        if next.rtsp != was.rtsp {
            let _ = sv.rtsp.apply(&prev.rtsp);
        }
        if next.srt != was.srt {
            let _ = sv.srt.apply(&prev.srt);
        }
        if next.udp != was.udp {
            let _ = sv.udp.apply(&prev.udp);
        }
    };
    if next.ndi != prev.ndi
        && let Err(e) = sv.ndi.apply(&next.ndi)
    {
        undo_streams(sv);
        let _ = sv.ndi.apply(&prev.ndi);
        return Err(failed(format!("NDI: {e}")));
    }
    if next.omt != prev.omt
        && let Err(e) = sv.omt.apply(&next.omt)
    {
        undo_streams(sv);
        if next.ndi != prev.ndi {
            let _ = sv.ndi.apply(&prev.ndi);
        }
        let _ = sv.omt.apply(&prev.omt);
        return Err(failed(format!("OMT: {e}")));
    }

    if next.input != prev.input {
        sv.input.set_mode(next.input.mode);
        sv.uvc.apply(&next.input.uvc);
        if next.input.rtmp != prev.input.rtmp {
            sv.rtmp_input.apply(&next.input.rtmp);
        }
        sv.passthrough.set_enabled(next.input.passthrough);
    }

    // Never refused: an unreachable server is retried in the background
    if next.rtmp != was.rtmp {
        sv.rtmp.apply(&next.rtmp);
    }

    let display_changed = next.display != prev.display;
    let rollback = |sv: &Services| {
        if next.rtsp != was.rtsp {
            let _ = sv.rtsp.apply(&prev.rtsp);
        }
        if next.srt != was.srt {
            let _ = sv.srt.apply(&prev.srt);
        }
        if next.rtmp != was.rtmp {
            sv.rtmp.apply(&prev.rtmp);
        }
        if next.udp != was.udp {
            let _ = sv.udp.apply(&prev.udp);
        }
        if next.ndi != prev.ndi {
            let _ = sv.ndi.apply(&prev.ndi);
        }
        if next.omt != prev.omt {
            let _ = sv.omt.apply(&prev.omt);
        }
        if next.input != prev.input {
            sv.input.set_mode(prev.input.mode);
            sv.uvc.apply(&prev.input.uvc);
            if next.input.rtmp != prev.input.rtmp {
                sv.rtmp_input.apply(&prev.input.rtmp);
            }
            sv.passthrough.set_enabled(prev.input.passthrough);
        }
        if display_changed {
            let _ = sv.player.apply(&prev.display);
        }
    };

    sv.player.processed().set_targets(Targets::of(&next));
    if display_changed && let Err(e) = sv.player.apply(&next.display) {
        rollback(sv);
        sv.player.processed().set_targets(Targets::of(&prev));
        return Err(failed(format!("HDMI output: {e}")));
    }
    // A new target may want it running, or none any more
    sv.player.processed().apply(&next.display);

    let timezone_changed = next.device.timezone != prev.device.timezone;
    if timezone_changed && let Err(e) = system::set_timezone(&next.device.timezone).await {
        rollback(sv);
        return Err(failed(format!("Couldn't set the timezone: {e}")));
    }

    if next.device.hostname != prev.device.hostname {
        if let Err(e) = system::set_hostname(&next.device.hostname).await {
            rollback(sv);
            if timezone_changed {
                let _ = system::set_timezone(&prev.device.timezone).await;
            }
            return Err(failed(format!("Couldn't rename the device: {e}")));
        }
        notice = Some(format!("Now reachable as {}.local", next.device.hostname));
    }

    let saved = sv.store.update(|s| *s = next.clone()).map_err(|e| {
        rollback(sv);
        failed(e)
    })?;

    if saved.input.button != prev.input.button {
        sv.input_button.configure((&saved.input.button).into());
    }

    if saved.recording != prev.recording {
        sv.recorder.configure(&saved.recording);
        if saved.recording.button != prev.recording.button {
            sv.button.configure((&saved.recording.button).into());
        }
    }

    if saved.overlay != prev.overlay {
        sv.overlay.set_cfg(&saved.overlay);
        overlay_changed(sv);
    }

    if saved.wifi != prev.wifi {
        sv.network.apply_later(saved.wifi.clone());
        notice = Some(
            "Applying WiFi settings. If you're connected over WiFi, you may need to reconnect."
                .into(),
        );
    }

    if saved.wifi2 != prev.wifi2 {
        sv.network.apply_wifi2_later(saved.wifi2.clone());
    }

    if saved.ethernet != prev.ethernet {
        sv.network.apply_ethernet_later(saved.ethernet.clone());
        notice = Some(match saved.ethernet.mode {
            crate::settings::EthernetMode::Static => format!(
                "Applying Ethernet settings. Over Ethernet, this page moves to http://{}",
                saved.ethernet.address.split('/').next().unwrap_or_default()
            ),
            crate::settings::EthernetMode::Auto => {
                "Applying Ethernet settings. If you're connected over Ethernet, the address may change."
                    .into()
            }
            crate::settings::EthernetMode::System => {
                "Handing Ethernet back to the system's own settings.".into()
            }
        });
    }

    Ok(Json(Saved {
        settings: saved,
        notice,
    }))
}

#[cfg(test)]
#[path = "settings.test.rs"]
mod tests;

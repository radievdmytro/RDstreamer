use super::*;

fn network(wifi: InterfaceStatus, ethernet: Option<InterfaceStatus>) -> NetworkStatus {
    NetworkStatus {
        wifi,
        ethernet,
        wifi2: None,
        wifi2_error: None,
        tether: None,
        internet_via: None,
        iphone_support: false,
        image: false,
        fallback_active: false,
        fallback_reason: None,
        fallback_resolved: false,
        applying: false,
        last_error: None,
    }
}

fn access_point() -> (WifiSettings, NetworkStatus) {
    let mut wifi = WifiSettings {
        mode: WifiMode::Ap,
        ..Default::default()
    };
    wifi.ap.ssid = "YStreamer".into();
    wifi.ap.password = "flyfast123".into();
    let status = network(
        InterfaceStatus {
            addresses: vec!["10.42.0.1/24".into()],
            role: Some("ap".into()),
            ssid: Some("YStreamer".into()),
            ..Default::default()
        },
        Some(InterfaceStatus {
            addresses: vec!["192.168.1.20/24".into()],
            ..Default::default()
        }),
    );
    (wifi, status)
}

#[test]
fn band_over_an_image_keeps_what_is_needed_to_get_in() {
    let (wifi, status) = access_point();
    let svg = describe(80, LinkState::Unplugged, &wifi, &status, true).over_image(
        "data:image/png;base64,AAAA",
        Scaling::Fit,
        Layout::Band,
    );

    assert!(svg.contains(r#"href="data:image/png;base64,AAAA""#));
    assert!(svg.contains("Connect the goggles to the USB-C port"));
    assert!(svg.contains("10.42.0.1"));
    assert!(svg.contains("192.168.1.20"));
    assert!(svg.contains("flyfast123"));
}

#[test]
fn band_over_an_image_leaves_out_the_branding() {
    let (wifi, status) = access_point();
    let svg = describe(80, LinkState::Unplugged, &wifi, &status, true).over_image(
        "data:image/png;base64,AAAA",
        Scaling::Fit,
        Layout::Band,
    );

    assert!(!svg.contains("SCAN TO JOIN"));
    assert!(!svg.contains("streamer</tspan>"));
}

#[test]
fn image_under_the_band_is_fitted_or_fills_the_screen() {
    let (wifi, status) = access_point();
    let screen = describe(80, LinkState::Unplugged, &wifi, &status, true);
    assert!(
        screen
            .over_image("data:,", Scaling::Fit, Layout::Band)
            .contains("xMidYMid meet")
    );
    assert!(
        screen
            .over_image("data:,", Scaling::Fill, Layout::Band)
            .contains("xMidYMid slice")
    );
}

#[test]
fn access_point_password_can_be_kept_off_the_screen() {
    let (wifi, status) = access_point();
    let screen = describe(80, LinkState::Unplugged, &wifi, &status, false);
    for svg in [
        screen.svg(),
        screen.over_image("data:,", Scaling::Fit, Layout::Band),
    ] {
        assert!(!svg.contains("flyfast123"));
        assert!(!svg.contains("SCAN TO JOIN"));
        // still says which network to join and where to go
        assert!(svg.contains("YStreamer"));
        assert!(svg.contains("10.42.0.1"));
    }
    assert!(
        describe(80, LinkState::Unplugged, &wifi, &status, true)
            .svg()
            .contains("SCAN TO JOIN")
    );
}

#[test]
fn the_details_fit_in_a_corner_box() {
    let (wifi, status) = access_point();
    let screen = describe(80, LinkState::Unplugged, &wifi, &status, true);
    for corner in [
        Corner::TopLeft,
        Corner::TopRight,
        Corner::BottomLeft,
        Corner::BottomRight,
    ] {
        let svg = screen.over_video(Layout::Corner(corner));
        assert!(svg.contains("10.42.0.1"));
        assert!(svg.contains("flyfast123"));
        // a small box, not the band across the screen
        let w: u32 = svg
            .split(r#"rx="14""#)
            .next()
            .and_then(|head| head.rsplit("width=\"").next())
            .and_then(|v| v.split('"').next())
            .and_then(|v| v.parse().ok())
            .expect("the box's width");
        assert!(w < WIDTH / 2, "{w}");
    }
}

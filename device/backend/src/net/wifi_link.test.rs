use super::*;

const INFO_AP: &str = "Interface wlan0
\tifindex 3
\twdev 0x1
\taddr dc:a6:32:00:00:01
\tssid RDstreamer
\ttype AP
\twiphy 0
\tchannel 6 (2437 MHz), width: 20 MHz, center1: 2437 MHz
\ttxpower 31.00 dBm
";

const INFO_CLIENT: &str = "Interface wlan0
\tifindex 3
\taddr dc:a6:32:00:00:01
\tssid Home
\ttype managed
\tchannel 11 (2462 MHz), width: 20 MHz, center1: 2462 MHz
";

#[test]
fn reads_the_role_network_and_channel() {
    let ap = parse_info(INFO_AP);
    assert_eq!(ap.role, Role::Ap);
    assert_eq!(ap.ssid.as_deref(), Some("RDstreamer"));
    assert_eq!(ap.channel, Some(6));

    let client = parse_info(INFO_CLIENT);
    assert_eq!(client.role, Role::Client);
    assert_eq!(client.channel, Some(11));

    // Managed but not connected to anything
    let idle = parse_info("Interface wlan0\n\ttype managed\n");
    assert_eq!(idle.role, Role::Off);
}

#[test]
fn reads_the_routers_signal() {
    let out = "Connected to 11:22:33:44:55:66 (on wlan0)
\tSSID: Home
\tfreq: 2462
\tsignal: -58 dBm
\trx bitrate: 65.0 MBit/s MCS 7
\ttx bitrate: 72.2 MBit/s MCS 7 short GI
";
    assert_eq!(parse_link(out), (Some(-58), Some(72.2)));
    assert_eq!(parse_link("Not connected."), (None, None));
}

#[test]
fn names_the_access_points_clients() {
    let dump = "Station AA:BB:CC:00:00:01 (on wlan0)
\tinactive time:\t120 ms
\tsignal:  \t-44 [-44, -47] dBm
\ttx bitrate:\t65.0 MBit/s MCS 6
Station aa:bb:cc:00:00:02 (on wlan0)
\tsignal:  \t-71 dBm
";
    let leases = "1795400000 aa:bb:cc:00:00:01 10.0.0.23 iPhone-Dmytro 01:aa:bb\n";
    let arp = "IP address       HW type     Flags       HW address            Mask     Device
10.0.0.42        0x1         0x2         aa:bb:cc:00:00:02     *        wlan0
";
    let st = parse_stations(dump, leases, arp);
    assert_eq!(st.len(), 2);
    assert_eq!(st[0].name.as_deref(), Some("iPhone-Dmytro"));
    assert_eq!(st[0].ip.as_deref(), Some("10.0.0.23"));
    assert_eq!(st[0].signal_dbm, Some(-44));
    assert_eq!(st[0].bitrate_mbps, Some(65.0));
    assert_eq!(st[1].name, None);
    assert_eq!(st[1].ip.as_deref(), Some("10.0.0.42"));
    assert_eq!(st[1].signal_dbm, Some(-71));
}

const SCAN: &str = "BSS 11:22:33:44:55:01(on wlan0)
\tlast seen: 120 ms ago
\tfreq: 2412
\tsignal: -45.00 dBm
\tSSID: Neighbour
\tDS Parameter set: channel 1
BSS 66:77:88:99:aa:02(on wlan0) -- associated
\tfreq: 2412.0
\tsignal: -60.00 dBm
\tSSID: Other
BSS 11:22:33:44:55:03(on wlan0)
\tfreq: 2437
\tsignal: -85.00 dBm
\tSSID: \\x00\\x00\\x00
BSS 11:22:33:44:55:04(on wlan0)
\tfreq: 5180
\tsignal: -50.00 dBm
\tSSID: Fast
";

#[test]
fn reads_a_scan() {
    let found = parse_scan(SCAN);
    assert_eq!(found.len(), 4);
    assert_eq!(found[0].bssid, "11:22:33:44:55:01");
    assert_eq!(found[0].channel, 1);
    assert_eq!(found[0].signal_dbm, -45);
    assert_eq!(found[0].ssid, "Neighbour");
    assert_eq!(found[1].freq, 2412);
    assert_eq!(found[2].ssid, "", "hidden");
    assert_eq!(found[2].channel, 6);
    assert_eq!(found[3].channel, 36);
}

#[test]
fn picks_the_clearest_channel_that_doesnt_overlap() {
    let survey = summarise(parse_scan(SCAN), Some(1));
    assert_eq!(survey.networks.len(), 3, "5 GHz kept apart");
    assert_eq!(survey.networks_5ghz, 1);
    assert_eq!(survey.networks[0].signal_dbm, -45, "strongest first");
    // Two loud networks on 1, one faint one on 6, nothing near 11
    assert_eq!(survey.best, 11);
    let load = |c: u32| survey.channels.iter().find(|l| l.channel == c).unwrap();
    assert_eq!(load(1).networks, 2);
    assert!(load(1).load > load(6).load);
    assert!(load(6).load > load(11).load);
    assert_eq!(load(11).load, 0.0);
    assert_eq!(survey.channels.len(), 13);
}

#[test]
fn overlap_fades_over_four_channels() {
    let one = [Bss {
        channel: 6,
        freq: 2437,
        signal_dbm: -40,
        ..Default::default()
    }];
    assert_eq!(load_on(6, &one), 1.0);
    assert!((load_on(8, &one) - 0.6).abs() < 1e-6);
    assert_eq!(load_on(11, &one), 0.0);
    assert_eq!(load_on(1, &one), 0.0);
}

#[test]
fn finds_where_the_internet_goes() {
    let out = "default via 192.168.0.1 dev eth0 proto dhcp src 192.168.0.190 metric 100\n\
               default via 10.0.0.1 dev wlan0 metric 600\n";
    assert_eq!(parse_default_route(out).as_deref(), Some("eth0"));
    assert_eq!(parse_default_route(""), None);
}

#[test]
fn explains_scan_errors() {
    assert!(scan_error("command failed: Operation not supported (-95)").contains("can't scan"));
    assert!(scan_error("command failed: Device or resource busy (-16)").contains("busy"));
}

#[test]
fn a_second_interface_gets_its_own_address() {
    assert_eq!(
        local_address("dc:a6:32:00:00:01").as_deref(),
        Some("de:a6:32:00:00:01")
    );
    // Already locally administered: told apart by the last octet
    assert_eq!(
        local_address("de:a6:32:00:00:01").as_deref(),
        Some("de:a6:32:00:00:00")
    );
    assert_eq!(local_address("nonsense"), None);
}

#[test]
fn the_chip_refusing_to_scan_is_explained() {
    assert!(scan_error("command failed: Invalid exchange (-52)").contains("can't scan"));
}

#[test]
fn a_router_with_two_names_is_one_radio() {
    let b = |bssid: &str, channel: u32, signal_dbm: i32| Bss {
        bssid: bssid.into(),
        channel,
        freq: 2407 + channel * 5,
        signal_dbm,
        ..Default::default()
    };
    let nets = vec![
        b("50:c7:bf:00:11:20", 9, -54),
        b("50:c7:bf:00:11:21", 9, -58), // same router, next address
        b("52:c7:bf:00:11:20", 9, -60), // same router, local bit
        b("50:c7:bf:00:11:21", 1, -60), // other channel: another radio
        b("aa:bb:cc:dd:ee:ff", 9, -70),
    ];
    let r = radios(&nets);
    assert_eq!(r.len(), 3);
    assert_eq!(r[0].signal_dbm, -54);
}

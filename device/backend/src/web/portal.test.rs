use super::*;

#[test]
fn knows_the_internet_check_names() {
    assert!(is_probe_host("captive.apple.com"));
    assert!(is_probe_host("connectivitycheck.gstatic.com:80"));
    assert!(is_probe_host("WWW.MSFTCONNECTTEST.COM"));
    assert!(!is_probe_host("10.0.0.1"));
    assert!(!is_probe_host("rdstreamer.local"));
}

#[test]
fn sends_those_names_to_the_device() {
    let conf = dnsmasq_conf("10.0.0.1");
    assert!(conf.contains("address=/captive.apple.com/10.0.0.1\n"));
    assert_eq!(conf.matches("address=").count(), PROBE_HOSTS.len());
}

#[test]
fn answers_each_kind_of_device_as_it_expects() {
    assert_eq!(expected("/generate_204").status(), StatusCode::NO_CONTENT);
    assert_eq!(expected("/hotspot-detect.html").status(), StatusCode::OK);
    assert_eq!(expected("/connecttest.txt").status(), StatusCode::OK);
}

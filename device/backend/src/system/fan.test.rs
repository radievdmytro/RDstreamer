use super::*;

fn p(temp_c: u8, speed: u8) -> Point {
    Point { temp_c, speed }
}

#[test]
fn reads_argononed_curves() {
    let text = "# Температура (°C)=Скорость вентилятора (%)\n\
                45=5\n55 = 25\n\n60=55 # comment\n65=100\nnonsense\n";
    assert_eq!(
        parse(text),
        vec![p(45, 5), p(55, 25), p(60, 55), p(65, 100)]
    );
    assert_eq!(
        parse("60=50\n40=10\n-3=5\n300=1\n"),
        vec![p(40, 10), p(60, 50)]
    );
}

#[test]
fn the_quiet_curve_is_allowed() {
    assert_eq!(validate(&QUIET), Ok(()));
}

#[test]
fn refuses_curves_that_could_overheat() {
    assert!(validate(&[]).is_err());
    assert!(
        validate(&[p(45, 5), p(70, 60)]).is_err(),
        "never full speed"
    );
    assert!(
        validate(&[p(45, 5), p(80, 100)]).is_err(),
        "full speed too late"
    );
    assert!(
        validate(&[p(20, 5), p(60, 100)]).is_err(),
        "below the range"
    );
    assert!(
        validate(&[p(45, 5), p(90, 100)]).is_err(),
        "above the range"
    );
    assert!(validate(&[p(45, 101), p(60, 100)]).is_err());
    assert!(validate(&[p(60, 5), p(50, 100)]).is_err(), "out of order");
    assert!(
        validate(&[p(50, 5), p(50, 100)]).is_err(),
        "same temperature"
    );
    assert!(
        validate(&[p(50, 60), p(60, 30), p(65, 100)]).is_err(),
        "slower when hotter"
    );
    assert!(validate(&[p(40, 10); MAX_POINTS + 1]).is_err());
}

#[test]
fn speed_follows_the_curve() {
    assert_eq!(speed_at(&QUIET, 38.0), 0);
    assert_eq!(speed_at(&QUIET, 45.0), 5);
    assert_eq!(speed_at(&QUIET, 57.3), 25);
    assert_eq!(speed_at(&QUIET, 80.0), 100);
    assert_eq!(speed_at(&[], 80.0), 0);
}

#[test]
fn speeds_up_at_once_but_slows_down_only_after_a_while() {
    let t0 = Instant::now();
    let at = |s: u64| t0 + Duration::from_secs(s);
    let mut g = Governor::default();

    assert_eq!(
        g.next(&QUIET, Some(40.0), at(0)),
        Some(0),
        "first reading is sent"
    );
    assert_eq!(g.next(&QUIET, Some(40.0), at(2)), None, "nothing new");
    assert_eq!(
        g.next(&QUIET, Some(56.0), at(4)),
        Some(25),
        "hotter: straight away"
    );

    // Dips below 55 °C for a moment, then back: no change at all
    assert_eq!(g.next(&QUIET, Some(54.0), at(6)), None);
    assert_eq!(g.next(&QUIET, Some(55.5), at(10)), None);

    // Stays cooler: slows down after 30 s, not before
    assert_eq!(g.next(&QUIET, Some(50.0), at(12)), None);
    assert_eq!(g.next(&QUIET, Some(50.0), at(40)), None);
    assert_eq!(g.next(&QUIET, Some(50.0), at(42)), Some(5));
    assert_eq!(g.previous, Some(25));

    // The same speed again after five minutes, in case the case restarted
    assert_eq!(g.next(&QUIET, Some(50.0), at(42 + 300)), Some(5));
}

#[test]
fn without_a_temperature_it_cools() {
    let mut g = Governor::default();
    assert_eq!(g.next(&QUIET, None, Instant::now()), Some(100));
    let mut g = Governor::default();
    g.next(&QUIET, Some(40.0), Instant::now());
    assert_eq!(g.next(&QUIET, None, Instant::now()), Some(50));
}

#[test]
fn power_button_pulses() {
    assert_eq!(classify(5), None, "noise");
    assert_eq!(classify(20), Some(Press::Reboot));
    assert_eq!(classify(30), Some(Press::Reboot));
    assert_eq!(classify(40), Some(Press::Poweroff));
    assert_eq!(classify(50), Some(Press::Poweroff));
    assert_eq!(classify(70), None);
}

#[test]
fn who_runs_the_fan() {
    use Control::*;
    assert_eq!(decide(Mode::Auto, true, false, false), Driving);
    assert_eq!(
        decide(Mode::Auto, true, true, false),
        Argononed,
        "never fights argononed"
    );
    assert_eq!(decide(Mode::On, true, true, false), Argononed);
    assert_eq!(decide(Mode::Auto, false, false, false), NoCase);
    assert_eq!(
        decide(Mode::Auto, true, false, true),
        Hat,
        "a HAT may own 0x1a"
    );
    assert_eq!(
        decide(Mode::On, true, false, true),
        Driving,
        "unless told otherwise"
    );
    assert_eq!(decide(Mode::Off, true, false, false), Off);
}

#[test]
fn config_falls_back_to_the_defaults() {
    let dir = std::env::temp_dir().join(format!("fan-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fan.json");
    assert_eq!(load_config(&path), FanConfig::default(), "nothing saved");

    fs::write(&path, r#"{"mode":"off","curve":[{"temp_c":45,"speed":5}]}"#).unwrap();
    let c = load_config(&path);
    assert_eq!(c.mode, Mode::Off, "what was saved is kept");
    assert_eq!(c.curve, QUIET.to_vec(), "but a curve that can't pass isn't");
    assert!(c.power_button, "and what's missing comes from the defaults");

    let c = FanConfig {
        spin_up: true,
        ..Default::default()
    };
    save_config(&path, &c).unwrap();
    assert_eq!(load_config(&path), c);
    fs::remove_dir_all(&dir).unwrap();
}

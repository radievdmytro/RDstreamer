use super::*;

fn p(temp_c: u8, speed: u8) -> Point {
    Point { temp_c, speed }
}

#[test]
fn reads_what_argon_writes() {
    let text = "# Температура (°C)=Скорость вентилятора (%)\n\
                45=5\n55 = 25\n\n60=55 # comment\n65=100\nnonsense\n";
    assert_eq!(
        parse(text),
        vec![p(45, 5), p(55, 25), p(60, 55), p(65, 100)]
    );
}

#[test]
fn sorts_and_skips_what_argononed_would() {
    assert_eq!(
        parse("60=50\n40=10\n-3=5\n300=1\n"),
        vec![p(40, 10), p(60, 50)]
    );
}

#[test]
fn writes_what_it_reads() {
    assert_eq!(parse(&render(&QUIET)), QUIET.to_vec());
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
fn speed_follows_argononed() {
    assert_eq!(speed_at(&QUIET, 38.0), 0);
    assert_eq!(speed_at(&QUIET, 45.0), 5);
    assert_eq!(speed_at(&QUIET, 57.3), 25);
    assert_eq!(speed_at(&QUIET, 80.0), 100);
    assert_eq!(speed_at(&[], 80.0), 0);
}

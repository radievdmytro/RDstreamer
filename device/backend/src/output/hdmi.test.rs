use super::*;

#[test]
fn a_frozen_screen_is_noticed_after_a_few_seconds() {
    let mut f = Frozen::default();
    // Showing pictures as they come
    assert!(!f.update(60, 58));
    assert!(!f.update(120, 117));
    // Pictures still coming, the screen stuck
    assert!(!f.update(180, 117));
    assert!(!f.update(240, 117));
    assert!(f.update(300, 117));
}

#[test]
fn no_video_is_not_a_frozen_screen() {
    let mut f = Frozen::default();
    assert!(!f.update(60, 60));
    // The goggles stopped sending: nothing fed, nothing shown
    for _ in 0..10 {
        assert!(!f.update(60, 60));
    }
    // The splash screen's still picture: a trickle
    for i in 0..10 {
        assert!(!f.update(61 + i, 60));
    }
}

#[test]
fn a_moment_of_freeze_is_forgiven() {
    let mut f = Frozen::default();
    assert!(!f.update(60, 60));
    assert!(!f.update(120, 60));
    assert!(!f.update(180, 60));
    // Back before the third second
    assert!(!f.update(240, 120));
    assert!(!f.update(300, 120));
    assert!(!f.update(360, 120));
}

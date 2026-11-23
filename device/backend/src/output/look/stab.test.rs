use super::*;

/// A textured scene, in shader units (y -1..1)
fn scene(x: f32, y: f32) -> f32 {
    128.0
        + 40.0 * (x * 7.3).sin() * (y * 5.1).cos()
        + 30.0 * (x * 2.9 + y * 3.7).sin()
        + 20.0 * ((x - y) * 11.0).cos()
}

/// The scene as seen by a camera: pixel p shows scene(R(angle)·p + shift)
fn picture(shift: (f32, f32), angle: f32) -> Small {
    let (w, h) = (640usize, 360usize);
    let aspect = w as f32 / h as f32;
    let mut plane = vec![0u8; w * h];
    for py in 0..h {
        for px in 0..w {
            let x = ((px as f32 + 0.5) / w as f32 * 2.0 - 1.0) * aspect;
            let y = (py as f32 + 0.5) / h as f32 * 2.0 - 1.0;
            let (s, c) = angle.sin_cos();
            let (rx, ry) = (x * c - y * s, x * s + y * c);
            plane[py * w + px] = scene(rx + shift.0, ry + shift.1).clamp(0.0, 255.0) as u8;
        }
    }
    Small::from_luma(&plane, w, w, h).unwrap()
}

#[test]
fn finds_how_far_the_picture_moved() {
    let a = picture((0.0, 0.0), 0.0);
    let b = picture((0.08, -0.05), 0.0);
    let m = estimate(&a, &b, true).unwrap();
    assert!((m.x - 0.08).abs() < 0.015, "{m:?}");
    assert!((m.y + 0.05).abs() < 0.015, "{m:?}");
    assert!(m.angle.abs() < 0.01, "{m:?}");
}

#[test]
fn finds_how_far_it_turned() {
    let a = picture((0.0, 0.0), 0.0);
    let b = picture((0.0, 0.0), 0.03);
    let m = estimate(&a, &b, true).unwrap();
    assert!((m.angle - 0.03).abs() < 0.01, "{m:?}");
    // Turning not asked for
    assert_eq!(estimate(&a, &b, false).unwrap().angle, 0.0);
}

#[test]
fn a_flat_picture_gives_nothing_to_go_by() {
    let plane = vec![100u8; 640 * 360];
    let flat = Small::from_luma(&plane, 640, 640, 360).unwrap();
    assert!(estimate(&flat, &flat, true).is_none());
}

fn motion(x: f32, y: f32, angle: f32) -> Option<Motion> {
    Some(Motion { x, y, angle })
}

const FRAME: u64 = 16_666_667;

#[test]
fn shake_is_undone_and_a_pan_let_through() {
    let t = Tuning::default();
    let mut path = Path::default();
    // Shaking left and right: the correction leans against each jolt
    for i in 0..40u64 {
        let jolt = if i % 2 == 0 { 0.02 } else { -0.02 };
        path.step(i * FRAME, motion(jolt, 0.0, 0.0), &t);
        let c = path.correction(i * FRAME, &t);
        if i > 4 {
            assert!(c.offset[0] * jolt < 0.0, "against the jolt at {i}: {c:?}");
        }
    }
    // A steady pan: the correction stays within the zoom's room
    for i in 40..300u64 {
        path.step(i * FRAME, motion(0.02, 0.0, 0.0), &t);
        let c = path.correction(i * FRAME, &t);
        assert!(c.offset[0].abs() <= t.margin() + 1e-6);
        assert!(c.zoom > 1.0);
    }
}

#[test]
fn looking_ahead_follows_a_pan_without_lagging() {
    let live = Tuning::default();
    let ahead = Tuning {
        delay_ms: 500,
        ..live
    };
    let mut path = Path::default();
    // A steady pan, with a little shake on it
    for i in 0..120u64 {
        let wobble = if i % 2 == 0 { 0.004 } else { -0.004 };
        path.step(i * FRAME, motion(0.01 + wobble, 0.0, 0.0), &live);
    }
    // In the middle, with half a second known either side: a pan is
    // straight, so the centred average sits on it (small correction); live,
    // following behind, lags it
    let mid = 60 * FRAME;
    let centred = path.correction(mid, &ahead).offset[0].abs();
    let behind = path.correction(mid, &live).offset[0].abs();
    assert!(centred < behind, "{centred} vs {behind}");
    assert!(centred < 0.01);
}

#[test]
fn the_horizon_is_left_alone_when_off() {
    let t = Tuning {
        horizon: 0,
        ..Tuning::default()
    };
    let mut path = Path::default();
    for i in 0..30u64 {
        path.step(
            i * FRAME,
            motion(0.0, 0.0, if i % 2 == 0 { 0.01 } else { -0.01 }),
            &t,
        );
    }
    assert_eq!(path.correction(29 * FRAME, &t).angle, 0.0);
}

#[test]
fn a_jolt_too_big_to_be_motion_starts_again() {
    let t = Tuning::default();
    let mut path = Path::default();
    for i in 0..20u64 {
        path.step(i * FRAME, motion(0.01, 0.0, 0.0), &t);
    }
    // A cut: a huge jump, not followed
    path.step(20 * FRAME, motion(0.9, 0.0, 0.0), &t);
    let c = path.correction(20 * FRAME, &t);
    assert_eq!(c.offset, [0.0, 0.0]);
}

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

#[test]
fn shake_is_undone_and_a_pan_let_through() {
    let mut path = Path::default();
    // Shaking left and right: the correction leans against each jolt
    let mut last = 0.0;
    for i in 0..40 {
        let jolt = if i % 2 == 0 { 0.04 } else { -0.04 };
        let c = path.step(
            Some(Motion {
                x: jolt,
                y: 0.0,
                angle: 0.0,
            }),
            80,
        );
        if i > 4 {
            assert!(c.offset[0] * jolt < 0.0, "against the jolt");
        }
        last = c.offset[0];
    }
    assert!(last.abs() < 0.05);
    // A steady pan: the correction stays within the zoom's room
    for _ in 0..200 {
        let c = path.step(
            Some(Motion {
                x: 0.02,
                y: 0.0,
                angle: 0.0,
            }),
            80,
        );
        assert!(c.offset[0].abs() <= margin(80) + 1e-6);
        assert!(c.zoom > 1.0);
    }
}

#[test]
fn losing_track_resets_without_a_jump_later() {
    let mut path = Path::default();
    path.step(
        Some(Motion {
            x: 0.1,
            y: 0.0,
            angle: 0.0,
        }),
        50,
    );
    let c = path.step(None, 50);
    assert_eq!(c.offset, [0.0, 0.0]);
}

#[test]
fn the_correction_found_is_the_pictures_own() {
    let c = Correction {
        offset: [0.1, 0.2],
        angle: 0.01,
        zoom: 1.1,
    };
    publish(Some(42), c);
    publish(Some(43), Correction::NONE);
    assert_eq!(correction_for(Some(42)), c);
    // Not listed: the latest
    assert_eq!(correction_for(Some(7)), Correction::NONE);
}

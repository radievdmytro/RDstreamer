use super::*;

#[test]
fn reads_what_ffprobe_says() {
    let json = r#"{"streams":[{"codec_name":"h264","width":1920,"height":1080,
        "r_frame_rate":"60/1","avg_frame_rate":"30000/1001"}]}"#;
    let p = parse_probe(json).unwrap();
    assert_eq!(p.codec, "h264");
    assert_eq!((p.width, p.height), (1920, 1080));
    assert!((p.fps - 29.97).abs() < 0.01);
    assert!(parse_probe(r#"{"streams":[]}"#).is_none());
}

#[test]
fn only_what_the_pi_cant_decode_is_converted() {
    let p = |codec: &str, w, h, fps| Probe {
        codec: codec.into(),
        width: w,
        height: h,
        fps,
    };
    assert!(!needs_converting(&p("h264", 1920, 1080, 30.0)));
    assert!(!needs_converting(&p("h264", 1280, 720, 60.0)));
    assert!(needs_converting(&p("h264", 3840, 2160, 30.0)), "4K");
    assert!(needs_converting(&p("hevc", 1920, 1080, 30.0)));
    assert!(needs_converting(&p("h264", 1920, 1080, 120.0)));
    assert_eq!(fit(3840, 2160), (1920, 1080));
    assert_eq!(fit(1280, 720), (1280, 720));
}

#[test]
fn plays_pictures_in_a_loop() {
    // Delimiter, parameter sets and a keyframe, then two more pictures
    let aud = [0u8, 0, 0, 1, 9, 0xf0];
    let mut clip = Vec::new();
    clip.extend_from_slice(&aud);
    clip.extend_from_slice(&[0, 0, 0, 1, 0x67, 1, 2, 3]);
    clip.extend_from_slice(&[0, 0, 0, 1, 0x68, 4]);
    clip.extend_from_slice(&[0, 0, 0, 1, 0x65, 0x88, 1]);
    for _ in 0..2 {
        clip.extend_from_slice(&aud);
        clip.extend_from_slice(&[0, 0, 0, 1, 0x41, 0x9a, 2]);
    }
    let dir = std::env::temp_dir().join(format!("rd-splash-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("clip.h264");
    fs::write(&path, &clip).unwrap();

    let mut player = Player::new(path, 60.0);
    let pictures: Vec<Vec<u8>> = (0..7)
        .map(|_| player.next_picture().unwrap().unwrap())
        .collect();
    // 3 pictures, then the same again from the top
    let keyframes: Vec<bool> = pictures
        .iter()
        .map(|p| crate::input::h264::has_keyframe(p))
        .collect();
    assert_eq!(
        keyframes,
        [true, false, false, true, false, false, true],
        "{pictures:?}"
    );
    // Each keyframe comes with its parameter sets
    assert!(pictures[3].windows(5).any(|w| w == [0, 0, 0, 1, 0x67]));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn an_empty_clip_plays_nothing() {
    let dir = std::env::temp_dir().join(format!("rd-splash-empty-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("clip.h264");
    fs::write(&path, b"").unwrap();
    let mut player = Player::new(path, 30.0);
    assert_eq!(player.next_picture().unwrap(), None);
    let _ = fs::remove_dir_all(&dir);
}

/// With ffmpeg here: a real file made ready, then played picture by picture
#[test]
fn converts_real_videos_when_ffmpeg_is_here() {
    let Ok(input) = std::env::var("RD_TEST_CLIP") else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("rd-splash-real-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let out = dir.join("clip.h264");
    let info = convert(Path::new(&input), &out).unwrap();
    eprintln!("{info:?}");
    assert!(info.width <= MAX_WIDTH && info.height <= MAX_HEIGHT);
    let mut player = Player::new(out, info.fps);
    let mut keyframes = 0;
    for _ in 0..90 {
        let au = player.next_picture().unwrap().unwrap();
        if crate::input::h264::has_keyframe(&au) {
            keyframes += 1;
            // Parameter sets with every keyframe
            assert!(crate::input::h264::nals(&au).any(|n| n.kind == crate::input::h264::SPS));
        }
    }
    assert!(keyframes >= 2, "loops back to a keyframe");
    let _ = fs::remove_dir_all(&dir);
}

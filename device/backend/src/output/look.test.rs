use super::*;

const IDENTITY_2: &str = "TITLE \"Identity\"\n\
LUT_3D_SIZE 2\n\
# red changes fastest\n\
0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n";

#[test]
fn reads_a_cube_file() {
    let lut = parse_cube(IDENTITY_2).unwrap();
    assert_eq!(lut.size, 2);
    assert_eq!(lut.title.as_deref(), Some("Identity"));
    assert_eq!(lut.data[1], [1.0, 0.0, 0.0]);
    assert_eq!(lut.data[2], [0.0, 1.0, 0.0]);
    assert_eq!(lut.data[4], [0.0, 0.0, 1.0]);
    let mid = lut.sample([0.25, 0.5, 0.75]);
    for (a, b) in mid.iter().zip([0.25, 0.5, 0.75]) {
        assert!((a - b).abs() < 1e-5, "{mid:?}");
    }
}

#[test]
fn scales_a_domain_into_range() {
    let text = "LUT_3D_SIZE 2\nDOMAIN_MIN 0 0 0\nDOMAIN_MAX 2 2 2\n\
                0 0 0\n2 0 0\n0 2 0\n2 2 0\n0 0 2\n2 0 2\n0 2 2\n1 1 1\n";
    let lut = parse_cube(text).unwrap();
    assert_eq!(lut.data[1], [1.0, 0.0, 0.0]);
    assert_eq!(lut.data[7], [0.5, 0.5, 0.5]);
}

#[test]
fn refuses_what_isnt_a_3d_lut() {
    assert!(parse_cube("LUT_1D_SIZE 4\n0 0 0\n").is_err());
    assert!(parse_cube("0 0 0\n1 1 1\n").is_err(), "no size");
    assert!(
        parse_cube("LUT_3D_SIZE 2\n0 0 0\n1 1 1\n").is_err(),
        "too few entries"
    );
    assert!(
        parse_cube("LUT_3D_SIZE 2\n0 0 x\n").is_err(),
        "not a number"
    );
    assert!(parse_cube("LUT_3D_SIZE 1\n0 0 0\n").is_err());
}

#[test]
fn every_preset_is_a_33_point_lut() {
    for (id, _) in PRESETS {
        let lut = preset(id).unwrap_or_else(|| panic!("{id}"));
        assert_eq!(lut.size, 33, "{id}");
        assert_eq!(lut.data.len(), 33 * 33 * 33, "{id}");
        assert!(
            lut.data.iter().flatten().all(|v| (0.0..=1.0).contains(v)),
            "{id}"
        );
    }
    assert!(preset("nope").is_none());
}

#[test]
fn black_and_white_is_grey() {
    let lut = preset("mono").unwrap();
    let c = lut.sample([0.9, 0.2, 0.1]);
    assert!(
        (c[0] - c[1]).abs() < 1e-4 && (c[1] - c[2]).abs() < 1e-4,
        "{c:?}"
    );
}

#[test]
fn big_luts_are_resampled_for_the_gpu() {
    let big = Lut::from_fn(65, |c| c);
    let fitted = big.fitted();
    assert_eq!(fitted.size, PRESET_SIZE);
    let c = fitted.sample([0.3, 0.6, 0.9]);
    assert!(
        (c[0] - 0.3).abs() < 0.01 && (c[2] - 0.9).abs() < 0.01,
        "{c:?}"
    );
}

#[test]
fn the_texture_lays_blue_out_as_slices() {
    let lut = parse_cube(IDENTITY_2).unwrap();
    let atlas = Atlas::new(&lut);
    assert_eq!((atlas.width, atlas.height), (4, 2));
    let px = |x: usize, y: usize| {
        let at = (y * atlas.width + x) * 4;
        &atlas.pixels[at..at + 3]
    };
    // x = blue slice * size + red, y = green
    assert_eq!(px(1, 0), [255, 0, 0]);
    assert_eq!(px(0, 1), [0, 255, 0]);
    assert_eq!(px(2, 0), [0, 0, 255]);
    assert_eq!(px(3, 1), [255, 255, 255]);
}

/// The real thing: a picture through `rdlook` on whatever GL there is here
/// (Mesa's software renderer in CI and development)
#[test]
fn grades_a_picture_on_the_gpu() {
    use gstreamer as gst;
    use gstreamer::prelude::*;

    if gst::init().is_err() || gst::ElementFactory::find("glupload").is_none() {
        eprintln!("no GStreamer GL here; skipped");
        return;
    }
    register().unwrap();

    let run = |strength: u8| -> Option<[u8; 3]> {
        select(&LookSettings {
            enabled: true,
            lut: "mono".into(),
            strength,
            quality: Default::default(),
        });
        let pipeline = gst::parse::launch(
            "videotestsrc num-buffers=1 pattern=solid-color foreground-color=0xffe03010 \
             ! video/x-raw,format=RGBA,width=64,height=64 \
             ! glupload ! rdlook ! gldownload ! video/x-raw,format=RGBA \
             ! appsink name=sink sync=false",
        )
        .unwrap()
        .dynamic_cast::<gst::Pipeline>()
        .unwrap();
        let sink = pipeline
            .by_name("sink")
            .unwrap()
            .dynamic_cast::<gstreamer_app::AppSink>()
            .unwrap();
        pipeline.set_state(gst::State::Playing).ok()?;
        let sample = sink.try_pull_sample(gst::ClockTime::from_seconds(10));
        let _ = pipeline.set_state(gst::State::Null);
        let buffer = sample?.buffer_owned()?;
        let map = buffer.map_readable().ok()?;
        let at = (32 * 64 + 32) * 4;
        Some([map[at], map[at + 1], map[at + 2]])
    };

    let Some(plain) = run(0) else {
        eprintln!("GL wouldn't start here; skipped");
        return;
    };
    let graded = run(100).expect("graded a picture");
    // Strength 0 leaves the colour alone
    assert!((plain[0] as i32 - 0xe0).abs() <= 2, "{plain:?}");
    assert!((plain[2] as i32 - 0x10).abs() <= 2, "{plain:?}");
    // Black & white: all three channels equal, and not the input's red
    assert!(
        (graded[0] as i32 - graded[1] as i32).abs() <= 2
            && (graded[1] as i32 - graded[2] as i32).abs() <= 2,
        "{graded:?}"
    );
    assert!(graded[0] < 200, "{graded:?}");
}

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

fn untitled_lut() -> String {
    IDENTITY_2.replace("TITLE \"Identity\"\n", "")
}

fn test_library() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let dir = std::env::temp_dir().join(format!("rd-luts-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let _ = LIBRARY.set(dir.join("luts"));
    });
}

#[test]
fn names_files_safely() {
    assert_eq!(slug("Crisp Autumn.cube"), "crisp-autumn");
    assert_eq!(slug("../../etc/passwd"), "passwd");
    assert_eq!(
        slug("DJI Avata 2 D-Log M to Rec.709.cube"),
        "dji-avata-2-d-log-m-to-rec-709"
    );
    assert_eq!(slug("___"), "");
    assert!(file_path("file:../x").is_none());
    assert!(file_path("film").is_none());
}

#[test]
fn keeps_a_library_of_luts() {
    test_library();
    let saved = save("My Look.cube", IDENTITY_2.as_bytes()).unwrap();
    assert_eq!(saved.id, "file:my-look");
    assert_eq!(saved.name, "Identity", "named by its TITLE");
    assert!(save("bad.cube", b"nonsense").is_err());
    assert!(
        files()
            .iter()
            .any(|f| f.id == "file:my-look" && f.size == 2)
    );
    remove("file:my-look").unwrap();
    assert!(!files().iter().any(|f| f.id == "file:my-look"));
}

#[test]
fn unpacks_the_luts_in_a_zip() {
    test_library();
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let untitled = IDENTITY_2.replace("TITLE \"Identity\"\n", "");
    for (name, body) in [
        ("pack/Crispautumn.cube", untitled.as_str()),
        ("pack/Waves.cube", untitled.as_str()),
        ("pack/readme.txt", "hello"),
    ] {
        zip.start_file(name, opts).unwrap();
        std::io::Write::write_all(&mut zip, body.as_bytes()).unwrap();
    }
    let bytes = zip.finish().unwrap().into_inner();
    assert_eq!(import_zip(&bytes).unwrap(), 2);
    let names: Vec<String> = files().into_iter().map(|f| f.name).collect();
    assert!(names.contains(&"Crispautumn".to_string()), "{names:?}");
    let dji = save(
        "DJI Avata 2 D-Log M to Rec.709.cube",
        untitled_lut().as_bytes(),
    )
    .unwrap();
    assert_eq!(
        dji.name, "DJI Avata 2 D-Log M to Rec.709",
        "the file's name as it was"
    );
    assert!(
        files()
            .iter()
            .any(|f| f.name == "DJI Avata 2 D-Log M to Rec.709")
    );
    assert!(names.contains(&"Waves".to_string()), "{names:?}");
}

#[test]
fn hlg_to_709_keeps_grey_grey_and_rises() {
    let lut = preset("hlg_709").unwrap();
    let mut last = -1.0;
    for i in 0..=10 {
        let v = i as f32 / 10.0;
        let c = lut.sample([v, v, v]);
        assert!(
            (c[0] - c[1]).abs() < 0.01 && (c[1] - c[2]).abs() < 0.01,
            "{c:?}"
        );
        assert!(c[0] >= last, "brighter in, brighter out");
        last = c[0];
    }
    // HLG's reference white comes out near SDR white
    let white = lut.sample([0.75, 0.75, 0.75])[0];
    assert!((0.85..=0.97).contains(&white), "{white}");
}

#[test]
fn dewarp_geometry() {
    let aspect = 16.0 / 9.0;
    let off = Dewarp::from(&DewarpSettings::default());
    assert_eq!(off.source([0.2, 0.7], aspect), [0.2, 0.7], "switched off");

    let none = Dewarp::from(&DewarpSettings {
        enabled: true,
        fisheye: 0,
        edge: 0,
        zoom: 0,
        offset_x: 0,
        offset_y: 0,
    });
    let p = none.source([0.2, 0.7], aspect);
    assert!(
        (p[0] - 0.2).abs() < 1e-5 && (p[1] - 0.7).abs() < 1e-5,
        "{p:?}"
    );

    // Straightening a fisheye samples the corners from further in
    let fisheye = Dewarp::from(&DewarpSettings {
        enabled: true,
        fisheye: 50,
        zoom: 0,
        ..Default::default()
    });
    let corner = fisheye.source([1.0, 1.0], aspect);
    assert!(corner[0] < 1.0 && corner[1] < 1.0, "{corner:?}");
    let centre = fisheye.source([0.5, 0.5], aspect);
    assert!((centre[0] - 0.5).abs() < 1e-5);

    // 100 % zoom: the right edge shows what was three quarters across
    let zoom = Dewarp::from(&DewarpSettings {
        enabled: true,
        fisheye: 0,
        zoom: 100,
        ..Default::default()
    });
    assert!((zoom.source([1.0, 0.5], aspect)[0] - 0.75).abs() < 1e-5);

    let moved = Dewarp::from(&DewarpSettings {
        enabled: true,
        fisheye: 0,
        zoom: 0,
        offset_x: 10,
        ..Default::default()
    });
    assert!((moved.source([0.5, 0.5], aspect)[0] - 0.6).abs() < 1e-5);
}

/// A picture through `rdlook` on whatever GL there is here (Mesa's software
/// renderer in development), or nothing when there's none
fn through_gpu(input: &[u8], size: (usize, usize)) -> Option<Vec<u8>> {
    use gstreamer as gst;
    use gstreamer::prelude::*;

    let pipeline = gst::parse::launch(
        "appsrc name=src format=time ! glupload ! rdlook ! gldownload \
         ! video/x-raw,format=RGBA ! appsink name=sink sync=false",
    )
    .ok()?
    .dynamic_cast::<gst::Pipeline>()
    .ok()?;
    let src = pipeline
        .by_name("src")?
        .dynamic_cast::<gstreamer_app::AppSrc>()
        .ok()?;
    src.set_caps(Some(
        &gst::Caps::builder("video/x-raw")
            .field("format", "RGBA")
            .field("width", size.0 as i32)
            .field("height", size.1 as i32)
            .field("framerate", gst::Fraction::new(30, 1))
            .build(),
    ));
    let sink = pipeline
        .by_name("sink")?
        .dynamic_cast::<gstreamer_app::AppSink>()
        .ok()?;
    pipeline.set_state(gst::State::Playing).ok()?;
    let _ = src.push_buffer(gst::Buffer::from_slice(input.to_vec()));
    let _ = src.end_of_stream();
    let sample = sink.try_pull_sample(gst::ClockTime::from_seconds(10));
    let _ = pipeline.set_state(gst::State::Null);
    let buffer = sample?.buffer_owned()?;
    let map = buffer.map_readable().ok()?;
    Some(map.to_vec())
}

/// The real thing, on the GPU: grading, and dewarp as `Dewarp::source` says
#[test]
fn grades_and_dewarps_on_the_gpu() {
    use gstreamer as gst;

    if gst::init().is_err() || gst::ElementFactory::find("glupload").is_none() {
        eprintln!("no GStreamer GL here; skipped");
        return;
    }
    register().unwrap();
    let (w, h) = (64usize, 36usize);
    let solid: Vec<u8> = (0..w * h).flat_map(|_| [0xe0, 0x30, 0x10, 0xff]).collect();
    let px = |out: &[u8], x: usize, y: usize| {
        let at = (y * w + x) * 4;
        [out[at], out[at + 1], out[at + 2]]
    };
    let look = |lut: &str, strength| LookSettings {
        enabled: true,
        lut: lut.into(),
        strength,
        quality: Default::default(),
    };
    let flat = DewarpSettings::default();

    select(&look("mono", 0), &flat);
    let Some(plain) = through_gpu(&solid, (w, h)) else {
        eprintln!("GL wouldn't start here; skipped");
        return;
    };
    let c = px(&plain, 32, 18);
    assert!(
        (c[0] as i32 - 0xe0).abs() <= 2 && (c[2] as i32 - 0x10).abs() <= 2,
        "{c:?}"
    );

    select(&look("mono", 100), &flat);
    let grey = through_gpu(&solid, (w, h)).expect("graded");
    let c = px(&grey, 32, 18);
    assert!((c[0] as i32 - c[1] as i32).abs() <= 2 && (c[1] as i32 - c[2] as i32).abs() <= 2);
    assert!(c[0] < 200, "{c:?}");

    // A ramp: red is how far across the picture a pixel is
    let ramp: Vec<u8> = (0..w * h)
        .flat_map(|i| [((i % w) * 255 / (w - 1)) as u8, 0, 0, 0xff])
        .collect();
    let dewarp = DewarpSettings {
        enabled: true,
        fisheye: 60,
        edge: 0,
        zoom: 30,
        offset_x: 5,
        offset_y: 0,
    };
    select(&look("mono", 0), &dewarp);
    let out = through_gpu(&ramp, (w, h)).expect("dewarped");
    let d = Dewarp::from(&dewarp);
    for x in [4usize, 16, 32, 48, 60] {
        let uv = [(x as f32 + 0.5) / w as f32, (18.0 + 0.5) / h as f32];
        let want = d.source(uv, w as f32 / h as f32)[0];
        // The ramp's value at that point
        let want = ((want * w as f32 - 0.5) / (w - 1) as f32 * 255.0).clamp(0.0, 255.0);
        let got = px(&out, x, 18)[0] as f32;
        assert!(
            (got - want).abs() <= 8.0,
            "x={x}: got {got}, expected {want}"
        );
    }
}

//! `rdlook`: a GL filter that straightens the picture (dewarp) and grades
//! every pixel through the LUT in `look::CURRENT`. GStreamer's own glshader
//! takes just the one texture, so this binds the LUT as a second one beside
//! the picture.

use std::ffi::c_void;
use std::sync::{LazyLock, Mutex};

use gstreamer as gst;
use gstreamer::glib;
use gstreamer::prelude::*;
use gstreamer::subclass::prelude::*;
use gstreamer_base::subclass::prelude::*;
use gstreamer_gl as gst_gl;
use gstreamer_gl::prelude::*;
use gstreamer_gl::subclass::prelude::*;

const TEXTURE_2D: u32 = 0x0DE1;
const TEXTURE0: u32 = 0x84C0;
/// GstGLFilter draws with the picture on unit 1, so the LUT goes on 2
const LUT_UNIT: u32 = 0x84C2;
const RGBA: u32 = 0x1908;
const UNSIGNED_BYTE: u32 = 0x1401;
const TEXTURE_MAG_FILTER: u32 = 0x2800;
const TEXTURE_MIN_FILTER: u32 = 0x2801;
const TEXTURE_WRAP_S: u32 = 0x2802;
const TEXTURE_WRAP_T: u32 = 0x2803;
const LINEAR: i32 = 0x2601;
const CLAMP_TO_EDGE: i32 = 0x812F;
const UNPACK_ALIGNMENT: u32 = 0x0CF5;
const FRAGMENT_SHADER: u32 = 0x8B30;

/// The LUT is N slices of N × N side by side: blue picks two neighbouring
/// slices, red and green are filtered by the texture unit inside each
pub const FRAGMENT: &str = r#"
#ifdef GL_ES
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
#endif
varying vec2 v_texcoord;
uniform sampler2D tex;
uniform sampler2D lut;
uniform float size;
uniform float strength;
uniform float dewarp;
uniform float k1;
uniform float k2;
uniform float scale;
uniform vec2 offset;
uniform float aspect;
// 1 when the picture comes in video levels (black at 16, white at 235)
uniform float levels;
// 1 when the picture goes on to the image processor to be encoded, which
// takes RGB in video levels too
uniform float vout;
// Stabilisation: sample at R(sangle)·p/szoom + soffset (look::stab)
uniform float stab;
uniform vec2 soffset;
uniform float sangle;
uniform float szoom;

vec2 steady(vec2 uv) {
    vec2 p = vec2((uv.x * 2.0 - 1.0) * aspect, uv.y * 2.0 - 1.0) / szoom;
    float c = cos(sangle);
    float s = sin(sangle);
    p = vec2(p.x * c - p.y * s, p.x * s + p.y * c) + soffset;
    return vec2(p.x / aspect, p.y) * 0.5 + 0.5;
}

// Where this output point samples the picture: look::Dewarp::source
vec2 warp(vec2 uv) {
    vec2 p = vec2((uv.x * 2.0 - 1.0) * aspect, uv.y * 2.0 - 1.0);
    p = p / scale + vec2(offset.x * 2.0 * aspect, offset.y * 2.0);
    float r2 = dot(p, p) / (1.0 + aspect * aspect);
    p *= 1.0 + k1 * r2 + k2 * r2 * r2;
    return vec2(p.x / aspect, p.y) * 0.5 + 0.5;
}

vec3 lookup(vec3 c) {
    float n = size - 1.0;
    float b = c.b * n;
    float b0 = floor(b);
    float b1 = min(b0 + 1.0, n);
    float x = c.r * n + 0.5;
    float y = (c.g * n + 0.5) / size;
    float w = size * size;
    vec3 s0 = texture2D(lut, vec2((b0 * size + x) / w, y)).rgb;
    vec3 s1 = texture2D(lut, vec2((b1 * size + x) / w, y)).rgb;
    return mix(s0, s1, b - b0);
}

void main() {
    // Straightened first, then steadied where the motion was measured: in
    // the picture as the lens made it. The other way round, the lens bent
    // the correction and the edges wobbled like jelly
    vec2 uv = dewarp > 0.5 ? warp(v_texcoord) : v_texcoord;
    uv = stab > 0.5 ? steady(uv) : uv;
    // What the correction pulls in from beyond the picture is black
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
        gl_FragColor = vec4(0.0, 0.0, 0.0, 1.0);
        return;
    }
    vec4 c = texture2D(tex, uv);
    if (levels > 0.5) {
        c.rgb = clamp((c.rgb - 16.0 / 255.0) * (255.0 / 219.0), 0.0, 1.0);
    }
    vec3 graded = lookup(clamp(c.rgb, 0.0, 1.0));
    vec3 o = mix(c.rgb, graded, strength);
    if (vout > 0.5) {
        o = 16.0 / 255.0 + o * (219.0 / 255.0);
    }
    gl_FragColor = vec4(o, 1.0);
}
"#;

type ActiveTexture = extern "C" fn(u32);
type BindTexture = extern "C" fn(u32, u32);
type GenTextures = extern "C" fn(i32, *mut u32);
type DeleteTextures = extern "C" fn(i32, *const u32);
type TexParameteri = extern "C" fn(u32, u32, i32);
type PixelStorei = extern "C" fn(u32, i32);
type TexImage2d = extern "C" fn(u32, i32, i32, i32, i32, i32, u32, u32, *const c_void);
type Finish = extern "C" fn();

/// The few GL calls GStreamer has no wrapper for
struct Gl {
    active_texture: ActiveTexture,
    bind_texture: BindTexture,
    gen_textures: GenTextures,
    delete_textures: DeleteTextures,
    tex_parameteri: TexParameteri,
    pixel_storei: PixelStorei,
    tex_image_2d: TexImage2d,
    finish: Finish,
}

impl Gl {
    fn load(context: &gst_gl::GLContext) -> Option<Self> {
        macro_rules! get {
            ($name:literal, $ty:ty) => {{
                let p = context.proc_address($name);
                if p == 0 {
                    return None;
                }
                // SAFETY: the address GL gave for this very function,
                // called with the signature the GL ES 2 spec gives it
                unsafe { std::mem::transmute::<usize, $ty>(p) }
            }};
        }
        Some(Self {
            active_texture: get!("glActiveTexture", ActiveTexture),
            bind_texture: get!("glBindTexture", BindTexture),
            gen_textures: get!("glGenTextures", GenTextures),
            delete_textures: get!("glDeleteTextures", DeleteTextures),
            tex_parameteri: get!("glTexParameteri", TexParameteri),
            pixel_storei: get!("glPixelStorei", PixelStorei),
            tex_image_2d: get!("glTexImage2D", TexImage2d),
            finish: get!("glFinish", Finish),
        })
    }
}

#[derive(Default)]
struct GlState {
    gl: Option<Gl>,
    shader: Option<gst_gl::GLShader>,
    texture: u32,
    version: u64,
    size: f32,
    /// The picture being drawn's timestamp
    pts: Option<u64>,
}

// SAFETY: only ever touched on GStreamer's GL thread, behind the mutex
unsafe impl Send for GlState {}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct RdLook {
        state: Mutex<GlState>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RdLook {
        const NAME: &'static str = "RdLook";
        type Type = super::RdLook;
        type ParentType = gst_gl::GLFilter;
    }

    impl ObjectImpl for RdLook {}
    impl GstObjectImpl for RdLook {}

    impl ElementImpl for RdLook {
        fn metadata() -> Option<&'static gst::subclass::ElementMetadata> {
            static META: LazyLock<gst::subclass::ElementMetadata> = LazyLock::new(|| {
                gst::subclass::ElementMetadata::new(
                    "RDstreamer look",
                    "Filter/Effect/Video",
                    "Grades video through a 3D LUT on the GPU",
                    "RDstreamer",
                )
            });
            Some(&META)
        }
    }

    impl BaseTransformImpl for RdLook {
        const MODE: gstreamer_base::subclass::BaseTransformMode =
            gstreamer_base::subclass::BaseTransformMode::NeverInPlace;
        const PASSTHROUGH_ON_SAME_CAPS: bool = false;
        const TRANSFORM_IP_ON_PASSTHROUGH: bool = false;

        // Which picture is coming: its stabilisation is looked up by time
        fn before_transform(&self, inbuf: &gst::BufferRef) {
            self.state.lock().unwrap().pts = inbuf.pts().map(|t| t.nseconds());
            self.parent_before_transform(inbuf);
        }
    }

    impl GLBaseFilterImpl for RdLook {
        fn gl_start(&self) -> Result<(), gst::LoggableError> {
            let context = GLBaseFilterExt::context(&*self.obj())
                .ok_or_else(|| gst::loggable_error!(gst::CAT_RUST, "no GL context"))?;
            let shader = gst_gl::GLShader::new(&context);
            let vertex = gst_gl::GLSLStage::new_default_vertex(&context);
            vertex
                .compile()
                .map_err(|e| gst::loggable_error!(gst::CAT_RUST, "vertex shader: {e}"))?;
            shader
                .attach_unlocked(&vertex)
                .map_err(|e| gst::loggable_error!(gst::CAT_RUST, "vertex shader: {e}"))?;
            let fragment = gst_gl::GLSLStage::with_strings(
                &context,
                FRAGMENT_SHADER,
                gst_gl::GLSLVersion::None,
                gst_gl::GLSLProfile::ES | gst_gl::GLSLProfile::COMPATIBILITY,
                &[FRAGMENT],
            );
            fragment
                .compile()
                .map_err(|e| gst::loggable_error!(gst::CAT_RUST, "LUT shader: {e}"))?;
            shader
                .attach_unlocked(&fragment)
                .map_err(|e| gst::loggable_error!(gst::CAT_RUST, "LUT shader: {e}"))?;
            shader
                .link()
                .map_err(|e| gst::loggable_error!(gst::CAT_RUST, "LUT shader: {e}"))?;
            let gl = Gl::load(&context)
                .ok_or_else(|| gst::loggable_error!(gst::CAT_RUST, "GL functions missing"))?;

            let mut st = self.state.lock().unwrap();
            *st = GlState {
                gl: Some(gl),
                shader: Some(shader),
                ..Default::default()
            };
            drop(st);
            self.parent_gl_start()
        }

        fn gl_stop(&self) {
            let mut st = self.state.lock().unwrap();
            if let Some(gl) = &st.gl
                && st.texture != 0
            {
                (gl.delete_textures)(1, &st.texture);
            }
            *st = GlState::default();
            drop(st);
            self.parent_gl_stop()
        }
    }

    impl GLFilterImpl for RdLook {
        const MODE: gst_gl::subclass::GLFilterMode = gst_gl::subclass::GLFilterMode::Texture;

        fn filter_texture(
            &self,
            input: &gst_gl::GLMemory,
            output: &gst_gl::GLMemory,
        ) -> Result<(), gst::LoggableError> {
            let (atlas, version, strength, dewarp, stabilise) = {
                let current = crate::output::look::CURRENT.lock().unwrap();
                (
                    current.atlas.clone(),
                    current.version,
                    current.strength,
                    current.dewarp,
                    current.stabilise,
                )
            };
            let mut st = self.state.lock().unwrap();
            let st = &mut *st;
            let (Some(gl), Some(shader)) = (&st.gl, &st.shader) else {
                return Err(gst::loggable_error!(gst::CAT_RUST, "not started"));
            };

            if st.version != version {
                if let Some(atlas) = &atlas {
                    if st.texture == 0 {
                        (gl.gen_textures)(1, &mut st.texture);
                    }
                    (gl.active_texture)(LUT_UNIT);
                    (gl.bind_texture)(TEXTURE_2D, st.texture);
                    (gl.pixel_storei)(UNPACK_ALIGNMENT, 1);
                    for (param, value) in [
                        (TEXTURE_MIN_FILTER, LINEAR),
                        (TEXTURE_MAG_FILTER, LINEAR),
                        (TEXTURE_WRAP_S, CLAMP_TO_EDGE),
                        (TEXTURE_WRAP_T, CLAMP_TO_EDGE),
                    ] {
                        (gl.tex_parameteri)(TEXTURE_2D, param, value);
                    }
                    (gl.tex_image_2d)(
                        TEXTURE_2D,
                        0,
                        RGBA as i32,
                        atlas.width as i32,
                        atlas.height as i32,
                        0,
                        RGBA,
                        UNSIGNED_BYTE,
                        atlas.pixels.as_ptr().cast(),
                    );
                    (gl.active_texture)(TEXTURE0);
                    st.size = atlas.size as f32;
                }
                st.version = version;
            }

            // Without a LUT (a custom one that's gone, say) the picture
            // goes through as it is
            // The splash screen goes through untouched
            let splash = crate::output::look::SPLASH.load(std::sync::atomic::Ordering::Relaxed);
            let dewarp = if splash {
                crate::output::look::Dewarp::OFF
            } else {
                dewarp
            };
            let graded = atlas.is_some() && st.texture != 0 && !splash;
            (gl.active_texture)(LUT_UNIT);
            (gl.bind_texture)(TEXTURE_2D, if graded { st.texture } else { 0 });
            (gl.active_texture)(TEXTURE0);
            shader.use_();
            shader.set_uniform_1i("lut", 2);
            shader.set_uniform_1f("size", st.size.max(2.0));
            shader.set_uniform_1f("strength", if graded { strength } else { 0.0 });
            let aspect = output.texture_width() as f32 / output.texture_height().max(1) as f32;
            shader.set_uniform_1f("dewarp", if dewarp.on { 1.0 } else { 0.0 });
            shader.set_uniform_1f("k1", dewarp.k1);
            shader.set_uniform_1f("k2", dewarp.k2);
            shader.set_uniform_1f("scale", dewarp.scale);
            shader.set_uniform_2f("offset", dewarp.offset[0], dewarp.offset[1]);
            shader.set_uniform_1f("aspect", aspect);
            let levels =
                crate::output::look::VIDEO_LEVELS.load(std::sync::atomic::Ordering::Relaxed);
            shader.set_uniform_1f("levels", if levels { 1.0 } else { 0.0 });
            // Named so in the pipelines that hand it to the image processor
            let vout = self.obj().name().starts_with(crate::output::look::TO_ISP);
            shader.set_uniform_1f("vout", if vout { 1.0 } else { 0.0 });
            let steady = if stabilise && !splash {
                crate::output::look::stab::correction_for(st.pts)
            } else {
                crate::output::look::stab::Correction::NONE
            };
            shader.set_uniform_1f("stab", if stabilise && !splash { 1.0 } else { 0.0 });
            shader.set_uniform_2f("soffset", steady.offset[0], steady.offset[1]);
            shader.set_uniform_1f("sangle", steady.angle);
            shader.set_uniform_1f("szoom", steady.zoom);
            self.obj()
                .render_to_target_with_shader(input, output, shader);
            // The picture goes to the screen as a dmabuf with nothing to say
            // when the GPU is done with it: shown early, the screen got half
            // of this frame over the last one in that buffer, and moving
            // things doubled. Done drawing before it's handed on
            (gl.finish)();
            Ok(())
        }
    }
}

glib::wrapper! {
    pub struct RdLook(ObjectSubclass<imp::RdLook>)
        @extends gst_gl::GLFilter, gst_gl::GLBaseFilter, gstreamer_base::BaseTransform,
            gst::Element, gst::Object;
}

/// Makes `rdlook` available to pipelines; once is enough
pub fn register() -> Result<(), String> {
    static DONE: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
    DONE.get_or_init(|| {
        gst::init().map_err(|e| e.to_string())?;
        gst::Element::register(None, "rdlook", gst::Rank::NONE, RdLook::static_type())
            .map_err(|e| e.to_string())
    })
    .clone()
}

// The HDMI look, drawn in the browser: the same dewarp and LUT the Pi's GPU
// applies (device/backend/src/output/look/gl.rs and look.rs Dewarp), so the
// preview in Display → Image shows what the screen will. Keep the two in step.

import type { Settings } from "@/types/settings";

type Display = Settings["display"];

const VERTEX = `
attribute vec2 a_position;
varying vec2 v_texcoord;
void main() {
  // y = 0 at the top of the picture, as in GStreamer's GL
  v_texcoord = vec2(a_position.x * 0.5 + 0.5, 0.5 - a_position.y * 0.5);
  gl_Position = vec4(a_position, 0.0, 1.0);
}`;

const FRAGMENT = `
precision highp float;
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
// Left of this (0-1 across) the picture is shown as it comes, to compare
uniform float split;

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
  if (v_texcoord.x < split) {
    gl_FragColor = vec4(texture2D(tex, v_texcoord).rgb, 1.0);
    return;
  }
  vec2 uv = dewarp > 0.5 ? warp(v_texcoord) : v_texcoord;
  if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
    gl_FragColor = vec4(0.0, 0.0, 0.0, 1.0);
    return;
  }
  vec4 c = texture2D(tex, uv);
  vec3 graded = lookup(clamp(c.rgb, 0.0, 1.0));
  gl_FragColor = vec4(mix(c.rgb, graded, strength), 1.0);
}`;

export interface Lut {
  size: number;
  pixels: Uint8Array;
}

/** The LUT texture the Pi grades with, fetched from it */
export async function fetchLut(id: string): Promise<Lut> {
  const res = await fetch(
    `/api/display/look/atlas?id=${encodeURIComponent(id)}`,
  );
  if (!res.ok) throw new Error(`LUT ${id}: ${res.status}`);
  const size = Number(res.headers.get("x-lut-size"));
  return { size, pixels: new Uint8Array(await res.arrayBuffer()) };
}

/** The shader's dewarp numbers from the settings, as look.rs works them out */
export function dewarpUniforms(d: Display["dewarp"]) {
  return {
    on: d.enabled,
    k1: (-d.fisheye / 100) * 0.5,
    k2: (-d.edge / 100) * 0.3,
    scale: Math.max(0.5, 1 + d.zoom / 100),
    offset: [d.offset_x / 100, d.offset_y / 100] as [number, number],
  };
}

export class Grader {
  #gl: WebGLRenderingContext;
  #program: WebGLProgram;
  #video: WebGLTexture;
  #lut: WebGLTexture;
  #lutSize = 2;
  #hasLut = false;

  constructor(canvas: HTMLCanvasElement) {
    const gl = canvas.getContext("webgl", {
      premultipliedAlpha: false,
      antialias: false,
    });
    if (!gl) throw new Error("This browser has no WebGL");
    this.#gl = gl;

    const compile = (type: number, src: string) => {
      const s = gl.createShader(type)!;
      gl.shaderSource(s, src);
      gl.compileShader(s);
      if (!gl.getShaderParameter(s, gl.COMPILE_STATUS))
        throw new Error(gl.getShaderInfoLog(s) ?? "shader");
      return s;
    };
    const program = gl.createProgram()!;
    gl.attachShader(program, compile(gl.VERTEX_SHADER, VERTEX));
    gl.attachShader(program, compile(gl.FRAGMENT_SHADER, FRAGMENT));
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS))
      throw new Error(gl.getProgramInfoLog(program) ?? "program");
    this.#program = program;
    gl.useProgram(program);

    const quad = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, quad);
    gl.bufferData(
      gl.ARRAY_BUFFER,
      new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]),
      gl.STATIC_DRAW,
    );
    const at = gl.getAttribLocation(program, "a_position");
    gl.enableVertexAttribArray(at);
    gl.vertexAttribPointer(at, 2, gl.FLOAT, false, 0, 0);

    const texture = () => {
      const t = gl.createTexture()!;
      gl.bindTexture(gl.TEXTURE_2D, t);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      return t;
    };
    this.#video = texture();
    this.#lut = texture();
    gl.uniform1i(gl.getUniformLocation(program, "tex"), 0);
    gl.uniform1i(gl.getUniformLocation(program, "lut"), 1);
  }

  setLut(lut: Lut | null) {
    const gl = this.#gl;
    this.#hasLut = !!lut;
    if (!lut) return;
    gl.activeTexture(gl.TEXTURE1);
    gl.bindTexture(gl.TEXTURE_2D, this.#lut);
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
    gl.texImage2D(
      gl.TEXTURE_2D,
      0,
      gl.RGBA,
      lut.size * lut.size,
      lut.size,
      0,
      gl.RGBA,
      gl.UNSIGNED_BYTE,
      lut.pixels,
    );
    gl.activeTexture(gl.TEXTURE0);
    this.#lutSize = lut.size;
  }

  /** One frame of `video` through the look; false while it has no picture */
  draw(video: HTMLVideoElement, display: Display, split: number): boolean {
    if (video.readyState < 2 || !video.videoWidth) return false;
    const gl = this.#gl;
    const canvas = gl.canvas as HTMLCanvasElement;
    if (
      canvas.width !== video.videoWidth ||
      canvas.height !== video.videoHeight
    ) {
      canvas.width = video.videoWidth;
      canvas.height = video.videoHeight;
    }
    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.#video);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, video);

    const p = this.#program;
    const u = (name: string) => gl.getUniformLocation(p, name);
    const d = dewarpUniforms(display.dewarp);
    const graded = display.look.enabled && this.#hasLut;
    gl.uniform1f(u("size"), this.#lutSize);
    gl.uniform1f(u("strength"), graded ? display.look.strength / 100 : 0);
    gl.uniform1f(u("dewarp"), d.on ? 1 : 0);
    gl.uniform1f(u("k1"), d.k1);
    gl.uniform1f(u("k2"), d.k2);
    gl.uniform1f(u("scale"), d.scale);
    gl.uniform2f(u("offset"), d.offset[0], d.offset[1]);
    gl.uniform1f(u("aspect"), canvas.width / canvas.height);
    gl.uniform1f(u("split"), split);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
    return true;
  }
}

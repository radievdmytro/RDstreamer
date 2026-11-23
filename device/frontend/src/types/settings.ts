export interface Settings {
  input: {
    mode: import("@/types/input").InputMode;
    uvc: {
      /** /dev/v4l/by-id path, empty for the first one found */
      device: string;
      /** `WxH@fps`, empty for the best available */
      mode: string;
      bitrate_kbps: number;
      keyframe_secs: number;
    };
    rtmp: {
      port: number;
      /** Made up when RTMP is first picked */
      key: string;
      /** IP addresses allowed to stream, comma separated; empty for any */
      allowed_from: string;
    };
    /** Pass the goggles' link through to DJI Fly on a phone in USB-A */
    passthrough: boolean;
    /** Swaps between `first` and `second`; so does POST /api/input/toggle */
    button: {
      enabled: boolean;
      /** Switch: open is `first`, closed `second` */
      kind: "push" | "switch";
      /** GPIO number, not the position on the header */
      pin: number;
      first: import("@/types/input").InputMode;
      second: import("@/types/input").InputMode;
    };
  };
  device: { hostname: string; timezone: string };
  /** A second WiFi adapter (USB) joining a network for the internet */
  wifi2: {
    enabled: boolean;
    ssid: string;
    password: string;
    allow_5ghz: boolean;
  };
  wifi: {
    mode: WifiMode;
    client: { ssid: string; password: string };
    ap: {
      ssid: string;
      password: string;
      channel: number;
      address: string;
      /** Phones that join open the viewer by themselves */
      open_viewer: boolean;
    };
    fallback_ap: boolean;
    /** Two-letter code; empty leaves the radio's rules to the system */
    country: string;
  };
  ethernet: {
    mode: "system" | "auto" | "static";
    address: string;
    gateway: string;
    dns: string;
  };
  rtsp: {
    enabled: boolean;
    port: number;
    path: string;
    tcp_only: boolean;
    auth: boolean;
    username: string;
    password: string;
    /** With the HDMI look (LUT, dewarp, stabilisation) rather than the goggles' own picture */
    processed: boolean;
  };
  srt: {
    enabled: boolean;
    mode: SrtMode;
    host: string;
    port: number;
    latency_ms: number;
    passphrase: string;
    stream_id: string;
    /** With the HDMI look (LUT, dewarp, stabilisation) rather than the goggles' own picture */
    processed: boolean;
  };
  rtmp: {
    enabled: boolean;
    url: string;
    key: string;
    silent_audio: boolean;
    /** With the HDMI look (LUT, dewarp, stabilisation) rather than the goggles' own picture */
    processed: boolean;
  };
  udp: {
    enabled: boolean;
    format: "rtp" | "mpegts";
    /** host:port list, comma separated */
    destinations: string;
    /** With the HDMI look (LUT, dewarp, stabilisation) rather than the goggles' own picture */
    processed: boolean;
  };
  ndi: NetworkSource;
  omt: NetworkSource;
  recording: {
    path: string;
    format: RecordingFormat;
    prefix: string;
    split_minutes: number;
    auto_start: boolean;
    reserve_mb: number;
    /** GPIO numbers, not positions on the header */
    button: {
      enabled: boolean;
      kind: "push" | "switch";
      pin: number;
      led_pin: number | null;
    };
  };
  viewer: {
    transport: ViewerTransport;
    max_viewers: number;
    /** Original, a lighter 720p copy, or the light one once a viewer lags */
    quality: ViewerQuality;
    /** With the HDMI look (LUT, dewarp, stabilisation) rather than the goggles' own picture */
    processed: boolean;
  };
  display: {
    hdmi_output: boolean;
    /** `WxH@Hz` from the screen's list, or "auto" */
    mode: string;
    scaling: "fit" | "fill";
    rotate_180: boolean;
    mirror: boolean;
    /** A 3D LUT on the HDMI picture, graded on the GPU */
    look: {
      enabled: boolean;
      /** A preset's id, or "file:" and a library LUT's name */
      lut: string;
      /** 0-100 % */
      strength: number;
      /** For the look and dewarp both. speed: 720p, 60 fps; quality: full size, about 48 fps */
      quality: "speed" | "quality";
    };
    /** Lens correction on the HDMI picture */
    dewarp: {
      enabled: boolean;
      /** -100 to 100 % */
      fisheye: number;
      /** -100 to 100 % */
      edge: number;
      /** -50 to 100 % */
      zoom: number;
      /** -50 to 50 % */
      offset_x: number;
      offset_y: number;
    };
    /** Shake taken out of the picture, from the picture itself */
    stabilization: {
      enabled: boolean;
      /** 0-100: how much of the shake is taken out */
      shake: number;
      /** 0-100: how much of the horizon's wobble is levelled; 0 leaves it */
      horizon: number;
      /** 2-30 %: zoomed in this much, the room the picture has to move */
      crop: number;
      /** 0, 250, 500 or 1000 ms: shown later, smoothed with what comes next */
      delay_ms: number;
    };
  };
  splash: {
    freeze_last_frame: boolean;
    grayscale_freeze: boolean;
    status_over_image: boolean;
    image_scaling: "fit" | "fill";
    show_ap_password: boolean;
    /** Play the uploaded background video behind the status */
    background_video: boolean;
  };
  overlay: {
    enabled: boolean;
    /** Where the logo sits between the left edge (0) and the right (100) */
    x_percent: number;
    /** Same, from the top edge to the bottom */
    y_percent: number;
    /** Logo width, percent of the picture's width */
    size_percent: number;
    opacity_percent: number;
    burn_in: boolean;
  };
  advanced: {
    verbose_logs: boolean;
    terminal: boolean;
    check_for_updates: boolean;
    /** Install a downloaded update by itself once nothing is going on */
    auto_install_updates: boolean;
  };
}

export type WifiMode = "unmanaged" | "client" | "ap";

export type RecordingFormat = "mp4" | "ts";

export interface OverlayStatus {
  /** Pixel size of the uploaded logo */
  image: [number, number] | null;
  version: number;
}

export interface OverlayView {
  visible: boolean;
  version: number;
  x_percent: number;
  y_percent: number;
  size_percent: number;
  opacity_percent: number;
}

export type SrtMode = "listener" | "caller";

/** NDI or OMT: a source video software finds on the network by itself */
export interface NetworkSource {
  enabled: boolean;
  /** Shown as "HOSTNAME (name)" */
  name: string;
  /** Larger pictures are scaled down to this height */
  max_height: number;
  max_fps: number;
}

export interface NdiStatus {
  /** The NDI runtime is on the device */
  installed: boolean;
  active: boolean;
  receivers: number;
  width: number | null;
  height: number | null;
  error: string | null;
}

/** NDI's runtime, fetched and waiting for the owner to accept NDI's licence */
export interface NdiRuntimeStaged {
  version: string;
  /** The licence text from inside NDI's own package */
  license: string;
}

/** A download from NDI underway */
export interface NdiRuntimeDownload {
  received: number;
  /** The download's size, when NDI's server said it */
  total: number | null;
  /** Downloaded, and being checked */
  checking: boolean;
}

export interface NdiRuntime {
  staged: NdiRuntimeStaged | null;
  download: NdiRuntimeDownload | null;
}

export interface OmtStatus {
  active: boolean;
  port: number | null;
  /** Announced over mDNS, so receivers list it by themselves */
  announced: boolean;
  receivers: number;
  width: number | null;
  height: number | null;
  error: string | null;
}

export interface UdpStatus {
  active: boolean;
  destinations: number;
  error: string | null;
}

export interface RtmpStatus {
  state: "off" | "connecting" | "live" | "retrying";
  error: string | null;
  uptime_secs: number;
  bitrate_kbps: number;
}

export interface DisplayMode {
  id: string;
  width: number;
  height: number;
  refresh: number;
  preferred: boolean;
  fits_screen: boolean;
}

export interface LutFile {
  /** "file:" and its name */
  id: string;
  name: string;
  size: number;
}

export interface LookStatus {
  presets: { id: string; label: string }[];
  /** The LUT library */
  files: LutFile[];
  /** GPU grading couldn't start on this device, and why */
  error: string | null;
}

export interface DisplayStatus {
  modes: DisplayMode[];
  current: DisplayMode | null;
  error: string | null;
}

export interface SrtStatus {
  active: boolean;
  clients: number;
  error: string | null;
}

export type ViewerTransport = "websocket" | "webrtc";
export type ViewerQuality = "original" | "light" | "auto";

export type Section = keyof Settings;

export interface SplashStatus {
  custom_image: boolean;
  video: SplashVideo;
}

export type SplashVideo =
  | { state: "none" }
  | { state: "converting"; seconds: number }
  | {
      state: "ready";
      width: number;
      height: number;
      fps: number;
      converted: boolean;
    }
  | { state: "failed"; error: string };

export interface Saved {
  settings: Settings;
  notice: string | null;
}

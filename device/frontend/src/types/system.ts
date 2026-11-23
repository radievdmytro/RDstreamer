export interface SystemInfo {
  version: string;
  hostname: string;
  model: string | null;
  serial: string | null;
  os: string | null;
  kernel: string | null;
  uptime_secs: number | null;
  load: [number, number, number] | null;
  cpu_temp_c: number | null;
  mem_total_kb: number | null;
  mem_available_kb: number | null;
  disk_total_bytes: number | null;
  disk_free_bytes: number | null;
  throttling: {
    raw: string;
    under_voltage_now: boolean;
    throttled_now: boolean;
    under_voltage_seen: boolean;
    throttled_seen: boolean;
  } | null;
  settings_path: string;
  systemd: boolean;
  unit: string;
}

export interface SshStatus {
  available: boolean;
  enabled: boolean;
  user: string | null;
  default_password: boolean;
}

export interface UpdateStatus {
  current: string;
  /** Uploaded and verified, waiting to be installed */
  staged: string | null;
  /** What rolling back would install */
  previous: string | null;
  /** A newer release, once a check has found one */
  available: Release | null;
  installing: boolean;
  /** While a package downloads: how far it got */
  downloading: {
    version: string;
    bytes: number;
    /** Known for releases from 1.7 on */
    total: number | null;
    seconds: number;
  } | null;
  /** While installing: the installer's last line */
  step: string | null;
  failure: {
    version: string;
    /** It installed but never answered, and the version before is back on */
    reverted: boolean;
    log: string;
  } | null;
}

export interface Release {
  version: string;
  notes: string;
  /** Newer than what's running */
  newer: boolean;
}

export type SystemAction = "restart" | "restart-usb" | "reboot" | "poweroff";

/** From this temperature up, this fan speed in percent */
export interface FanPoint {
  temp_c: number;
  speed: number;
}

export type FanMode = "auto" | "on" | "off";

/** Who runs the Argon ONE case's fan now */
export type FanControl = "driving" | "argononed" | "off" | "hat" | "no_case";

export interface FanConfig {
  mode: FanMode;
  curve: FanPoint[];
  /** Double tap reboots, a 3 s hold powers off */
  power_button: boolean;
  /** The case switches the power off after the Pi powers off */
  power_cut: boolean;
  /** Full speed for a second before a slow speed from standstill */
  spin_up: boolean;
}

export interface FanStatus {
  control: FanControl;
  config: FanConfig;
  temp_c: number | null;
  /** What was last sent to the fan, while RDstreamer runs it */
  speed: number | null;
  /** The power button is being listened to */
  power_button: boolean;
  error: string | null;
  limits: {
    max_points: number;
    min_c: number;
    max_c: number;
    full_by_c: number;
    quiet: FanPoint[];
  };
}

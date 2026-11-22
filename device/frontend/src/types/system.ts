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

export interface FanStatus {
  /** argononed (the Argon ONE case's fan service) is installed */
  available: boolean;
  active: boolean;
  curve: FanPoint[];
  temp_c: number | null;
  /** What the curve asks of the fan now; the fan can't be read back */
  speed: number | null;
  limits: {
    max_points: number;
    min_c: number;
    max_c: number;
    full_by_c: number;
    quiet: FanPoint[];
  };
}

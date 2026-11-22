export interface InterfaceStatus {
  device: string;
  state: string;
  connection: string | null;
  addresses: string[];
  role: "ap" | "client" | null;
  ssid: string | null;
  /** USB product name of a tethered phone */
  product: string | null;
  /** "auto" (DHCP), "manual" (static), "shared" (our access point) */
  method: string | null;
  gateway: string | null;
}

export interface NetworkStatus {
  wifi: InterfaceStatus;
  ethernet: InterfaceStatus | null;
  tether: InterfaceStatus | null;
  internet_via: string | null;
  /** usbmuxd is installed, which iPhones need */
  iphone_support: boolean;
  /** The ready-made image: YStreamer owns the network, no "System" modes */
  image: boolean;
  fallback_active: boolean;
  /** Why the fallback access point is on, as a sentence */
  fallback_reason: string | null;
  /** The cause is gone; the access point stays on anyway */
  fallback_resolved: boolean;
  applying: boolean;
  last_error: string | null;
}

export interface ScanResult {
  ssid: string;
  signal: number;
  security: string;
  channel: number;
}

export interface WifiCountries {
  /** What the radio follows right now, if any country at all */
  current: string | null;
  countries: { code: string; name: string }[];
}

/** What the WiFi radio is doing right now (GET /network/link) */
export interface WifiLink {
  role: "ap" | "client" | "off";
  ssid: string | null;
  channel: number | null;
  /** As a client: the router's signal here */
  signal_dbm: number | null;
  bitrate_mbps: number | null;
  /** As an access point: who is connected */
  stations: {
    mac: string;
    name: string | null;
    ip: string | null;
    signal_dbm: number | null;
    bitrate_mbps: number | null;
  }[];
  /** Where streams to the internet leave: wlan0, eth0, usb0... */
  uplink: string | null;
}

/** 2.4 GHz networks around and how busy each channel is */
export interface WifiSurvey {
  networks: {
    bssid: string;
    ssid: string;
    channel: number;
    freq: number;
    signal_dbm: number;
  }[];
  networks_5ghz: number;
  channels: { channel: number; networks: number; load: number }[];
  /** The clearest of 1, 6 and 11 */
  best: number;
  current: number | null;
}

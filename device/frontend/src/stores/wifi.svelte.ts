import { api } from "@/api/client";
import type { WifiLink } from "@/types/network";

/** What the WiFi radio is doing, for the status bar */
class WifiStore {
  link = $state<WifiLink | null>(null);

  #users = 0;
  #timer: ReturnType<typeof setInterval> | null = null;

  start() {
    this.#users++;
    if (this.#timer) return;
    this.#poll();
    this.#timer = setInterval(() => this.#poll(), 5000);
  }

  stop() {
    this.#users = Math.max(0, this.#users - 1);
    if (this.#users > 0 || !this.#timer) return;
    clearInterval(this.#timer);
    this.#timer = null;
  }

  async #poll() {
    if (document.hidden) return;
    try {
      this.link = await api.wifiLink();
    } catch {
      // Keep the last reading
    }
  }
}

export const wifi = new WifiStore();

/** -50 dBm and better is excellent, -80 and worse barely works */
export function signalQuality(dbm: number | null): {
  bars: number;
  tone: string;
  word: string;
} {
  if (dbm === null) return { bars: 0, tone: "text-muted", word: "—" };
  if (dbm >= -55) return { bars: 4, tone: "text-ok", word: "excellent" };
  if (dbm >= -65) return { bars: 3, tone: "text-ok", word: "good" };
  if (dbm >= -72) return { bars: 2, tone: "text-warn", word: "fair" };
  return { bars: 1, tone: "text-down", word: "weak" };
}

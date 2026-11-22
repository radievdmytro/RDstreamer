import { api } from "@/api/client";
import type { FanStatus } from "@/types/system";

/** The Argon ONE fan, for the status bar; null where there's none */
class FanStore {
  status = $state<FanStatus | null>(null);

  #users = 0;
  /** Asked once and there was no fan service: no point asking again */
  #absent = false;
  #failures = 0;
  #timer: ReturnType<typeof setInterval> | null = null;

  start() {
    this.#users++;
    if (this.#timer || this.#absent) return;
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
    try {
      const s = await api.fan();
      this.#failures = 0;
      this.status = s.available ? s : null;
      if (!s.available && this.#timer) {
        this.#absent = true;
        clearInterval(this.#timer);
        this.#timer = null;
      }
    } catch {
      // Keep the last reading; after a few failures in a row, stop asking
      if (++this.#failures >= 3 && this.#timer) {
        clearInterval(this.#timer);
        this.#timer = null;
        this.#failures = 0;
      }
    }
  }
}

export const fan = new FanStore();

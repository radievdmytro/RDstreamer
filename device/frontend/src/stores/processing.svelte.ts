import { api } from "@/api/client";
import type { ProcessingStatus } from "@/types/settings";

const POLL_MS = 2000;

/** Polled while anything shows it */
class ProcessingStore {
  status = $state<ProcessingStatus | null>(null);
  #users = 0;
  #timer: ReturnType<typeof setInterval> | null = null;

  async poll() {
    try {
      this.status = await api.processing();
    } catch {
      // Kept as it was; tried again shortly
    }
  }

  /** Starts polling; call the returned function to stop */
  watch(): () => void {
    this.#users++;
    if (!this.#timer) {
      this.poll();
      this.#timer = setInterval(() => this.poll(), POLL_MS);
    }
    return () => {
      this.#users--;
      if (this.#users === 0 && this.#timer) {
        clearInterval(this.#timer);
        this.#timer = null;
      }
    };
  }
}

export const processing = new ProcessingStore();

export const PROCESSING_LABEL: Record<ProcessingStatus["state"], string> = {
  off: "Not running",
  starting: "Starting",
  running: "Running",
  failed: "Failed",
};

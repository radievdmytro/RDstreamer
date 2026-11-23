<script lang="ts">
  // The page to give guests: the picture and nothing to change on the
  // device. They may pick how their own browser receives it. The owner gets
  // to the settings with three taps on the logo
  import type { Status } from "@/types/stream";
  import type { ViewerQuality, ViewerTransport } from "@/types/settings";
  import Player from "@/components/watch/Player.svelte";
  import Brand from "@/components/ui/Brand.svelte";
  import Segmented from "@/components/ui/Segmented.svelte";
  import { statusDot, statusLabel } from "@/lib/status";
  import { route } from "@/stores/route.svelte";
  import { socketStreamSupported } from "@/lib/socketStream";
  import Settings2 from "@lucide/svelte/icons/settings-2";

  const KEY = "rdViewer";
  let saved: { quality?: ViewerQuality; transport?: ViewerTransport } = {};
  try {
    saved = JSON.parse(localStorage.getItem(KEY) ?? "{}");
  } catch {
    // A browser without storage starts from the device's settings
  }

  let status = $state<Status>("connecting");
  let theater = $state(true);
  let quality = $state<ViewerQuality | null>(saved.quality ?? null);
  let transport = $state<ViewerTransport | null>(saved.transport ?? null);
  let open = $state(false);

  $effect(() => {
    try {
      localStorage.setItem(KEY, JSON.stringify({ quality, transport }));
    } catch {
      // Not kept, fine
    }
  });

  const QUALITIES = [
    { label: "Auto", value: "auto" },
    { label: "Original", value: "original" },
    { label: "Light", value: "light" },
  ];
  const TRANSPORTS = [
    { label: "WebRTC", value: "webrtc" },
    {
      label: "WebSocket",
      value: "websocket",
      blocked: socketStreamSupported() ? null : "Not in this browser",
    },
  ];

  // Three taps on the logo within a second: the owner's way in
  let taps: number[] = [];
  function tap() {
    const now = Date.now();
    taps = [...taps.filter((t) => now - t < 1000), now];
    if (taps.length >= 3) {
      taps = [];
      route.go("/admin");
    }
  }
</script>

<svelte:head>
  <title>RDstreamer · Live</title>
</svelte:head>

<div class="flex min-h-dvh flex-col bg-base">
  <header class="flex items-center justify-between gap-3 px-4 py-3 sm:px-6">
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <span class="cursor-default select-none" onclick={tap}>
      <Brand />
    </span>
    <div class="flex items-center gap-3">
      <span class="flex items-center gap-2 text-[12px] font-medium text-muted">
        <span class="h-2 w-2 rounded-full {statusDot(status)}"></span>
        {statusLabel(status)}
      </span>
      <button
        type="button"
        class="grid h-9 w-9 place-items-center rounded-md border border-line text-muted transition-colors hover:border-line-strong hover:text-fg {open
          ? 'border-primary text-primary'
          : ''}"
        aria-label="Playback options"
        aria-expanded={open}
        onclick={() => (open = !open)}
      >
        <Settings2 class="h-4 w-4" aria-hidden="true" />
      </button>
    </div>
  </header>

  {#if open}
    <div
      class="mx-4 mb-3 flex flex-col gap-4 rounded-xl border border-line bg-surface p-4 sm:mx-6 sm:flex-row sm:items-start"
    >
      <div class="flex-1">
        <Segmented
          label="Quality"
          options={QUALITIES}
          value={quality ?? "auto"}
          onchange={(v) => (quality = v)}
        />
        <p class="mt-1.5 text-[12px] text-muted">
          {#if (quality ?? "auto") === "auto"}
            Full quality, switching to the light picture if your WiFi can't keep
            up.
          {:else if quality === "original"}
            Full quality: needs a strong WiFi signal.
          {:else}
            720p, about 3.5 Mbit/s: steady on weak WiFi, slightly softer.
          {/if}
        </p>
      </div>
      <div class="flex-1">
        <Segmented
          label="Connection"
          options={TRANSPORTS}
          value={transport ?? "webrtc"}
          onchange={(v) => (transport = v)}
        />
        <p class="mt-1.5 text-[12px] text-muted">
          {(transport ?? "webrtc") === "webrtc"
            ? "Lowest delay; skips a frame rather than wait for it."
            : "Steadier frame rate on some phones, a little more delay."}
        </p>
      </div>
    </div>
  {/if}

  <main class="flex flex-1 items-center justify-center px-0 pb-4 sm:px-6">
    <div class="w-full max-w-[min(100%,calc((100dvh-6rem)*16/9))]">
      <Player
        bind:status
        bind:theater
        qualityChoice={quality}
        transportChoice={transport}
      />
    </div>
  </main>
  <footer class="px-4 pb-3 text-center text-[11px] text-faint sm:px-6">
    Double-click (or double-tap) the picture for full screen
  </footer>
</div>

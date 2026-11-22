<script lang="ts">
  import type { Settings } from "@/types/settings";
  import type { Status } from "@/types/stream";
  import Player from "@/components/watch/Player.svelte";
  import Segmented from "@/components/ui/Segmented.svelte";

  let { display }: { display: Settings["display"] } = $props();

  const MODES = [
    { label: "Before", value: 1 },
    { label: "Split", value: 0.5 },
    { label: "After", value: 0 },
  ];

  let split = $state(0);
  let status = $state<Status>("connecting");
  const active = $derived(display.look.enabled || display.dewarp.enabled);
</script>

<!-- Stays in view while the sliders below are moved -->
<section
  class="card sticky top-2 z-20 overflow-hidden rounded-xl border border-line bg-surface shadow-lg shadow-black/30"
>
  <header
    class="flex flex-wrap items-center justify-between gap-3 border-b border-line bg-surface-2/40 px-4 py-2.5"
  >
    <div class="min-w-0">
      <h2 class="font-display text-[15px] leading-tight font-semibold text-fg">
        Live preview
      </h2>
      <p class="mt-0.5 text-[12px] text-muted">
        {active
          ? "The goggles' picture as HDMI will show it — follows the sliders before you save"
          : "Turn on the LUT or the lens correction to see them here"}
      </p>
    </div>
    <div class="w-56 shrink-0">
      <Segmented
        options={MODES}
        value={split}
        disabled={!active}
        onchange={(v) => (split = v)}
      />
    </div>
  </header>
  <div class="mx-auto max-w-3xl p-3 [&_.video-stage]:max-h-[42dvh]">
    <Player bind:status graded={{ display, split: active ? split : 1 }} />
  </div>
</section>

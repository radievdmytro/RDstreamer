<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { metrics } from "@/stores/metrics.svelte";
  import { fan } from "@/stores/fan.svelte";
  import { recording, duration } from "@/stores/recording.svelte";
  import Thermometer from "@lucide/svelte/icons/thermometer";
  import FanIcon from "@lucide/svelte/icons/fan";
  import Cpu from "@lucide/svelte/icons/cpu";
  import ArrowDownToLine from "@lucide/svelte/icons/arrow-down-to-line";

  onMount(() => {
    metrics.start();
    fan.start();
  });
  onDestroy(() => {
    metrics.stop();
    fan.stop();
  });

  const temp = $derived(metrics.latest("temp"));
  const cpu = $derived(metrics.latest("cpu"));
  const kbps = $derived(metrics.latest("kbps_in"));
  const speed = $derived(fan.status?.speed ?? null);

  const tempTone = $derived(
    temp === null
      ? "text-muted"
      : temp >= 75
        ? "text-down"
        : temp >= 62
          ? "text-warn"
          : "text-fg",
  );

  const rate = (k: number) =>
    k >= 1000 ? `${(k / 1000).toFixed(1)} Mbit/s` : `${Math.round(k)} kbit/s`;

  const chip =
    "flex shrink-0 items-center gap-1.5 rounded-full border border-line bg-surface-2/60 px-2.5 py-1 text-[12px] font-medium tabular-nums transition-colors hover:border-line-strong hover:bg-surface-2";
</script>

<div
  class="flex min-w-0 [scrollbar-width:none] items-center gap-1.5 overflow-x-auto"
  aria-label="Device status"
>
  {#if recording.active}
    <a
      href="/recording/files"
      class="{chip} border-down/50 bg-down/15 text-down hover:border-down hover:bg-down/20"
      title="Recording"
    >
      <span class="rec-ring h-2 w-2 rounded-full bg-down"></span>
      REC {duration(recording.elapsed)}
    </a>
  {/if}

  {#if temp !== null}
    <a
      href="/performance"
      class="{chip} {tempTone}"
      title="Processor temperature"
    >
      <Thermometer class="h-3.5 w-3.5 opacity-70" aria-hidden="true" />
      {temp.toFixed(1)}°C
    </a>
  {/if}

  {#if fan.status?.control === "driving"}
    <a
      href="/system/general"
      class="{chip} text-fg"
      title="Cooling fan: what the curve asks of it now"
    >
      <FanIcon
        class="h-3.5 w-3.5 {speed ? 'animate-spin text-primary' : 'opacity-70'}"
        style={speed
          ? `animation-duration: ${Math.max(0.4, 2.4 - speed / 50)}s`
          : undefined}
        aria-hidden="true"
      />
      {speed ? `${speed}%` : "off"}
    </a>
  {/if}

  {#if cpu !== null}
    <a href="/performance" class="{chip} text-fg" title="Processor load">
      <Cpu class="h-3.5 w-3.5 opacity-70" aria-hidden="true" />
      {Math.round(cpu)}%
    </a>
  {/if}

  {#if kbps}
    <a href="/performance" class="{chip} text-fg" title="Incoming video">
      <ArrowDownToLine class="h-3.5 w-3.5 opacity-70" aria-hidden="true" />
      {rate(kbps)}
    </a>
  {/if}
</div>

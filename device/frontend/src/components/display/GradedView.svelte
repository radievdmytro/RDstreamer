<script lang="ts">
  import { onDestroy } from "svelte";
  import type { Settings } from "@/types/settings";
  import { Grader, fetchLut } from "@/lib/grade";

  let {
    video,
    display,
    split,
  }: {
    /** The player's video element, once there is one */
    video: () => HTMLVideoElement | undefined;
    display: Settings["display"];
    /** 0 graded throughout, 0.5 half and half, 1 not graded */
    split: number;
  } = $props();

  let canvas = $state<HTMLCanvasElement>();
  let grader: Grader | null = null;
  let error = $state<string | null>(null);
  let showing = $state(false);
  let frame = 0;

  $effect(() => {
    if (!canvas || grader) return;
    try {
      grader = new Grader(canvas);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      return;
    }
    const loop = () => {
      const v = video();
      showing = !!(grader && v && grader.draw(v, display, split));
      frame = requestAnimationFrame(loop);
    };
    frame = requestAnimationFrame(loop);
  });

  // The LUT the Pi would use, fetched again when another one is picked
  $effect(() => {
    const id = display.look.lut;
    const on = display.look.enabled;
    if (!grader || !on) return;
    let current = true;
    fetchLut(id)
      .then((lut) => current && grader?.setLut(lut))
      .catch(() => current && grader?.setLut(null));
    return () => {
      current = false;
    };
  });

  onDestroy(() => cancelAnimationFrame(frame));
</script>

<canvas
  bind:this={canvas}
  class="pointer-events-none absolute inset-0 h-full w-full object-contain {showing
    ? ''
    : 'invisible'}"
></canvas>
{#if error}
  <p
    class="absolute top-3 right-3 rounded-md bg-black/60 px-2 py-1 text-[11px] text-white/80"
  >
    No preview: {error}
  </p>
{/if}
{#if showing && split > 0 && split < 1}
  <div
    class="pointer-events-none absolute inset-y-0 w-px bg-primary shadow-[0_0_8px_var(--color-primary)]"
    style="left: {split * 100}%"
  >
    <span
      class="absolute top-3 right-2 rounded bg-black/60 px-1.5 py-0.5 text-[10px] font-semibold tracking-wide text-white/80 uppercase"
      >Before</span
    >
    <span
      class="absolute top-3 left-2 rounded bg-black/60 px-1.5 py-0.5 text-[10px] font-semibold tracking-wide text-primary uppercase"
      >After</span
    >
  </div>
{/if}

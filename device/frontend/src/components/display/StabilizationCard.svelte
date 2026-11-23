<script lang="ts">
  import type { Settings } from "@/types/settings";
  import Card from "@/components/ui/Card.svelte";
  import Field from "@/components/ui/Field.svelte";
  import Switch from "@/components/ui/Switch.svelte";
  import Slider from "@/components/ui/Slider.svelte";
  import Segmented from "@/components/ui/Segmented.svelte";

  let { display = $bindable() }: { display: Settings["display"] } = $props();

  const off = $derived(!display.stabilization.enabled);

  const DELAY = [
    { label: "Live", value: 0 },
    { label: "0.25 s", value: 250 },
    { label: "0.5 s", value: 500 },
    { label: "1 s", value: 1000 },
  ];
</script>

<Card
  title="Stabilisation"
  description="Takes the shake out, from the picture itself: each frame is compared with the one before and moved back along a smoothed path. It works on the straightened picture (after Dewarp), so the edges don't wobble like jelly. Deliberate turns and pans come through."
>
  <Field label="Stabilise" for="stab-enabled">
    <Switch id="stab-enabled" bind:checked={display.stabilization.enabled} />
  </Field>

  <div
    class="flex flex-col gap-4 border-t border-line px-4 py-3"
    class:opacity-45={off}
  >
    <Slider
      label="Shake"
      value={display.stabilization.shake}
      min={0}
      max={100}
      format={(v) => (v === 0 ? "Off" : `${v}%`)}
      minLabel="Light"
      maxLabel="Locked"
      disabled={off}
      onchange={(v) => (display.stabilization.shake = v)}
    />
    <Slider
      label="Horizon"
      value={display.stabilization.horizon}
      min={0}
      max={100}
      format={(v) => (v === 0 ? "Left alone" : `${v}%`)}
      minLabel="Off"
      maxLabel="Level"
      disabled={off}
      onchange={(v) => (display.stabilization.horizon = v)}
    />
    <Slider
      label="Zoom"
      value={display.stabilization.crop}
      min={2}
      max={30}
      format={(v) => `+${v}%`}
      minLabel="Wide"
      maxLabel="More room"
      note="The room the picture has to move in. Too little and big shakes reach the edge; the picture is never stretched, only zoomed evenly."
      disabled={off}
      onchange={(v) => (display.stabilization.crop = v)}
    />
  </div>

  <Field
    label="Look ahead"
    for="stab-delay"
    help={display.stabilization.delay_ms === 0
      ? "Live: smoothed from what's already been seen, no added delay. Fine for watching while flying."
      : `Everything that's processed is shown ${display.stabilization.delay_ms / 1000} s later, and each frame is smoothed with what comes after it too: steadier, and pans start without lag. For recording-quality streams, not for flying.`}
    disabled={off}
  >
    <div class="w-full sm:w-80">
      <Segmented
        options={DELAY}
        value={display.stabilization.delay_ms}
        disabled={off}
        onchange={(v) => (display.stabilization.delay_ms = v)}
      />
    </div>
  </Field>
  <p class="px-4 pb-3 text-[12px] text-faint">
    Shows on HDMI and in each stream that has “Image processing” on (the
    browsers have it on by default), not in the live preview above.
  </p>
</Card>

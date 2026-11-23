<script lang="ts">
  import type { Settings } from "@/types/settings";
  import Card from "@/components/ui/Card.svelte";
  import Field from "@/components/ui/Field.svelte";
  import Switch from "@/components/ui/Switch.svelte";
  import Slider from "@/components/ui/Slider.svelte";

  let { display = $bindable() }: { display: Settings["display"] } = $props();

  const zoom = $derived(
    Math.round(
      (0.1 + 0.2 * (display.stabilization.strength / 100)) * 0.9 * 100,
    ),
  );
</script>

<Card
  title="Stabilisation"
  description="Takes the shake out, from the picture itself: each frame is compared with the one before and moved back along a smoothed path, zoomed in a little so the edges stay out of sight. Deliberate turns and pans come through."
>
  <Field label="Stabilise" for="stab-enabled">
    <Switch id="stab-enabled" bind:checked={display.stabilization.enabled} />
  </Field>

  <div
    class="flex flex-col gap-4 border-t border-line px-4 py-3"
    class:opacity-45={!display.stabilization.enabled}
  >
    <Slider
      label="Strength"
      value={display.stabilization.strength}
      min={0}
      max={100}
      format={(v) => `${v}% · zoom +${zoom}%`}
      disabled={!display.stabilization.enabled}
      onchange={(v) => (display.stabilization.strength = v)}
    />
  </div>
  <Field
    label="Level out turning"
    for="stab-roll"
    help="Also steadies the horizon's wobble, not only up-down and sideways shake."
    disabled={!display.stabilization.enabled}
  >
    <Switch
      id="stab-roll"
      disabled={!display.stabilization.enabled}
      bind:checked={display.stabilization.roll}
    />
  </Field>
  <p class="px-4 pb-3 text-[12px] text-faint">
    Shows on HDMI (and in the streams if “Also in streams and browsers” is on),
    not in the live preview above. Fast FPV flying keeps its motion: it's the
    jitter that goes.
  </p>
</Card>

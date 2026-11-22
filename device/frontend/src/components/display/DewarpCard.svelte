<script lang="ts">
  import type { Settings } from "@/types/settings";
  import Card from "@/components/ui/Card.svelte";
  import Field from "@/components/ui/Field.svelte";
  import Switch from "@/components/ui/Switch.svelte";
  import Slider from "@/components/ui/Slider.svelte";
  import { btn } from "@/components/ui/buttons";

  let { display = $bindable() }: { display: Settings["display"] } = $props();

  const signed = (v: number) => `${v > 0 ? "+" : ""}${v}%`;

  function reset() {
    display.dewarp = {
      ...display.dewarp,
      fisheye: 30,
      edge: 0,
      zoom: 10,
      offset_x: 0,
      offset_y: 0,
    };
  }
</script>

<Card
  title="Dewarp"
  description="Straightens a fisheye lens's curved lines. Correcting pulls the corners in, so zoom in a little to crop away the black edges."
>
  <Field label="Correct the lens" for="dewarp-enabled">
    <Switch id="dewarp-enabled" bind:checked={display.dewarp.enabled} />
  </Field>

  <div
    class="flex flex-col gap-4 border-t border-line px-4 py-3"
    class:opacity-45={!display.dewarp.enabled}
  >
    <Slider
      label="Fisheye correction"
      value={display.dewarp.fisheye}
      min={-100}
      max={100}
      format={signed}
      disabled={!display.dewarp.enabled}
      onchange={(v) => (display.dewarp.fisheye = v)}
    />
    <Slider
      label="Edge correction"
      value={display.dewarp.edge}
      min={-100}
      max={100}
      format={signed}
      disabled={!display.dewarp.enabled}
      onchange={(v) => (display.dewarp.edge = v)}
    />
    <Slider
      label="Zoom / crop"
      value={display.dewarp.zoom}
      min={-50}
      max={100}
      format={signed}
      disabled={!display.dewarp.enabled}
      onchange={(v) => (display.dewarp.zoom = v)}
    />
    <Slider
      label="Offset X"
      value={display.dewarp.offset_x}
      min={-50}
      max={50}
      format={signed}
      disabled={!display.dewarp.enabled}
      onchange={(v) => (display.dewarp.offset_x = v)}
    />
    <Slider
      label="Offset Y"
      value={display.dewarp.offset_y}
      min={-50}
      max={50}
      format={signed}
      disabled={!display.dewarp.enabled}
      onchange={(v) => (display.dewarp.offset_y = v)}
    />
  </div>

  <div class="flex justify-end px-4 py-3">
    <button
      type="button"
      class={btn.plain}
      disabled={!display.dewarp.enabled}
      onclick={reset}>Reset</button
    >
  </div>
</Card>

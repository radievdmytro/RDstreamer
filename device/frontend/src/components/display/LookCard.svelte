<script lang="ts">
  import { onMount } from "svelte";
  import { api, message } from "@/api/client";
  import type { LookStatus, Settings } from "@/types/settings";
  import { snackbar } from "@/stores/snackbar.svelte";
  import Card from "@/components/ui/Card.svelte";
  import Field from "@/components/ui/Field.svelte";
  import Switch from "@/components/ui/Switch.svelte";
  import Select from "@/components/ui/Select.svelte";
  import Segmented from "@/components/ui/Segmented.svelte";
  import Slider from "@/components/ui/Slider.svelte";
  import { btn } from "@/components/ui/buttons";

  let { display = $bindable() }: { display: Settings["display"] } = $props();

  const QUALITY = [
    { label: "Speed · 720p", value: "speed" as const },
    { label: "Quality · 1080p", value: "quality" as const },
  ];

  let status = $state<LookStatus | null>(null);
  let busy = $state(false);
  let fileInput = $state<HTMLInputElement>();

  async function refresh() {
    try {
      status = await api.look();
    } catch {
      // The card still works with what's saved
    }
  }

  onMount(refresh);

  const options = $derived.by(() => {
    const list = (status?.presets ?? []).map((p) => ({
      label: p.label,
      value: p.id,
    }));
    if (status?.custom)
      list.push({
        label: `Custom: ${status.custom.title || "uploaded .cube"} (${status.custom.size}-point)`,
        value: "custom",
      });
    else if (display.look.lut === "custom")
      list.push({ label: "Custom (file missing)", value: "custom" });
    return list;
  });

  async function upload(file: File) {
    busy = true;
    try {
      const info = await api.uploadLut(file);
      await refresh();
      display.look.lut = "custom";
      snackbar.show(
        `LUT ${info.title ? `"${info.title}" ` : ""}loaded (${info.size}-point). Save to apply.`,
      );
    } catch (e) {
      snackbar.show(message(e), true);
    } finally {
      busy = false;
    }
  }

  async function remove() {
    busy = true;
    try {
      await api.removeLut();
      if (display.look.lut === "custom") display.look.lut = "film";
      await refresh();
      snackbar.show("Custom LUT removed");
    } catch (e) {
      snackbar.show(message(e), true);
    } finally {
      busy = false;
    }
  }
</script>

<Card
  title="Look"
  description="Colour grading on the HDMI picture, done on the GPU. Streams, recordings and the browser view are not changed."
>
  {#if status?.error}
    <p class="bg-down/10 px-4 py-3 text-[12px] text-down">{status.error}</p>
  {/if}

  <Field label="Apply a LUT" for="look-enabled">
    <Switch id="look-enabled" bind:checked={display.look.enabled} />
  </Field>

  <Field
    label="Processing"
    help={display.look.quality === "speed"
      ? "Graded at 720p and scaled up by the display: smooth at 60 fps, a little softer."
      : "Graded at full size: sharpest, but a Pi 4 manages about 48 fps, so fast motion can judder."}
    disabled={!display.look.enabled}
  >
    <div class="w-full sm:w-80">
      <Segmented
        options={QUALITY}
        value={display.look.quality}
        disabled={!display.look.enabled}
        onchange={(v) => (display.look.quality = v)}
      />
    </div>
  </Field>

  <Field label="LUT" for="look-lut" disabled={!display.look.enabled}>
    <Select
      id="look-lut"
      bind:value={display.look.lut}
      {options}
      disabled={!display.look.enabled}
    />
  </Field>

  <div
    class="border-t border-line px-4 py-3"
    class:opacity-45={!display.look.enabled}
  >
    <Slider
      label="Strength"
      value={display.look.strength}
      min={0}
      max={100}
      step={5}
      format={(v) => `${v}%`}
      disabled={!display.look.enabled}
      onchange={(v) => (display.look.strength = v)}
    />
  </div>

  <Field
    label="Your own LUT"
    help="A 3D .cube file, e.g. a manufacturer's LOG to Rec.709 LUT or one exported from DaVinci Resolve. Up to 65 points."
  >
    <div class="flex gap-2">
      <input
        bind:this={fileInput}
        type="file"
        accept=".cube,text/plain"
        class="hidden"
        onchange={(e) => {
          const f = e.currentTarget.files?.[0];
          e.currentTarget.value = "";
          if (f) upload(f);
        }}
      />
      <button
        type="button"
        class={btn.plain}
        disabled={busy}
        onclick={() => fileInput?.click()}
        >{busy ? "Loading…" : status?.custom ? "Replace…" : "Upload…"}</button
      >
      {#if status?.custom}
        <button
          type="button"
          class={btn.danger}
          disabled={busy}
          onclick={remove}>Remove</button
        >
      {/if}
    </div>
  </Field>
</Card>

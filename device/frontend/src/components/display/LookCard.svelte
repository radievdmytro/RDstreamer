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
  let busy = $state<string | null>(null);
  let fileInput = $state<HTMLInputElement>();

  async function refresh() {
    try {
      status = await api.look();
    } catch {
      // The card still works with what's saved
    }
  }

  onMount(refresh);

  const gpu = $derived(display.look.enabled || display.dewarp.enabled);
  const isFile = $derived(display.look.lut.startsWith("file:"));
  const selectedFile = $derived(
    status?.files.find((f) => f.id === display.look.lut) ?? null,
  );

  const options = $derived.by(() => {
    const list = (status?.presets ?? []).map((p) => ({
      label: p.label,
      value: p.id,
    }));
    for (const f of status?.files ?? [])
      list.push({ label: `${f.name} (${f.size}-point)`, value: f.id });
    if (isFile && !selectedFile)
      list.push({
        label: "LUT no longer in the library",
        value: display.look.lut,
      });
    return list;
  });

  async function upload(files: File[]) {
    busy = "upload";
    let added = 0;
    let last: string | null = null;
    const failed: string[] = [];
    for (const file of files) {
      try {
        const saved = await api.uploadLut(file.name, file);
        added++;
        last = saved.id;
      } catch (e) {
        failed.push(`${file.name}: ${message(e)}`);
      }
    }
    await refresh();
    if (added === 1 && last) display.look.lut = last;
    busy = null;
    if (failed.length) snackbar.show(failed.join(" · "), true);
    else
      snackbar.show(
        added === 1
          ? "LUT added and selected. Save to apply."
          : `${added} LUTs added to the list`,
      );
  }

  async function downloadPack() {
    busy = "download";
    try {
      const { added } = await api.downloadLuts();
      await refresh();
      snackbar.show(`${added} LUTs added to the list`);
    } catch (e) {
      snackbar.show(message(e), true);
    } finally {
      busy = null;
    }
  }

  async function removeSelected() {
    const file = selectedFile;
    if (!file) return;
    busy = "remove";
    try {
      await api.removeLut(file.id);
      display.look.lut = "film";
      await refresh();
      snackbar.show(`${file.name} removed`);
    } catch (e) {
      snackbar.show(message(e), true);
    } finally {
      busy = null;
    }
  }
</script>

<Card
  title="Processing"
  description="The LUT and dewarp are done on the GPU, for HDMI only. Streams, recordings and the browser view are not changed."
>
  {#if status?.error}
    <p class="bg-down/10 px-4 py-3 text-[12px] text-down">{status.error}</p>
  {/if}
  <Field
    label="Quality"
    help={display.look.quality === "speed"
      ? "Worked at 720p and scaled up by the display: smooth at 60 fps, a little softer."
      : "Worked at full size: sharpest, but a Pi 4 manages about 48 fps, so fast motion can judder."}
    disabled={!gpu}
  >
    <div class="w-full sm:w-80">
      <Segmented
        options={QUALITY}
        value={display.look.quality}
        disabled={!gpu}
        onchange={(v) => (display.look.quality = v)}
      />
    </div>
  </Field>
</Card>

<Card title="LUT">
  <Field label="Apply a LUT" for="look-enabled">
    <Switch id="look-enabled" bind:checked={display.look.enabled} />
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
    label="Add LUTs"
    help="3D .cube files, several at once: DJI's official D-Log / D-Log M / D-Cinelike to Rec.709 LUTs for your camera (dji.com/lut), ones exported from DaVinci Resolve, or any other."
  >
    <div class="flex flex-wrap justify-end gap-2">
      <input
        bind:this={fileInput}
        type="file"
        accept=".cube,text/plain"
        multiple
        class="hidden"
        onchange={(e) => {
          const input = e.currentTarget;
          const picked = [...(input.files ?? [])];
          input.value = "";
          if (picked.length) upload(picked);
        }}
      />
      <button
        type="button"
        class={btn.plain}
        disabled={!!busy}
        onclick={() => fileInput?.click()}
        >{busy === "upload" ? "Adding…" : "Upload .cube…"}</button
      >
      {#if selectedFile}
        <button
          type="button"
          class={btn.danger}
          disabled={!!busy}
          onclick={removeSelected}
          >{busy === "remove" ? "Removing…" : "Remove selected"}</button
        >
      {/if}
    </div>
  </Field>

  <Field
    label="Free LUT pack"
    help="Todd Blankenship's 13 looks (Crisp Autumn, Hard Boost, Long Beach Morning, Orange and Blue, Waves…), downloaded by the device from G'MIC's site. Needs internet."
  >
    <button
      type="button"
      class={btn.plain}
      disabled={!!busy}
      onclick={downloadPack}
      >{busy === "download" ? "Downloading…" : "Download"}</button
    >
  </Field>
</Card>

<script lang="ts">
  import { onMount } from "svelte";
  import { api, message } from "@/api/client";
  import type { Settings, SplashVideo } from "@/types/settings";
  import { settings } from "@/stores/settings.svelte";
  import { snackbar } from "@/stores/snackbar.svelte";
  import Card from "@/components/ui/Card.svelte";
  import Field from "@/components/ui/Field.svelte";
  import Switch from "@/components/ui/Switch.svelte";
  import { btn } from "@/components/ui/buttons";
  import Film from "@lucide/svelte/icons/film";
  import Power from "@lucide/svelte/icons/power";

  let { splash = $bindable() }: { splash: Settings["splash"] } = $props();

  let video = $state<SplashVideo>({ state: "none" });
  let uploading = $state<number | null>(null);
  let switching = $state(false);
  let input: HTMLInputElement;
  let timer: ReturnType<typeof setInterval> | null = null;

  async function refresh() {
    try {
      video = (await api.splash()).video;
    } catch {
      // Shown as it was
    }
    // Only asked again while it's being made ready
    if (video.state !== "converting" && timer) {
      clearInterval(timer);
      timer = null;
      if (video.state === "ready") snackbar.show("Background video ready");
      if (video.state === "failed") snackbar.show(video.error, true);
    }
  }

  onMount(() => {
    refresh();
    return () => timer && clearInterval(timer);
  });

  function watch() {
    if (!timer) timer = setInterval(refresh, 2000);
  }

  // The request with progress, which fetch can't report for uploads
  function send(file: File): Promise<void> {
    return new Promise((resolve, reject) => {
      const xhr = new XMLHttpRequest();
      xhr.open("PUT", "/api/splash/video");
      xhr.upload.onprogress = (e) => {
        if (e.lengthComputable) uploading = e.loaded / e.total;
      };
      xhr.onload = () =>
        xhr.status < 300
          ? resolve()
          : reject(
              new Error(xhr.responseText || `Upload failed (${xhr.status})`),
            );
      xhr.onerror = () => reject(new Error("Upload failed"));
      xhr.send(file);
    });
  }

  async function upload(e: Event) {
    const file = (e.currentTarget as HTMLInputElement).files?.[0];
    input.value = "";
    if (!file) return;
    if (file.size > 400 * 1024 * 1024) {
      snackbar.show("Up to 400 MB: shorten the clip first", true);
      return;
    }
    uploading = 0;
    try {
      await send(file);
      video = { state: "converting", seconds: 0 };
      watch();
    } catch (err) {
      snackbar.show(message(err), true);
    } finally {
      uploading = null;
    }
  }

  async function remove() {
    try {
      await api.removeSplashVideo();
      video = { state: "none" };
    } catch (err) {
      snackbar.show(message(err), true);
    }
  }

  // Straight away, without Save: the one-click switch
  async function toggle() {
    const on = !splash.background_video;
    const saved = settings.saved?.splash;
    if (!saved) return;
    switching = true;
    try {
      await api.saveSection("splash", { ...saved, background_video: on });
      splash.background_video = on;
      await settings.refresh("splash");
      snackbar.show(on ? "Background video on" : "Background video off");
    } catch (err) {
      snackbar.show(message(err), true);
    } finally {
      switching = false;
    }
  }

  const ready = $derived(video.state === "ready");
</script>

<Card
  title="Background video"
  description="A video loop behind the splash screen instead of the plain background, with the status and login details over it on HDMI."
>
  {#snippet aside()}
    {#if ready}
      <button
        type="button"
        class="{splash.background_video ? btn.primary : btn.plain} min-w-28"
        disabled={switching}
        onclick={toggle}
        title="Applies at once"
      >
        <Power class="h-4 w-4" aria-hidden="true" />
        {splash.background_video ? "Video on" : "Video off"}
      </button>
    {/if}
  {/snippet}

  <div class="flex flex-col gap-3 px-4 py-3">
    <div class="flex items-start gap-3 text-[13px]">
      <Film class="mt-0.5 h-4 w-4 shrink-0 text-muted" aria-hidden="true" />
      <div class="min-w-0 flex-1">
        {#if uploading !== null}
          <p class="text-fg">Uploading… {Math.round(uploading * 100)}%</p>
          <div class="mt-1.5 h-1.5 overflow-hidden rounded-full bg-surface-2">
            <div
              class="h-full rounded-full bg-primary"
              style="width: {uploading * 100}%"
            ></div>
          </div>
        {:else if video.state === "converting"}
          <p class="text-fg">
            Getting it ready… <span class="text-muted tabular-nums"
              >{video.seconds} s</span
            >
          </p>
          <p class="mt-1 text-[12px] text-muted">
            An H.264 video up to 1080p takes seconds. 4K or another codec is
            converted to 1080p on the Pi: about as long as the clip, or a few
            times that for 4K. The splash keeps its plain background meanwhile.
          </p>
        {:else if video.state === "ready"}
          <p class="text-fg">
            {video.width}×{video.height} · {Math.round(video.fps)} fps{video.converted
              ? " · converted"
              : ""}
          </p>
          <p class="mt-1 text-[12px] text-muted">
            {splash.background_video
              ? "Plays in a loop on HDMI, in the browser and in the streams while there's no video from the goggles."
              : "Off: the splash screen shows its plain background."}
          </p>
        {:else if video.state === "failed"}
          <p class="text-down">{video.error}</p>
        {:else}
          <p class="text-muted">
            No video. Any video file works: H.264 up to 1080p plays as it is, 4K
            or other codecs are converted to 1080p. A short loop (10–60 s) is
            best.
          </p>
        {/if}
      </div>
    </div>
    <div class="flex flex-wrap items-center gap-2">
      <button
        type="button"
        class={btn.plain}
        disabled={uploading !== null || video.state === "converting"}
        onclick={() => input.click()}
      >
        {video.state === "none" ? "Upload video" : "Replace video"}
      </button>
      {#if video.state === "ready" || video.state === "failed"}
        <button type="button" class={btn.text} onclick={remove}>Remove</button>
      {/if}
    </div>
    <input
      bind:this={input}
      type="file"
      accept="video/*,.mp4,.mov,.mkv,.h264"
      class="hidden"
      onchange={upload}
    />
  </div>
  <Field
    label="Show connection details over the video"
    for="splash-video-status"
    help="Status, addresses and Wi-Fi login on HDMI. Off: just the video. Where and how they show: “Details layout” in the Splash screen card."
    disabled={!ready}
  >
    <Switch
      id="splash-video-status"
      bind:checked={splash.status_over_video}
      disabled={!ready}
    />
  </Field>
</Card>

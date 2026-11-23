<script lang="ts">
  import { onMount } from "svelte";
  import { api, message } from "@/api/client";
  import type { Release, UpdateStatus } from "@/types/system";
  import type { Settings } from "@/types/settings";
  import { snackbar } from "@/stores/snackbar.svelte";
  import { update } from "@/stores/update.svelte";
  import Switch from "@/components/ui/Switch.svelte";
  import ReleaseNotes from "@/components/system/ReleaseNotes.svelte";
  import Card from "@/components/ui/Card.svelte";
  import Field from "@/components/ui/Field.svelte";
  import ConfirmDialog from "@/components/ui/ConfirmDialog.svelte";
  import { btn } from "@/components/ui/buttons";

  let { advanced = $bindable() }: { advanced: Settings["advanced"] } = $props();

  const POLL_MS = 1000;

  let status = $state<UpdateStatus | null>(null);
  let busy = $state(false);
  let latest = $state<Release | null>(null);
  let checking = $state(false);
  let downloading = $state(false);
  /** The version being installed, from the request until it's running */
  let installing = $state<string | null>(null);
  /** Seen underway or restarting, so the same version reported again means done */
  let underway = $state(false);
  let confirm = $state<"install" | "rollback" | null>(null);
  let confirmOpen = $state(false);
  let input: HTMLInputElement;

  onMount(() => {
    refresh();
    const timer = setInterval(
      () => (installing || downloading || status?.downloading) && refresh(),
      POLL_MS,
    );
    return () => clearInterval(timer);
  });

  async function refresh() {
    let next: UpdateStatus;
    try {
      next = await api.update();
    } catch {
      // Expected while the service restarts into the new version
      underway = true;
      return;
    }
    status = next;
    latest ??= next.available;
    update.available = next.available;
    if (!installing) return;
    if (next.installing) {
      underway = true;
    } else if (underway && next.current === installing && !next.failure) {
      // This page is the old version's; the new one comes with a reload
      location.reload();
    } else if (next.failure) {
      installing = null;
    }
  }

  async function upload(e: Event) {
    const files = [...((e.currentTarget as HTMLInputElement).files ?? [])];
    input.value = "";
    if (!files.length) return;
    const deb = files.find((f) => f.name.endsWith(".deb"));
    const sig = files.find((f) => f.name.endsWith(".sig"));
    if (!deb || !sig) {
      snackbar.show("Choose both the .deb file and its .sig file", true);
      return;
    }
    busy = true;
    try {
      status = await api.uploadUpdate(deb, (await sig.text()).trim());
    } catch (e) {
      snackbar.show(message(e), true);
    } finally {
      busy = false;
    }
  }

  async function check() {
    checking = true;
    try {
      latest = await api.checkUpdate();
      update.available = latest.newer ? latest : null;
    } catch (e) {
      snackbar.show(message(e), true);
    } finally {
      checking = false;
    }
  }

  const mb = (b: number) => (b / 1048576).toFixed(1);
  const rate = (d: NonNullable<UpdateStatus["downloading"]>) =>
    d.seconds > 0.5 ? d.bytes / d.seconds : 0;
  // Seconds since the install was asked for, to show it's still moving
  let installStarted = $state(0);
  let now = $state(Date.now());
  $effect(() => {
    if (!installing) return;
    const t = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(t);
  });

  async function download() {
    downloading = true;
    refresh();
    try {
      status = await api.downloadUpdate();
      latest = null;
    } catch (e) {
      snackbar.show(message(e), true);
    } finally {
      downloading = false;
    }
  }

  async function discard() {
    try {
      status = await api.discardUpdate();
    } catch (e) {
      snackbar.show(message(e), true);
    }
  }

  function ask(what: "install" | "rollback") {
    confirm = what;
    confirmOpen = true;
  }

  async function run() {
    const version = confirm === "install" ? status?.staged : status?.previous;
    if (!version) return;
    try {
      await (confirm === "install"
        ? api.installUpdate()
        : api.rollBackUpdate());
      underway = false;
      installing = version;
      installStarted = Date.now();
    } catch (e) {
      snackbar.show(message(e), true);
    }
  }
</script>

<Card title="Updates">
  {#if !status}
    <p class="px-4 py-3 text-[13px] text-muted">Loading…</p>
  {:else if installing}
    <div class="flex flex-col gap-2 px-4 py-3">
      <p class="text-[13px] text-fg">
        Installing {installing}…
        <span class="text-muted tabular-nums"
          >{Math.max(0, Math.round((now - installStarted) / 1000))} s</span
        >
      </p>
      <div class="h-1.5 overflow-hidden rounded-full bg-surface-2">
        <div
          class="h-full w-1/3 animate-[slide_1.4s_ease-in-out_infinite] rounded-full bg-primary"
        ></div>
      </div>
      <p class="truncate font-mono text-[12px] text-muted">
        {status?.step ??
          (underway
            ? "Starting the new version…"
            : "Preparing the installation…")}
      </p>
      <p class="text-[12px] text-faint">
        Usually under a minute; longer when it fetches packages it needs. Video
        stops while RDstreamer restarts, and this page reloads when it's done.
        If the new version doesn't start, the one before comes back.
      </p>
    </div>
  {:else}
    <Field label="Installed version">
      <span class="text-[13px] text-muted tabular-nums">{status.current}</span>
    </Field>
    {#if status.downloading}
      {@const d = status.downloading}
      <div class="flex flex-col gap-1.5 px-4 py-3">
        <div class="flex items-baseline justify-between text-[13px]">
          <span class="text-fg">Downloading {d.version}</span>
          <span class="text-muted tabular-nums">
            {mb(d.bytes)}{d.total ? ` / ${mb(d.total)}` : ""} MB
            {#if rate(d)}· {Math.round(rate(d) / 1024)} KB/s{/if}
            · {Math.round(d.seconds)} s
          </span>
        </div>
        <div class="h-1.5 overflow-hidden rounded-full bg-surface-2">
          {#if d.total}
            <div
              class="h-full rounded-full bg-primary transition-[width] duration-700"
              style="width: {Math.min(100, (d.bytes / d.total) * 100)}%"
            ></div>
          {:else}
            <div
              class="h-full w-1/3 animate-[slide_1.4s_ease-in-out_infinite] rounded-full bg-primary"
            ></div>
          {/if}
        </div>
        <p class="text-[12px] text-faint">
          {#if d.bytes === 0 && d.seconds > 10}
            Nothing has arrived yet: the device may have no internet. It gives
            up and says so after 20 s without data.
          {:else}
            About 4 MB. Then the signature is checked and it's ready to install.
          {/if}
        </p>
      </div>
    {/if}
    {#if status.staged}
      <Field
        label="Ready to install"
        help={status.staged === status.current
          ? "This is the version already installed."
          : undefined}
      >
        <div class="flex items-center gap-2">
          <span class="text-[13px] text-fg tabular-nums">{status.staged}</span>
          <button type="button" class={btn.text} onclick={discard}>
            Discard
          </button>
          <button
            type="button"
            class={btn.primary}
            onclick={() => ask("install")}
          >
            Install
          </button>
        </div>
      </Field>
    {:else}
      {#if latest?.newer}
        <Field label="Version {latest.version} is available">
          <button
            type="button"
            class={btn.primary}
            disabled={downloading || !!status.downloading}
            onclick={download}
          >
            {downloading || status.downloading ? "Downloading…" : "Download"}
          </button>
        </Field>
        {#if latest.notes}
          <div class="px-4 pb-3">
            <ReleaseNotes notes={latest.notes} />
          </div>
        {/if}
      {:else}
        <Field
          label="Check for updates"
          help={latest ? "This is the latest version." : undefined}
        >
          <button
            type="button"
            class={btn.plain}
            disabled={checking}
            onclick={check}
          >
            {checking ? "Checking…" : "Check"}
          </button>
        </Field>
      {/if}
      <Field
        label="Install from files"
        help="Choose a release's .deb file together with its .sig file."
      >
        <button
          type="button"
          class={btn.plain}
          disabled={busy}
          onclick={() => input.click()}
        >
          {busy ? "Checking…" : "Choose files…"}
        </button>
      </Field>
    {/if}
    {#if status.previous}
      <Field label="Previous version">
        <div class="flex items-center gap-2">
          <span class="text-[13px] text-muted tabular-nums">
            {status.previous}
          </span>
          <button
            type="button"
            class={btn.plain}
            onclick={() => ask("rollback")}
          >
            Roll back
          </button>
        </div>
      </Field>
    {/if}
    <Field
      label="Check automatically"
      for="update-check"
      help="Asks GitHub about twice a day and downloads a newer version (about 4 MB), ready to install with one click."
    >
      <Switch id="update-check" bind:checked={advanced.check_for_updates} />
    </Field>
    <Field
      label="Install automatically"
      for="update-auto"
      disabled={!advanced.check_for_updates}
      help="Installs a downloaded update by itself, but only after 5 minutes with no video from the goggles, no recording, no stream and nobody watching. If it doesn't start, the version before comes back."
    >
      <Switch
        id="update-auto"
        disabled={!advanced.check_for_updates}
        bind:checked={advanced.auto_install_updates}
      />
    </Field>
    {#if status.failure}
      <div class="border-t border-line px-4 py-3">
        <p class="text-[13px] text-down">
          {#if status.failure.reverted}
            Version {status.failure.version} didn't start, so the device went back
            to {status.current}.
          {:else}
            Installing {status.failure.version} failed.
          {/if}
        </p>
        {#if status.failure.log}
          <pre
            class="mt-2 max-h-48 overflow-auto rounded-lg bg-surface-2 p-3 font-mono text-[11px] whitespace-pre-wrap text-muted">{status
              .failure.log}</pre>
        {/if}
      </div>
    {/if}
  {/if}
  <input
    bind:this={input}
    type="file"
    accept=".deb,.sig"
    multiple
    class="hidden"
    onchange={upload}
  />
</Card>

<ConfirmDialog
  bind:open={confirmOpen}
  title={confirm === "install"
    ? `Install ${status?.staged}?`
    : `Roll back to ${status?.previous}?`}
  message="Video, streams and any recording stop while RDstreamer restarts. Don't unplug the device until it's back."
  confirmLabel={confirm === "install" ? "Install" : "Roll back"}
  onconfirm={run}
/>

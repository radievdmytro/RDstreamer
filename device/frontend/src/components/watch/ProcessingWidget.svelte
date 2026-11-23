<script lang="ts">
  import { onMount } from "svelte";
  import { api, message } from "@/api/client";
  import { settings } from "@/stores/settings.svelte";
  import { snackbar } from "@/stores/snackbar.svelte";
  import { processing, PROCESSING_LABEL } from "@/stores/processing.svelte";

  let busy = $state(false);

  onMount(() => {
    if (!settings.saved) settings.load();
    return processing.watch();
  });

  const on = $derived(settings.saved?.viewer.processed ?? false);
  const st = $derived(processing.status);

  /** Saved at once: it's what this player shows */
  async function toggle(next: boolean) {
    const viewer = settings.saved?.viewer;
    if (!viewer || busy) return;
    busy = true;
    try {
      settings.replace(
        await api.saveSection("viewer", { ...viewer, processed: next }),
      );
      processing.poll();
    } catch (e) {
      snackbar.show(message(e), true);
    } finally {
      busy = false;
    }
  }
</script>

<section class="card overflow-hidden rounded-xl border border-line bg-surface">
  <header
    class="flex items-center justify-between gap-3 border-b border-line bg-surface-2/40 px-4 py-3"
  >
    <h2 class="font-display text-[15px] font-semibold text-fg">
      Processing <span class="font-normal text-muted">· in browsers</span>
    </h2>
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label="Processing in browsers"
      disabled={busy || !settings.saved}
      onclick={() => toggle(!on)}
      class="relative inline-flex h-5 w-9 shrink-0 items-center rounded-full transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary disabled:opacity-60
        {on ? 'bg-primary' : 'bg-surface-3'}"
    >
      <span
        class="inline-flex h-4 w-4 rounded-full bg-white shadow-sm transition-transform duration-150
          {on ? 'translate-x-4.5' : 'translate-x-0.5'}"
      ></span>
    </button>
  </header>
  <div class="px-4 py-3 text-[12px] text-muted">
    {#if !on}
      Browsers get the goggles' own picture. On: the same as HDMI — LUT, Dewarp,
      Stabilisation.
    {:else if st}
      <span class="font-medium text-fg">{PROCESSING_LABEL[st.state]}</span>
      — {st.state === "running"
        ? "this player and the guests see the processed picture."
        : `${st.detail}. Until then, the original.`}
    {/if}
  </div>
</section>

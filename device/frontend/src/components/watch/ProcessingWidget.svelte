<script lang="ts">
  import { onMount } from "svelte";
  import { api, message } from "@/api/client";
  import { settings } from "@/stores/settings.svelte";
  import { snackbar } from "@/stores/snackbar.svelte";
  import { processing, PROCESSING_LABEL } from "@/stores/processing.svelte";
  import { auth } from "@/stores/auth.svelte";
  import Segmented from "@/components/ui/Segmented.svelte";

  const WHERE = [
    { label: "In the browser", value: "browser" },
    { label: "On the Pi", value: "pi" },
  ] as const;

  let busy = $state(false);

  onMount(() => {
    if (!settings.saved) settings.load();
    return processing.watch();
  });

  const on = $derived(settings.saved?.viewer.processed ?? false);
  const where = $derived(settings.saved?.viewer.grade_in ?? "browser");
  const st = $derived(processing.status);

  /** Saved at once: it's what this player shows */
  async function save(
    change: Partial<{ processed: boolean; grade_in: "browser" | "pi" }>,
  ) {
    const viewer = settings.saved?.viewer;
    if (!viewer || busy) return;
    busy = true;
    try {
      settings.replace(
        await api.saveSection("viewer", { ...viewer, ...change }),
      );
      processing.poll();
      // What this player draws itself comes with the access status
      await auth.load();
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
      onclick={() => save({ processed: !on })}
      class="relative inline-flex h-5 w-9 shrink-0 items-center rounded-full transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary disabled:opacity-60
        {on ? 'bg-primary' : 'bg-surface-3'}"
    >
      <span
        class="inline-flex h-4 w-4 rounded-full bg-white shadow-sm transition-transform duration-150
          {on ? 'translate-x-4.5' : 'translate-x-0.5'}"
      ></span>
    </button>
  </header>
  <div class="flex flex-col gap-2.5 px-4 py-3 text-[12px] text-muted">
    {#if !on}
      <p>
        Browsers get the goggles' own picture. On: the HDMI look — LUT and
        Dewarp, plus Stabilisation when made on the Pi.
      </p>
    {:else}
      <Segmented
        options={[...WHERE]}
        value={where}
        disabled={busy}
        onchange={(v) => save({ grade_in: v })}
      />
      {#if where === "browser"}
        <p>
          Each browser grades the goggles' picture with its own graphics: the Pi
          sends it as it is, nothing extra to do. LUT and Dewarp only — no
          stabilisation.
        </p>
      {:else if st}
        <p>
          <span class="font-medium text-fg">{PROCESSING_LABEL[st.state]}</span>
          — {st.state === "running"
            ? "the Pi grades, stabilises and encodes it again for the browsers."
            : `${st.detail}. Until then, the original.`}
        </p>
      {/if}
    {/if}
  </div>
</section>

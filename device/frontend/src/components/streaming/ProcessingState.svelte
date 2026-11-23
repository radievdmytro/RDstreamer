<script lang="ts">
  import { onMount } from "svelte";
  import { processing, PROCESSING_LABEL } from "@/stores/processing.svelte";

  /** Whether this output asks for the processed picture */
  let { on }: { on: boolean } = $props();

  onMount(() => processing.watch());

  const st = $derived(processing.status);
  const dot = $derived(
    !st
      ? "bg-faint"
      : st.state === "running"
        ? "bg-ok"
        : st.state === "failed"
          ? "bg-down"
          : st.state === "starting"
            ? "bg-warn"
            : "bg-faint",
  );
</script>

{#if st && on}
  <p class="flex items-start gap-2 px-4 pb-3 text-[12px] text-muted">
    <span class="mt-1.5 inline-block h-1.5 w-1.5 shrink-0 rounded-full {dot}"
    ></span>
    <span>
      <span class="font-medium text-fg"
        >Processing: {PROCESSING_LABEL[st.state]}</span
      >
      {#if st.state === "running"}
        — this output gets the processed picture ({st.detail.replace(
          /^Encoding \(|\)$/g,
          "",
        )}).
      {:else}
        — {st.detail}. Until it runs, this output gets the original.
      {/if}
    </span>
  </p>
{/if}

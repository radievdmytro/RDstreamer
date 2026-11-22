<script lang="ts">
  import { auth } from "@/stores/auth.svelte";
  import { settings } from "@/stores/settings.svelte";
  import { recording, duration } from "@/stores/recording.svelte";
  import { pageById, WATCH } from "@/lib/nav";
  import type { Component } from "svelte";
  import Menu from "@lucide/svelte/icons/menu";

  let { current, onmenu }: { current: string; onmenu: () => void } = $props();

  const streaming = pageById("streaming");
  const files = pageById("recording");
</script>

<!-- Phones only: the pages used most, and recording under the thumb -->
<nav
  aria-label="Quick"
  class="glass glass-strong fixed inset-x-0 bottom-0 z-30 border-t border-line pb-[env(safe-area-inset-bottom)] md:hidden"
>
  <div class="mx-auto grid h-16 max-w-md grid-cols-5 items-center px-2">
    {@render tab(WATCH.id, "/", WATCH.label, WATCH.icon)}
    {@render tab(streaming.id, "/streaming", streaming.label, streaming.icon)}

    <div class="grid place-items-center">
      {#if auth.admin}
        <button
          type="button"
          onclick={() => recording.toggle()}
          disabled={recording.busy}
          aria-label={recording.active ? "Stop recording" : "Start recording"}
          class="{settings.dirty.length
            ? 'h-10 w-10'
            : '-mt-7 h-14 w-14'} grid place-items-center rounded-full border-4 border-base shadow-lg transition-transform active:scale-95 disabled:opacity-60
            {recording.active
            ? 'rec-ring bg-down'
            : 'bg-surface-3 shadow-[0_0_24px_-6px_var(--color-down)]'}"
        >
          {#if recording.active}
            <span class="h-4 w-4 rounded-[3px] bg-white"></span>
          {:else}
            <span class="h-6 w-6 rounded-full bg-down"></span>
          {/if}
        </button>
        <span
          class="mt-0.5 text-[10px] font-medium tabular-nums {recording.active
            ? 'text-down'
            : 'text-faint'}"
        >
          {recording.active ? duration(recording.elapsed) : "Rec"}
        </span>
      {/if}
    </div>

    {@render tab(files.id, "/recording/files", "Files", files.icon)}

    <button
      type="button"
      onclick={onmenu}
      class="flex flex-col items-center gap-1 py-1 text-[10px] font-medium text-muted"
    >
      <Menu class="h-5 w-5" aria-hidden="true" />
      More
    </button>
  </div>
</nav>

{#snippet tab(id: string, href: string, label: string, Icon: Component)}
  {@const active = current === id}
  <a
    {href}
    aria-current={active ? "page" : undefined}
    class="relative flex flex-col items-center gap-1 py-1 text-[10px] font-medium transition-colors
      {active ? 'text-primary' : 'text-muted'}"
  >
    <Icon class="h-5 w-5" aria-hidden="true" />
    {label}
    {#if active}
      <span
        class="absolute -top-2 h-0.5 w-6 rounded-full bg-primary shadow-[0_0_10px_var(--color-primary)]"
      ></span>
    {/if}
  </a>
{/snippet}

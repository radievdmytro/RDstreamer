<script lang="ts">
  // The page to give guests: the picture and nothing to change. It has no
  // links into the settings; with a password set, nobody without it could
  // change anything from anywhere anyway
  import type { Status } from "@/types/stream";
  import Player from "@/components/watch/Player.svelte";
  import Brand from "@/components/ui/Brand.svelte";
  import { statusDot, statusLabel } from "@/lib/status";

  let status = $state<Status>("connecting");
  let theater = $state(true);
</script>

<svelte:head>
  <title>RDstreamer · Live</title>
</svelte:head>

<div class="flex min-h-dvh flex-col bg-base">
  <header class="flex items-center justify-between gap-3 px-4 py-3 sm:px-6">
    <Brand />
    <span class="flex items-center gap-2 text-[12px] font-medium text-muted">
      <span class="h-2 w-2 rounded-full {statusDot(status)}"></span>
      {statusLabel(status)}
    </span>
  </header>
  <main class="flex flex-1 items-center justify-center px-0 pb-4 sm:px-6">
    <div class="w-full max-w-[min(100%,calc((100dvh-6rem)*16/9))]">
      <Player bind:status bind:theater />
    </div>
  </main>
  <footer class="px-4 pb-3 text-center text-[11px] text-faint sm:px-6">
    Double-click the picture for full screen
  </footer>
</div>

<script lang="ts">
  import type { Snippet } from "svelte";
  import { fade, fly } from "svelte/transition";
  import { control } from "@/stores/control.svelte";
  import { route } from "@/stores/route.svelte";
  import { auth } from "@/stores/auth.svelte";
  import { palette } from "@/stores/palette.svelte";
  import { settings } from "@/stores/settings.svelte";
  import Sidebar from "@/components/layout/Sidebar.svelte";
  import StatusHud from "@/components/layout/StatusHud.svelte";
  import CommandPalette from "@/components/layout/CommandPalette.svelte";
  import BottomBar from "@/components/layout/BottomBar.svelte";
  import Battery from "@/components/ui/Battery.svelte";
  import SignalBars from "@/components/ui/SignalBars.svelte";
  import Brand from "@/components/ui/Brand.svelte";
  import Menu from "@lucide/svelte/icons/menu";
  import Search from "@lucide/svelte/icons/search";

  let {
    current,
    width,
    children,
  }: { current: string; width: string; children: Snippet } = $props();

  let drawerOpen = $state(false);
  const closeDrawer = () => (drawerOpen = false);

  $effect(() => {
    route.path;
    closeDrawer();
  });

  // Each new page slides in. Animated in place rather than rebuilt, so
  // nothing on the page loses its state
  let pageEl = $state<HTMLDivElement>();
  $effect(() => {
    current;
    if (!pageEl || matchMedia("(prefers-reduced-motion: reduce)").matches)
      return;
    pageEl.animate(
      // Opacity only: a transform here would unpin the settings save bar,
      // which is position: fixed inside the page
      [{ opacity: 0 }, { opacity: 1 }],
      { duration: 240, easing: "ease-out" },
    );
  });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") closeDrawer();
  }

  // Ctrl on most keyboards, ⌘ on a Mac
  const mod =
    typeof navigator !== "undefined" &&
    /Mac|iPhone|iPad/.test(navigator.platform)
      ? "⌘"
      : "Ctrl";
</script>

<svelte:window {onkeydown} />

<div class="flex min-h-full">
  <aside
    class="glass sticky top-0 hidden h-screen w-56 shrink-0 border-r border-line md:block"
  >
    <Sidebar {current} onnavigate={closeDrawer} />
  </aside>

  {#if drawerOpen}
    <div class="fixed inset-0 z-40 md:hidden">
      <button
        type="button"
        aria-label="Close menu"
        class="absolute inset-0 bg-black/50 backdrop-blur-[2px]"
        onclick={closeDrawer}
        transition:fade={{ duration: 150 }}
      ></button>
      <aside
        class="glass absolute inset-y-0 left-0 w-64 max-w-[85%] border-r border-line shadow-lg"
        transition:fly={{ x: -256, duration: 180, opacity: 1 }}
      >
        <Sidebar {current} onnavigate={closeDrawer} />
      </aside>
    </div>
  {/if}

  <div class="flex min-w-0 flex-1 flex-col">
    <!-- Phones: menu, name, search, link; the status bar underneath -->
    <div class="glass sticky top-0 z-30 border-b border-line md:hidden">
      <div class="flex h-14 items-center gap-2 px-2">
        <button
          type="button"
          onclick={() => (drawerOpen = true)}
          aria-label="Open menu"
          aria-expanded={drawerOpen}
          class="grid h-10 w-10 place-items-center rounded-md text-muted transition-colors hover:bg-surface-2 hover:text-fg focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-primary"
        >
          <Menu class="h-4 w-4" aria-hidden="true" />
        </button>
        <Brand small />
        <div class="ml-auto flex items-center gap-2">
          <button
            type="button"
            onclick={() => (palette.open = true)}
            aria-label="Search"
            class="grid h-10 w-10 place-items-center rounded-md text-muted transition-colors hover:bg-surface-2 hover:text-fg focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-primary"
          >
            <Search class="h-4 w-4" aria-hidden="true" />
          </button>
          <div class="mr-2 flex items-center gap-3">
            <SignalBars
              quality={control.aircraft ? control.linkQuality : null}
            />
            <Battery percent={control.gogglesBattery} />
          </div>
        </div>
      </div>
      {#if auth.admin}
        <div class="px-3 pb-2">
          <StatusHud />
        </div>
      {/if}
    </div>

    <!-- Larger screens: search on the left, the status bar on the right -->
    <div
      class="glass sticky top-0 z-30 hidden h-12 items-center gap-4 border-b border-line px-8 md:flex"
    >
      <button
        type="button"
        onclick={() => (palette.open = true)}
        class="flex w-56 shrink-0 items-center gap-2 rounded-lg border border-line bg-surface-2/50 px-3 py-1.5 text-[12px] text-faint transition-colors hover:border-line-strong hover:text-muted"
      >
        <Search class="h-3.5 w-3.5" aria-hidden="true" />
        Search…
        <kbd
          class="ml-auto rounded border border-line px-1.5 font-mono text-[10px]"
          >{mod} K</kbd
        >
      </button>
      {#if auth.admin}
        <div class="ml-auto min-w-0">
          <StatusHud />
        </div>
      {/if}
    </div>

    <main
      class="mx-auto w-full px-4 pt-6 sm:px-8 md:pb-6 {width} {settings.dirty
        .length
        ? 'pb-[calc(10rem+env(safe-area-inset-bottom))]'
        : 'pb-[calc(6rem+env(safe-area-inset-bottom))]'}"
    >
      <div bind:this={pageEl}>
        {@render children()}
      </div>
    </main>
  </div>
</div>

<BottomBar {current} onmenu={() => (drawerOpen = true)} />
<CommandPalette />

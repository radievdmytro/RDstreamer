<script lang="ts">
  import { tick, type Component } from "svelte";
  import { fade, scale } from "svelte/transition";
  import { palette } from "@/stores/palette.svelte";
  import { route } from "@/stores/route.svelte";
  import { auth } from "@/stores/auth.svelte";
  import { developer } from "@/stores/developer.svelte";
  import { settings } from "@/stores/settings.svelte";
  import { recording } from "@/stores/recording.svelte";
  import { theme } from "@/stores/theme.svelte";
  import { WATCH, SETTINGS_PAGES } from "@/lib/nav";
  import Search from "@lucide/svelte/icons/search";
  import CornerDownLeft from "@lucide/svelte/icons/corner-down-left";
  import Circle from "@lucide/svelte/icons/circle-dot";
  import Sun from "@lucide/svelte/icons/sun";
  import Moon from "@lucide/svelte/icons/moon";
  import Monitor from "@lucide/svelte/icons/monitor";

  interface Entry {
    id: string;
    label: string;
    /** Where it sits, shown faintly after the label */
    group: string;
    icon: Component;
    /** More words it answers to */
    words?: string;
    run: () => void;
  }

  let query = $state("");
  let selected = $state(0);
  let input = $state<HTMLInputElement>();
  let list = $state<HTMLUListElement>();

  const terminalUsable = $derived(
    developer.enabled &&
      auth.admin &&
      auth.enabled &&
      !!settings.saved?.advanced.terminal,
  );

  const entries = $derived.by(() => {
    const out: Entry[] = [];
    const go = (path: string) => () => route.go(path);

    out.push({
      id: "watch",
      label: WATCH.label,
      group: "Page",
      icon: WATCH.icon,
      words: "live video player",
      run: go("/"),
    });
    for (const p of SETTINGS_PAGES) {
      if (
        p.developer &&
        !(p.id === "terminal" ? terminalUsable : developer.enabled)
      )
        continue;
      out.push({
        id: p.id,
        label: p.label,
        group: "Page",
        icon: p.icon,
        words: p.sections.join(" "),
        run: go(`/${p.id}`),
      });
      for (const t of p.tabs ?? []) {
        if (t.developer && !developer.enabled) continue;
        out.push({
          id: `${p.id}/${t.id}`,
          label: t.label,
          group: p.label,
          icon: p.icon,
          words: `${p.label} ${t.sections.join(" ")}`,
          run: go(`/${p.id}/${t.id}`),
        });
      }
    }

    if (auth.admin) {
      out.push({
        id: "record",
        label: recording.active ? "Stop recording" : "Start recording",
        group: "Action · Shift+R",
        icon: Circle,
        words: "record rec",
        run: () => recording.toggle(),
      });
    }
    out.push(
      {
        id: "theme-dark",
        label: "Dark theme",
        group: "Action",
        icon: Moon,
        words: "appearance colour",
        run: () => theme.set("dark"),
      },
      {
        id: "theme-light",
        label: "Light theme",
        group: "Action",
        icon: Sun,
        words: "appearance colour",
        run: () => theme.set("light"),
      },
      {
        id: "theme-auto",
        label: "Theme like the system",
        group: "Action",
        icon: Monitor,
        words: "appearance colour auto",
        run: () => theme.set("auto"),
      },
    );
    return out;
  });

  /** Higher is better; null when it doesn't match at all */
  function score(e: Entry, q: string): number | null {
    if (!q) return 0;
    const label = e.label.toLowerCase();
    const all = `${label} ${e.group.toLowerCase()} ${e.words ?? ""}`;
    if (label.startsWith(q)) return 4;
    if (label.includes(q)) return 3;
    if (all.includes(q)) return 2;
    // The letters in order, as in "rmp" for RTMP
    let i = 0;
    for (const ch of all) if (ch === q[i]) i++;
    return i === q.length ? 1 : null;
  }

  const results = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return entries
      .map((e, order) => ({ e, s: score(e, q), order }))
      .filter((r) => r.s !== null)
      .sort((a, b) => b.s! - a.s! || a.order - b.order)
      .map((r) => r.e);
  });

  $effect(() => {
    query;
    selected = 0;
  });

  $effect(() => {
    if (palette.open) {
      opener = document.activeElement as HTMLElement | null;
      query = "";
      tick().then(() => input?.focus());
    } else if (opener) {
      opener.focus?.();
      opener = null;
    }
  });

  function choose(e: Entry | undefined) {
    if (!e) return;
    palette.open = false;
    e.run();
  }

  function move(by: number) {
    if (!results.length) return;
    selected = (selected + by + results.length) % results.length;
    tick().then(() =>
      list
        ?.querySelector(`[data-index="${selected}"]`)
        ?.scrollIntoView({ block: "nearest" }),
    );
  }

  function typing(t: EventTarget | null) {
    const el = t as HTMLElement | null;
    return (
      !!el &&
      (el.isContentEditable ||
        ["INPUT", "TEXTAREA", "SELECT"].includes(el.tagName))
    );
  }

  /** Where the opener had focus, to give it back on close */
  let opener: HTMLElement | null = null;

  function onkeydown(e: KeyboardEvent) {
    // The terminal needs its keys (Ctrl+K is kill-line in a shell), and a
    // dialog that's open owns the keyboard
    if ((e.target as HTMLElement | null)?.closest?.(".xterm, dialog")) return;
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
      e.preventDefault();
      palette.toggle();
      return;
    }
    if (
      !palette.open &&
      e.key === "/" &&
      !e.altKey &&
      !e.ctrlKey &&
      !e.metaKey &&
      !e.isComposing &&
      !typing(e.target)
    ) {
      e.preventDefault();
      palette.open = true;
      return;
    }
    if (!palette.open) return;
    if (e.key === "Escape") {
      e.preventDefault();
      palette.open = false;
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      move(1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      move(-1);
    } else if (e.key === "Enter") {
      e.preventDefault();
      choose(results[selected]);
    } else if (e.key === "Tab") {
      // Stays in the search box; the list is reached with the arrows
      e.preventDefault();
      input?.focus();
    }
  }
</script>

<svelte:window {onkeydown} />

{#if palette.open}
  <div
    class="fixed inset-0 z-50 flex items-start justify-center px-4 pt-[12vh]"
  >
    <button
      type="button"
      aria-label="Close"
      tabindex="-1"
      class="absolute inset-0 bg-black/55 backdrop-blur-[2px]"
      onclick={() => (palette.open = false)}
      transition:fade={{ duration: 120 }}
    ></button>

    <div
      role="dialog"
      aria-modal="true"
      aria-label="Go to"
      class="glass relative w-full max-w-lg overflow-hidden rounded-2xl border border-line-strong shadow-[0_30px_80px_-20px_rgb(0_0_0/0.7),0_0_0_1px_color-mix(in_srgb,var(--color-primary)_6%,transparent)]"
      transition:scale={{ start: 0.97, duration: 140 }}
    >
      <div class="flex items-center gap-3 border-b border-line px-4">
        <Search class="h-4 w-4 shrink-0 text-primary" aria-hidden="true" />
        <input
          bind:this={input}
          bind:value={query}
          placeholder="Go to a page or do something…"
          aria-label="Search"
          role="combobox"
          aria-expanded="true"
          aria-controls="palette-results"
          aria-activedescendant={results.length
            ? `palette-${selected}`
            : undefined}
          autocomplete="off"
          spellcheck="false"
          class="h-12 w-full bg-transparent text-[14px] text-fg outline-none placeholder:text-faint"
        />
        <kbd
          class="rounded border border-line px-1.5 py-0.5 font-mono text-[10px] text-faint"
          >Esc</kbd
        >
      </div>

      <ul
        bind:this={list}
        class="max-h-[50vh] overflow-y-auto p-1.5"
        role="listbox"
        id="palette-results"
      >
        {#each results as e, i (e.id)}
          <li
            role="option"
            id="palette-{i}"
            aria-selected={i === selected}
            data-index={i}
          >
            <!-- Mouse only: the keyboard works from the search box -->
            <button
              type="button"
              tabindex="-1"
              onmousemove={() => (selected = i)}
              onclick={() => choose(e)}
              class="flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left text-[13px] transition-colors
                {i === selected ? 'bg-primary/12 text-fg' : 'text-muted'}"
            >
              <e.icon
                class="h-4 w-4 shrink-0 {i === selected ? 'text-primary' : ''}"
                aria-hidden="true"
              />
              <span class="flex-1 truncate">
                <span class="text-fg">{e.label}</span>
                {#if e.group !== "Page"}
                  <span class="ml-1.5 text-[12px] text-faint">{e.group}</span>
                {/if}
              </span>
              {#if i === selected}
                <CornerDownLeft
                  class="h-3.5 w-3.5 text-faint"
                  aria-hidden="true"
                />
              {/if}
            </button>
          </li>
        {:else}
          <li class="px-3 py-6 text-center text-[13px] text-faint">
            Nothing called that
          </li>
        {/each}
      </ul>

      <div
        class="flex items-center gap-4 border-t border-line px-4 py-2 text-[11px] text-faint"
      >
        <span><kbd class="font-mono">↑↓</kbd> choose</span>
        <span><kbd class="font-mono">Enter</kbd> open</span>
        <span class="ml-auto"
          ><kbd class="font-mono">Ctrl K</kbd> or <kbd class="font-mono">/</kbd> anywhere</span
        >
      </div>
    </div>
  </div>
{/if}

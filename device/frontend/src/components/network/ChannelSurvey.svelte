<script lang="ts">
  import { api, message } from "@/api/client";
  import type { WifiSurvey } from "@/types/network";
  import { signalQuality } from "@/stores/wifi.svelte";
  import Card from "@/components/ui/Card.svelte";
  import { btn } from "@/components/ui/buttons";
  import Radar from "@lucide/svelte/icons/radar";

  let {
    channel = $bindable(),
    apOn,
  }: {
    /** The access point's channel in the settings being edited */
    channel: number;
    apOn: boolean;
  } = $props();

  let survey = $state<WifiSurvey | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let showAll = $state(false);

  async function scan() {
    busy = true;
    error = null;
    try {
      survey = await api.wifiSurvey();
    } catch (e) {
      error = message(e);
    } finally {
      busy = false;
    }
  }

  // Bars scale to the busiest channel, but never so far that one faint
  // network fills the chart
  const top = $derived(
    Math.max(1.5, ...(survey?.channels.map((c) => c.load) ?? [0])),
  );
  const loadOf = (c: number) =>
    survey?.channels.find((l) => l.channel === c)?.load ?? 0;

  function verdict(load: number) {
    if (load < 0.3) return { word: "clear", tone: "bg-ok" };
    if (load < 1) return { word: "some traffic", tone: "bg-warn" };
    return { word: "busy", tone: "bg-down" };
  }

  // The radio's channel now; the one being edited if it isn't saved yet
  const live = $derived(survey?.current ?? null);
  const shownNetworks = $derived(
    showAll ? (survey?.networks ?? []) : (survey?.networks.slice(0, 8) ?? []),
  );
</script>

<Card
  title="Channel scanner"
  description="Which 2.4 GHz channels the networks around use, and how busy each one is."
>
  {#snippet aside()}
    <button class={btn.plain} onclick={scan} disabled={busy}>
      <Radar class="h-4 w-4 {busy ? 'animate-spin' : ''}" aria-hidden="true" />
      {busy ? "Scanning…" : survey ? "Scan again" : "Scan"}
    </button>
  {/snippet}

  {#if error}
    <p class="px-4 py-3 text-[13px] text-down">{error}</p>
  {:else if !survey}
    <p class="px-4 py-3 text-[13px] text-muted">
      Takes a few seconds. Devices on the access point may stutter for a moment
      while the radio listens around.
    </p>
  {:else}
    <div class="px-4 pt-4 pb-3">
      <!-- One bar per channel: how much of the air other networks use there -->
      <div
        class="flex h-36 items-end gap-1.5"
        role="img"
        aria-label="Load per channel"
      >
        {#each survey.channels as c (c.channel)}
          {@const v = verdict(c.load)}
          {@const mine = c.channel === live}
          {@const best = c.channel === survey.best}
          <div
            class="flex h-full min-w-0 flex-1 flex-col items-center justify-end gap-1"
          >
            <span class="text-[10px] text-faint tabular-nums">
              {c.networks || ""}
            </span>
            <div
              class="w-full rounded-t-md {v.tone} transition-all
                {mine
                ? 'ring-2 ring-primary ring-offset-2 ring-offset-surface'
                : ''}
                {best && !mine
                ? 'outline-2 outline-offset-2 outline-ok outline-dashed'
                : ''}"
              style="height: {Math.max(
                3,
                (c.load / top) * 100,
              )}%; opacity: {c.channel > 11 ? 0.35 : 0.85}"
              title="Channel {c.channel}: {v.word}, {c.networks} network(s) right on it"
            ></div>
            <span
              class="text-[11px] tabular-nums {mine
                ? 'font-semibold text-primary'
                : best
                  ? 'font-semibold text-ok'
                  : [1, 6, 11].includes(c.channel)
                    ? 'text-fg'
                    : 'text-faint'}">{c.channel}</span
            >
          </div>
        {/each}
      </div>
      <p class="mt-2 text-[11px] text-faint">
        The number above a bar: networks right on that channel. Neighbouring
        channels count too, as 2.4 GHz networks spill two channels either side.
        1, 6 and 11 don't overlap each other. 12 and 13 are greyed out:
        RDstreamer uses 1–11.
      </p>
    </div>

    <div
      class="flex flex-wrap items-center justify-between gap-3 px-4 py-3 text-[13px]"
    >
      <p class="text-fg">
        {#if live === survey.best}
          Channel <b>{survey.best}</b> is already the clearest of 1, 6 and 11.
        {:else}
          Clearest: channel <b class="text-ok">{survey.best}</b>
          ({verdict(loadOf(survey.best)).word}){#if live}, now on
            <b class="text-primary">{live}</b> ({verdict(loadOf(live))
              .word}){/if}.
        {/if}
      </p>
      {#if apOn && channel !== survey.best}
        <button class={btn.primary} onclick={() => (channel = survey!.best)}>
          Use channel {survey.best}
        </button>
      {:else if apOn && channel === survey.best && live !== survey.best}
        <span class="text-[12px] text-muted">Save to switch to it.</span>
      {:else if !apOn}
        <span class="text-[12px] text-muted"
          >Joined networks: the router picks the channel.</span
        >
      {/if}
    </div>

    <div class="px-4 py-3">
      <div class="mb-2 flex items-baseline justify-between">
        <span class="text-[11px] tracking-wide text-faint uppercase"
          >{survey.networks.length} networks on 2.4 GHz</span
        >
        {#if survey.networks_5ghz}
          <span class="text-[11px] text-faint"
            >+ {survey.networks_5ghz} on 5 GHz, not used</span
          >
        {/if}
      </div>
      <ul class="flex flex-col gap-1">
        {#each shownNetworks as n (n.bssid)}
          {@const q = signalQuality(n.signal_dbm)}
          <li class="flex items-center gap-3 text-[13px]">
            <span class="w-7 text-right text-[12px] text-faint tabular-nums"
              >{n.channel}</span
            >
            <span
              class="min-w-0 flex-1 truncate {n.ssid
                ? 'text-fg'
                : 'text-faint italic'}">{n.ssid || "hidden network"}</span
            >
            <span class="flex items-end gap-0.5" aria-hidden="true">
              {#each [1, 2, 3, 4] as b}
                <span
                  class="w-1 rounded-sm {b <= q.bars
                    ? q.tone.replace('text-', 'bg-')
                    : 'bg-line'}"
                  style="height: {4 + b * 3}px"
                ></span>
              {/each}
            </span>
            <span
              class="w-16 text-right font-mono text-[12px] text-muted tabular-nums"
              >{n.signal_dbm} dBm</span
            >
          </li>
        {/each}
      </ul>
      {#if survey.networks.length > 8}
        <button
          class="mt-2 text-[12px] text-primary hover:underline"
          onclick={() => (showAll = !showAll)}
        >
          {showAll ? "Show fewer" : `Show all ${survey.networks.length}`}
        </button>
      {/if}
    </div>
  {/if}
</Card>

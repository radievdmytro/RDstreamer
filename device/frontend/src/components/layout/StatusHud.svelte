<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { metrics } from "@/stores/metrics.svelte";
  import { fan } from "@/stores/fan.svelte";
  import { wifi, signalQuality } from "@/stores/wifi.svelte";
  import { auth } from "@/stores/auth.svelte";
  import { api } from "@/api/client";
  import type { UpdateStatus } from "@/types/system";
  import Download from "@lucide/svelte/icons/download";
  import { recording, duration } from "@/stores/recording.svelte";
  import Thermometer from "@lucide/svelte/icons/thermometer";
  import FanIcon from "@lucide/svelte/icons/fan";
  import Cpu from "@lucide/svelte/icons/cpu";
  import ArrowDownToLine from "@lucide/svelte/icons/arrow-down-to-line";
  import Zap from "@lucide/svelte/icons/zap";
  import Router from "@lucide/svelte/icons/router";
  import Wifi from "@lucide/svelte/icons/wifi";
  import WifiOff from "@lucide/svelte/icons/wifi-off";
  import Cable from "@lucide/svelte/icons/cable";
  import Smartphone from "@lucide/svelte/icons/smartphone";

  onMount(() => {
    metrics.start();
    fan.start();
    wifi.start();
    checkUpdate();
    updateTimer = setInterval(checkUpdate, 2 * 60 * 1000);
  });
  onDestroy(() => {
    metrics.stop();
    fan.stop();
    wifi.stop();
    if (updateTimer) clearInterval(updateTimer);
  });

  // A newer version downloaded and waiting: worth a glance from any page
  let update = $state<UpdateStatus | null>(null);
  let updateTimer: ReturnType<typeof setInterval> | null = null;
  async function checkUpdate() {
    if (!auth.admin) return;
    try {
      update = await api.update();
    } catch {
      // Not shown
    }
  }
  // What runs, and whether a newer one is waiting: downloaded and checked,
  // or found and not downloaded yet (checking automatically is off)
  const version = $derived.by(() => {
    if (!update) return null;
    if (update.installing)
      return {
        label: `Installing ${update.staged ?? "update"}…`,
        title: "Installing the update: video stops for a few seconds",
        news: true,
      };
    if (update.staged && update.staged !== update.current)
      return {
        label: `Update ${update.staged} ready`,
        title: `RDstreamer ${update.staged} is downloaded and checked. Install it from System → Updates (running ${update.current})`,
        news: true,
      };
    if (update.available?.newer)
      return {
        label: `${update.available.version} available`,
        title: `RDstreamer ${update.available.version} is out: download and install it from System → Updates (running ${update.current})`,
        news: true,
      };
    return {
      label: `v${update.current}`,
      title: `RDstreamer ${update.current}: the latest as far as the last check knows`,
      news: false,
    };
  });

  const temp = $derived(metrics.latest("temp"));
  const cpu = $derived(metrics.latest("cpu"));
  const kbps = $derived(metrics.latest("kbps_in"));
  const speed = $derived(fan.status?.speed ?? null);

  const tempTone = $derived(
    temp === null
      ? "text-muted"
      : temp >= 75
        ? "text-down"
        : temp >= 62
          ? "text-warn"
          : "text-fg",
  );

  // The firmware's flags: bits 0-3 now, 16-19 at some point since boot
  const throttled = $derived(metrics.latest("throttled"));
  const undervoltage = $derived(metrics.latest("undervoltage"));
  const power = $derived.by(() => {
    if (throttled === null && undervoltage === null) return null;
    const bits = throttled ?? 0;
    if (bits & 0x1 || undervoltage)
      return {
        tone: "border-down/50 bg-down/15 text-down",
        label: "Low power",
        title:
          "The power supply can't keep up right now: the Pi slows down and video may stutter. Use a 5 V / 3 A supply and a short, thick cable.",
      };
    if (bits & 0x6)
      return {
        tone: "border-down/50 bg-down/15 text-down",
        label: "Throttled",
        title:
          "The processor is slowed down right now, to stay cool or within power.",
      };
    if (bits & 0x8)
      return {
        tone: "text-warn",
        label: "Hot",
        title:
          "Close to the temperature limit: the fan or airflow can't keep up.",
      };
    if (bits & 0x10000)
      return {
        tone: "text-warn",
        label: "Low power earlier",
        title:
          "The voltage dropped at some point since the Pi started. Fine now, but the supply or cable is marginal.",
      };
    if (bits & 0xe0000)
      return {
        tone: "text-warn",
        label: "Throttled earlier",
        title:
          "The processor was slowed down at some point since the Pi started.",
      };
    return {
      tone: "text-fg",
      label: "Power OK",
      title: "Voltage fine, no slowing down since the Pi started",
    };
  });

  const link = $derived(wifi.link);
  const weakest = $derived(
    link?.stations.reduce<number | null>(
      (min, s) =>
        s.signal_dbm === null
          ? min
          : min === null
            ? s.signal_dbm
            : Math.min(min, s.signal_dbm),
      null,
    ) ?? null,
  );
  const radio = $derived.by(() => {
    if (!link) return null;
    if (link.role === "ap") {
      const n = link.stations.length;
      const q = signalQuality(weakest);
      const who = link.stations
        .map(
          (s) =>
            `${s.name ?? s.ip ?? s.mac}${s.signal_dbm === null ? "" : ` ${s.signal_dbm} dBm`}`,
        )
        .join(", ");
      return {
        icon: Router,
        tone: n ? q.tone : "text-fg",
        label: `AP · ${n} ${n === 1 ? "device" : "devices"}`,
        title: `Broadcasting “${link.ssid ?? "RDstreamer"}” on channel ${link.channel ?? "?"}${who ? `. Connected: ${who}` : ". Nobody connected"}`,
      };
    }
    if (link.role === "client") {
      const q = signalQuality(link.signal_dbm);
      return {
        icon: Wifi,
        tone: q.tone,
        label: `${link.ssid ?? "WiFi"}${link.signal_dbm === null ? "" : ` · ${link.signal_dbm} dBm`}`,
        title: `Joined “${link.ssid}” on channel ${link.channel ?? "?"}: signal ${q.word}${link.bitrate_mbps ? `, link ${link.bitrate_mbps} Mbit/s` : ""}`,
      };
    }
    return {
      icon: WifiOff,
      tone: "text-muted",
      label: "WiFi off",
      title: "WiFi isn't connected or broadcasting",
    };
  });
  // Where streams to the internet leave, when it isn't the WiFi above
  const uplink = $derived.by(() => {
    const dev = link?.uplink;
    if (!dev || dev === "wlan0") return null;
    if (dev.startsWith("wlan") || dev.startsWith("wlx"))
      return { icon: Wifi, label: "WiFi 2" };
    if (link.uplink_phone) return { icon: Smartphone, label: "Phone" };
    return { icon: Cable, label: "Ethernet" };
  });

  const rate = (k: number) =>
    k >= 1000 ? `${(k / 1000).toFixed(1)} Mbit/s` : `${Math.round(k)} kbit/s`;

  const chip =
    "flex shrink-0 items-center gap-1.5 rounded-full border border-line bg-surface-2/60 px-2.5 py-1 text-[12px] font-medium tabular-nums transition-colors hover:border-line-strong hover:bg-surface-2";
</script>

<div
  class="flex min-w-0 [scrollbar-width:none] items-center gap-1.5 overflow-x-auto"
  aria-label="Device status"
>
  {#if recording.active}
    <a
      href="/recording/files"
      class="{chip} border-down/50 bg-down/15 text-down hover:border-down hover:bg-down/20"
      title="Recording"
    >
      <span class="rec-ring h-2 w-2 rounded-full bg-down"></span>
      REC {duration(recording.elapsed)}
    </a>
  {/if}

  {#if version}
    <a
      href="/system/updates"
      class="{chip} {version.news
        ? 'border-primary/50 bg-primary/15 text-primary hover:border-primary hover:bg-primary/20'
        : 'text-muted'}"
      title={version.title}
    >
      {#if version.news}
        <Download class="h-3.5 w-3.5" aria-hidden="true" />
      {/if}
      {version.label}
    </a>
  {/if}

  {#if temp !== null}
    <a
      href="/performance"
      class="{chip} {tempTone}"
      title="Processor temperature"
    >
      <Thermometer class="h-3.5 w-3.5 opacity-70" aria-hidden="true" />
      {temp.toFixed(1)}°C
    </a>
  {/if}

  {#if fan.status?.control === "driving"}
    <a
      href="/system/general"
      class="{chip} text-fg"
      title="Cooling fan: what the curve asks of it now"
    >
      <FanIcon
        class="h-3.5 w-3.5 {speed ? 'animate-spin text-primary' : 'opacity-70'}"
        style={speed
          ? `animation-duration: ${Math.max(0.4, 2.4 - speed / 50)}s`
          : undefined}
        aria-hidden="true"
      />
      {speed ? `${speed}%` : "off"}
    </a>
  {/if}

  {#if power}
    <a href="/performance" class="{chip} {power.tone}" title={power.title}>
      <Zap class="h-3.5 w-3.5 opacity-70" aria-hidden="true" />
      {power.label}
    </a>
  {/if}

  {#if radio}
    {@const Icon = radio.icon}
    <a href="/network" class="{chip} {radio.tone}" title={radio.title}>
      <Icon class="h-3.5 w-3.5 opacity-80" aria-hidden="true" />
      <span class="max-w-[11rem] truncate">{radio.label}</span>
    </a>
  {/if}

  {#if uplink}
    {@const Icon = uplink.icon}
    <a
      href="/network"
      class="{chip} text-fg"
      title="Streams to the internet go out this way"
    >
      <Icon class="h-3.5 w-3.5 opacity-70" aria-hidden="true" />
      {uplink.label}
    </a>
  {/if}

  {#if cpu !== null}
    <a href="/performance" class="{chip} text-fg" title="Processor load">
      <Cpu class="h-3.5 w-3.5 opacity-70" aria-hidden="true" />
      {Math.round(cpu)}%
    </a>
  {/if}

  {#if kbps}
    <a href="/performance" class="{chip} text-fg" title="Incoming video">
      <ArrowDownToLine class="h-3.5 w-3.5 opacity-70" aria-hidden="true" />
      {rate(kbps)}
    </a>
  {/if}
</div>

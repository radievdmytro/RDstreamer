<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { api, message } from "@/api/client";
  import type { FanConfig, FanMode, FanPoint, FanStatus } from "@/types/system";
  import { snackbar } from "@/stores/snackbar.svelte";
  import Card from "@/components/ui/Card.svelte";
  import Field from "@/components/ui/Field.svelte";
  import Switch from "@/components/ui/Switch.svelte";
  import Segmented from "@/components/ui/Segmented.svelte";
  import ConfirmDialog from "@/components/ui/ConfirmDialog.svelte";
  import { btn } from "@/components/ui/buttons";

  const MODES: { label: string; value: FanMode }[] = [
    { label: "Auto", value: "auto" },
    { label: "On", value: "on" },
    { label: "Off", value: "off" },
  ];

  let status = $state<FanStatus | null>(null);
  /** The curve being edited; null while it just follows the device */
  let draft = $state<FanPoint[] | null>(null);
  let saving = $state(false);
  let takingOver = $state(false);
  let confirmTakeOver = $state(false);
  let timer: ReturnType<typeof setInterval> | null = null;

  async function refresh() {
    try {
      status = await api.fan();
    } catch {
      // Keep the last reading; with none the card doesn't show
    }
  }

  onMount(() => {
    refresh();
    timer = setInterval(refresh, 3000);
  });
  onDestroy(() => {
    if (timer) clearInterval(timer);
  });

  // Shown on a device with the case, or where fan control was switched off
  const shown = $derived(
    !!status && (status.control !== "no_case" || status.config.mode !== "auto"),
  );
  const driving = $derived(status?.control === "driving");
  const points = $derived(draft ?? status?.config.curve ?? []);
  const changed = $derived(
    draft !== null &&
      JSON.stringify(sorted(draft)) !==
        JSON.stringify(status?.config.curve ?? []),
  );

  function sorted(list: FanPoint[]) {
    return [...list].sort((a, b) => a.temp_c - b.temp_c);
  }

  async function save(change: Partial<FanConfig>, done?: string) {
    if (!status || saving) return;
    saving = true;
    try {
      status = await api.setFan({ ...status.config, ...change });
      if (change.curve) draft = null;
      if (done) snackbar.show(done);
    } catch (e) {
      snackbar.show(message(e), true);
      await refresh();
    } finally {
      saving = false;
    }
  }

  async function takeOver() {
    takingOver = true;
    try {
      status = await api.fanTakeOver();
      snackbar.show("RDstreamer runs the fan now; argononed is off");
    } catch (e) {
      snackbar.show(message(e), true);
    } finally {
      takingOver = false;
    }
  }

  function edit(change: (list: FanPoint[]) => void) {
    const list = points.map((p) => ({ ...p }));
    change(list);
    draft = list;
  }

  function setValue(i: number, key: keyof FanPoint, value: string) {
    const n = Math.round(Number(value));
    edit((list) => (list[i][key] = Number.isFinite(n) ? n : 0));
  }

  function addPoint() {
    if (!status) return;
    const { max_c } = status.limits;
    edit((list) => {
      const last = sorted(list).at(-1) ?? { temp_c: 40, speed: 0 };
      list.push({
        temp_c: Math.min(last.temp_c + 5, max_c),
        speed: Math.min(last.speed + 20, 100),
      });
    });
  }

  /** The same rules the device applies, to say what's wrong before saving */
  const problem = $derived.by(() => {
    if (!status || draft === null) return null;
    const { max_points, min_c, max_c, full_by_c } = status.limits;
    const list = sorted(draft);
    if (!list.length || list.length > max_points)
      return `Use 1 to ${max_points} points`;
    for (const p of list) {
      if (p.temp_c < min_c || p.temp_c > max_c)
        return `${p.temp_c} °C is outside ${min_c}–${max_c} °C`;
      if (p.speed < 0 || p.speed > 100) return `${p.speed}% is outside 0–100%`;
    }
    for (let i = 1; i < list.length; i++) {
      if (list[i].temp_c === list[i - 1].temp_c)
        return "Two points have the same temperature";
      if (list[i].speed < list[i - 1].speed)
        return "The fan can't get slower as it gets hotter";
    }
    if (!list.some((p) => p.temp_c <= full_by_c && p.speed === 100))
      return `The fan has to reach 100% by ${full_by_c} °C`;
    return null;
  });

  // The curve as a step line, with the temperature now
  const W = 320;
  const H = 120;
  const clampT = (t: number) =>
    status
      ? Math.min(Math.max(t, status.limits.min_c), status.limits.max_c)
      : t;
  const x = (t: number) =>
    status
      ? ((clampT(t) - status.limits.min_c) /
          (status.limits.max_c - status.limits.min_c)) *
        W
      : 0;
  const y = (s: number) => H - (Math.min(Math.max(s, 0), 100) / 100) * H;
  const path = $derived.by(() => {
    let d = `M0,${y(0)}`;
    for (const p of sorted(points)) d += ` H${x(p.temp_c)} V${y(p.speed)}`;
    return d + ` H${W}`;
  });
</script>

{#if status && shown}
  <Card
    title="Cooling fan"
    description="The Argon ONE case: fan, power button and power cut, run by RDstreamer"
  >
    {#if status.control === "argononed"}
      <div
        class="flex flex-wrap items-center justify-between gap-3 bg-primary/10 px-4 py-3 text-[12px] text-fg"
      >
        <span>
          Argon40's own service (argononed) runs the case now, so RDstreamer
          leaves it alone. Switch it off to run everything from here.
        </span>
        <button
          type="button"
          class={btn.primary}
          disabled={takingOver}
          onclick={() => (confirmTakeOver = true)}
        >
          {takingOver ? "Switching…" : "Run it from RDstreamer"}
        </button>
      </div>
    {:else if status.control === "hat"}
      <p class="bg-warn/10 px-4 py-3 text-[12px] text-fg">
        A HAT is fitted, and it may be what answers at the case's address, so
        Auto leaves it alone. If this is an Argon ONE case, choose On.
      </p>
    {:else if status.control === "no_case" && status.config.mode === "on"}
      <p class="bg-warn/10 px-4 py-3 text-[12px] text-fg">
        Nothing answers at the case's address (0x1a on I2C bus 1). Check that
        I2C is on, and reboot after the first install.
      </p>
    {/if}
    {#if status.error}
      <p class="bg-down/10 px-4 py-3 text-[12px] text-down">{status.error}</p>
    {/if}

    <Field
      label="Now"
      help={driving
        ? "What RDstreamer sends the fan. The case can't report its speed back."
        : undefined}
    >
      <span class="text-[13px] text-fg tabular-nums">
        {status.temp_c === null ? "—" : `${status.temp_c.toFixed(1)} °C`}
        ·
        {!driving || status.speed === null
          ? "fan not run from here"
          : status.speed === 0
            ? "fan off"
            : `fan ${status.speed}%`}
      </span>
    </Field>

    <Field
      label="Fan control"
      help="Auto runs the case when it's found and no HAT is fitted. Off leaves it alone."
    >
      <div class="w-full sm:w-64">
        <Segmented
          options={MODES}
          value={status.config.mode}
          onchange={(mode) => save({ mode })}
        />
      </div>
    </Field>

    <div class="px-4 py-3">
      <svg
        viewBox="-2 -4 {W + 4} {H + 8}"
        preserveAspectRatio="none"
        class="h-32 w-full text-primary"
        role="img"
        aria-label="Fan speed by temperature"
      >
        {#each [0, 50, 100] as s}
          <line
            x1="0"
            x2={W}
            y1={y(s)}
            y2={y(s)}
            class="stroke-line"
            stroke-width="1"
            vector-effect="non-scaling-stroke"
          />
        {/each}
        <path
          d={path}
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          vector-effect="non-scaling-stroke"
        />
        {#if status.temp_c !== null}
          <line
            x1={x(status.temp_c)}
            x2={x(status.temp_c)}
            y1="0"
            y2={H}
            class="stroke-accent"
            stroke-width="1.5"
            stroke-dasharray="4 3"
            vector-effect="non-scaling-stroke"
          />
        {/if}
      </svg>
      <div class="flex justify-between text-[11px] text-faint tabular-nums">
        <span>{status.limits.min_c} °C</span>
        <span>{status.limits.max_c} °C</span>
      </div>
    </div>

    <div class="px-4 py-3">
      <div
        class="grid max-w-md grid-cols-[1fr_1fr_auto] items-center gap-x-3 gap-y-2 text-[13px]"
      >
        <span class="text-[12px] text-muted">From °C</span>
        <span class="text-[12px] text-muted">Fan %</span>
        <span></span>
        {#each points as p, i (i)}
          <input
            type="number"
            min={status.limits.min_c}
            max={status.limits.max_c}
            value={p.temp_c}
            oninput={(e) => setValue(i, "temp_c", e.currentTarget.value)}
            aria-label="Temperature of point {i + 1}"
            class="w-full rounded-md border border-line bg-surface-2 px-3 py-1.5 text-fg tabular-nums outline-none hover:border-line-strong focus:border-primary"
          />
          <input
            type="number"
            min="0"
            max="100"
            value={p.speed}
            oninput={(e) => setValue(i, "speed", e.currentTarget.value)}
            aria-label="Fan speed of point {i + 1}"
            class="w-full rounded-md border border-line bg-surface-2 px-3 py-1.5 text-fg tabular-nums outline-none hover:border-line-strong focus:border-primary"
          />
          <button
            type="button"
            class={btn.text}
            aria-label="Remove point {i + 1}"
            disabled={points.length <= 1}
            onclick={() => edit((list) => list.splice(i, 1))}>✕</button
          >
        {/each}
      </div>
      <p class="mt-2 text-[12px] text-muted">
        Below the first point the fan is off. It speeds up at once and slows
        down only after 30 s, so it doesn't hunt around a threshold.
      </p>
      {#if problem}
        <p class="mt-2 text-[13px] text-down">{problem}</p>
      {/if}
      <div class="mt-3 flex flex-wrap items-center gap-2">
        <button
          type="button"
          class={btn.plain}
          disabled={points.length >= status.limits.max_points}
          onclick={addPoint}>Add point</button
        >
        <button
          type="button"
          class={btn.plain}
          onclick={() =>
            (draft = status
              ? status.limits.quiet.map((p) => ({ ...p }))
              : null)}>Quiet preset</button
        >
        <span class="flex-1"></span>
        {#if draft !== null}
          <button type="button" class={btn.text} onclick={() => (draft = null)}
            >Cancel</button
          >
        {/if}
        <button
          type="button"
          class={btn.primary}
          disabled={!changed || !!problem || saving}
          onclick={() =>
            save({ curve: sorted(draft ?? []) }, "Fan curve saved")}
          >{saving ? "Saving…" : "Save curve"}</button
        >
      </div>
    </div>

    <Field
      label="Power button"
      for="fan-button"
      help={status.config.power_button && driving && !status.power_button
        ? "GPIO4 is in use by another button, so the case's button isn't heard."
        : "Double tap: reboot. Hold 3 s: power off. Recordings are finished first."}
    >
      <Switch
        id="fan-button"
        disabled={saving}
        bind:checked={
          () => status?.config.power_button ?? true,
          (on) => save({ power_button: on })
        }
      />
    </Field>
    <Field
      label="Cut the power after power-off"
      for="fan-cut"
      help="The case switches itself off once the Pi has shut down."
    >
      <Switch
        id="fan-cut"
        disabled={saving}
        bind:checked={
          () => status?.config.power_cut ?? true,
          (on) => save({ power_cut: on })
        }
      />
    </Field>
    <Field
      label="Spin-up kick"
      for="fan-kick"
      help="Full speed for a second before a slow speed, for fans that don't start slowly from standstill."
    >
      <Switch
        id="fan-kick"
        disabled={saving}
        bind:checked={
          () => status?.config.spin_up ?? false, (on) => save({ spin_up: on })
        }
      />
    </Field>
  </Card>

  <ConfirmDialog
    bind:open={confirmTakeOver}
    title="Run the case from RDstreamer?"
    message="argononed is stopped and switched off for good; its fan curve is brought over if it fits. The fan, the power button and the power cut are then run from here."
    confirmLabel="Switch over"
    onconfirm={takeOver}
  />
{/if}

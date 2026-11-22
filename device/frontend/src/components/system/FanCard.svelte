<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { api, message } from "@/api/client";
  import type { FanPoint, FanStatus } from "@/types/system";
  import { snackbar } from "@/stores/snackbar.svelte";
  import Card from "@/components/ui/Card.svelte";
  import Field from "@/components/ui/Field.svelte";
  import { btn } from "@/components/ui/buttons";

  let status = $state<FanStatus | null>(null);
  /** The curve being edited; null while it just follows the device */
  let draft = $state<FanPoint[] | null>(null);
  let saving = $state(false);
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
    timer = setInterval(refresh, 5000);
  });
  onDestroy(() => {
    if (timer) clearInterval(timer);
  });

  const points = $derived(draft ?? status?.curve ?? []);
  const changed = $derived(
    draft !== null &&
      JSON.stringify(sorted(draft)) !== JSON.stringify(status?.curve ?? []),
  );

  function sorted(list: FanPoint[]) {
    return [...list].sort((a, b) => a.temp_c - b.temp_c);
  }

  function edit(change: (list: FanPoint[]) => void) {
    const list = (draft ?? status?.curve ?? []).map((p) => ({ ...p }));
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

  async function save() {
    if (!draft || problem || saving) return;
    saving = true;
    try {
      status = await api.setFan(sorted(draft));
      draft = null;
      snackbar.show("Fan curve saved");
    } catch (e) {
      snackbar.show(message(e), true);
      await refresh();
    } finally {
      saving = false;
    }
  }

  // The curve as a step line, with the temperature now
  const W = 320;
  const H = 120;
  const x = (t: number) =>
    status
      ? ((t - status.limits.min_c) /
          (status.limits.max_c - status.limits.min_c)) *
        W
      : 0;
  const y = (s: number) => H - (s / 100) * H;
  const path = $derived.by(() => {
    if (!status) return "";
    const { min_c, max_c } = status.limits;
    let d = `M0,${y(0)}`;
    for (const p of sorted(points)) {
      const t = Math.min(Math.max(p.temp_c, min_c), max_c);
      d += ` H${x(t)} V${y(Math.min(Math.max(p.speed, 0), 100))}`;
    }
    return d + ` H${W}`;
  });
</script>

{#if status?.available}
  <Card
    title="Cooling fan"
    description="The Argon ONE case's fan, run by its argononed service"
  >
    {#if !status.active}
      <p class="bg-warn/10 px-4 py-3 text-[12px] text-fg">
        The fan service isn't running, so the fan isn't being controlled. The
        Logs page or <span class="font-mono">journalctl -u argononed</span> may say
        why.
      </p>
    {/if}

    <Field
      label="Now"
      help="The fan can't report its speed; this is what the curve asks of it."
    >
      <span class="text-[13px] text-fg tabular-nums">
        {status.temp_c === null ? "—" : `${status.temp_c.toFixed(1)} °C`}
        ·
        {status.speed === null
          ? "—"
          : status.speed === 0
            ? "fan off"
            : `fan ${status.speed}%`}
      </span>
    </Field>

    <div class="px-4 py-3">
      <svg
        viewBox="-2 -4 {W + 4} {H + 8}"
        class="h-32 w-full text-primary"
        preserveAspectRatio="none"
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
            x1={x(
              Math.min(
                Math.max(status.temp_c, status.limits.min_c),
                status.limits.max_c,
              ),
            )}
            x2={x(
              Math.min(
                Math.max(status.temp_c, status.limits.min_c),
                status.limits.max_c,
              ),
            )}
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
        Below the first point the fan is off. Saving restarts the fan service;
        if it doesn't come back up, the previous curve is put back.
      </p>
      {#if problem}
        <p class="mt-2 text-[13px] text-down">{problem}</p>
      {/if}
    </div>

    <div class="flex flex-wrap items-center gap-2 px-4 py-3">
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
          (draft = status ? status.limits.quiet.map((p) => ({ ...p })) : null)}
        >Quiet preset</button
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
        onclick={save}>{saving ? "Saving…" : "Save curve"}</button
      >
    </div>
  </Card>
{/if}

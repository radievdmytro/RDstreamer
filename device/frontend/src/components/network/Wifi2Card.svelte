<script lang="ts">
  import { api, message } from "@/api/client";
  import type { InterfaceStatus, ScanResult } from "@/types/network";
  import type { Settings } from "@/types/settings";
  import { snackbar } from "@/stores/snackbar.svelte";
  import Card from "@/components/ui/Card.svelte";
  import Field from "@/components/ui/Field.svelte";
  import Switch from "@/components/ui/Switch.svelte";
  import TextField from "@/components/ui/TextField.svelte";
  import { btn } from "@/components/ui/buttons";

  let {
    wifi2 = $bindable(),
    adapter,
    error,
  }: {
    wifi2: Settings["wifi2"];
    /** The second adapter as it is now; null while none is plugged in */
    adapter: InterfaceStatus | null;
    error: string | null;
  } = $props();

  let scanning = $state(false);
  let networks = $state<ScanResult[] | null>(null);

  async function scan() {
    scanning = true;
    try {
      networks = await api.scanWifi2();
    } catch (e) {
      snackbar.show(`Scan failed: ${message(e)}`, true);
    } finally {
      scanning = false;
    }
  }

  function pick(n: ScanResult) {
    if (n.ssid !== wifi2.ssid) wifi2.password = "";
    wifi2.ssid = n.ssid;
    networks = null;
    document.getElementById("wifi2-password")?.focus();
  }

  const shown = $derived(
    (networks ?? []).filter((n) => wifi2.allow_5ghz || n.channel <= 14),
  );
  const bars = (signal: number) => (signal >= 70 ? 3 : signal >= 45 ? 2 : 1);

  const now = $derived.by(() => {
    if (!adapter) return null;
    const name = adapter.product
      ? `${adapter.product} (${adapter.device})`
      : adapter.device;
    if (adapter.state === "connected")
      return `${name}: joined “${adapter.ssid ?? adapter.connection}”${adapter.addresses.length ? `, ${adapter.addresses.join(", ")}` : ""}`;
    return `${name}: ${adapter.state}`;
  });
</script>

<Card
  title="Second WiFi adapter"
  description="A USB WiFi adapter joins a network for the internet, while the built-in WiFi keeps broadcasting the access point for watching."
>
  {#if !adapter && !wifi2.enabled}
    <p class="px-4 py-3 text-[13px] text-muted">
      No second adapter plugged in. Plug a USB WiFi adapter into a USB-A port
      (MediaTek chips work without extra drivers: MT7610U, MT7612U, MT7921AU);
      it shows up here within a few seconds.
    </p>
  {:else}
    <div class="px-4 py-3 text-[13px] {adapter ? 'text-fg' : 'text-muted'}">
      {now ??
        "The adapter isn't plugged in. Its settings are kept, and it joins the network once it is."}
    </div>
    {#if error}
      <p class="bg-down/10 px-4 py-3 text-[13px] text-down">{error}</p>
    {/if}

    <Field
      label="Join a network for the internet"
      for="wifi2-on"
      help="Streams to YouTube and the like, and updates, then go out this way; devices on the access point get the internet too. Saving this leaves the access point alone."
    >
      <Switch id="wifi2-on" bind:checked={wifi2.enabled} />
    </Field>

    {#if wifi2.enabled}
      <Field label="Network name" for="wifi2-ssid">
        <TextField id="wifi2-ssid" bind:value={wifi2.ssid} maxlength={32} />
        <button
          type="button"
          class={btn.plain}
          onclick={scan}
          disabled={scanning || !adapter}
        >
          {scanning ? "Scanning…" : "Scan"}
        </button>
      </Field>

      {#if networks}
        <div class="px-4 py-2">
          {#if shown.length === 0}
            <p class="py-1 text-[13px] text-muted">No networks found.</p>
          {:else}
            <ul class="max-h-64 overflow-y-auto rounded-md border border-line">
              {#each shown as n (n.ssid + n.channel)}
                <li class="border-t border-line first:border-t-0">
                  <button
                    type="button"
                    onclick={() => pick(n)}
                    class="flex w-full items-center gap-3 px-3 py-2 text-left transition-colors hover:bg-surface-2"
                  >
                    <svg
                      viewBox="0 0 16 16"
                      class="h-4 w-4 shrink-0"
                      aria-label="Signal {n.signal}%"
                    >
                      {#each [1, 2, 3] as b}
                        <rect
                          x={1 + (b - 1) * 5}
                          y={13 - b * 4}
                          width="3.5"
                          height={b * 4}
                          rx="0.75"
                          class={b <= bars(n.signal)
                            ? "fill-fg"
                            : "fill-surface-3"}
                        />
                      {/each}
                    </svg>
                    <span class="min-w-0 flex-1 truncate text-[13px] text-fg"
                      >{n.ssid}</span
                    >
                    <span class="text-[12px] text-faint">
                      {n.channel > 14 ? "5 GHz" : "2.4 GHz"} · {n.security ||
                        "Open"}
                    </span>
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      {/if}

      <Field
        label="Password"
        for="wifi2-password"
        help="Leave empty for an open network."
      >
        <TextField
          id="wifi2-password"
          secret
          bind:value={wifi2.password}
          maxlength={63}
        />
      </Field>

      <Field
        label="Allow 5 GHz"
        for="wifi2-5ghz"
        help="Faster, but 5 GHz is where the goggles' video link runs: next to the goggles it can cost range and picture. Fine at home, best off in the field."
      >
        <Switch id="wifi2-5ghz" bind:checked={wifi2.allow_5ghz} />
      </Field>
    {/if}
  {/if}
</Card>

<script lang="ts">
  import mark from "@/assets/rd-mark.svg?raw";
  import logo from "@/assets/rd-logo.svg?raw";

  let {
    small = false,
    full = false,
  }: {
    small?: boolean;
    /** The whole logo, large and centred, for the login and startup screens */
    full?: boolean;
  } = $props();
</script>

<!-- The SVGs are inline so their white follows the theme's text colour -->
{#if full}
  <span class="flex flex-col items-center gap-4 text-fg">
    <span
      class="block h-28 drop-shadow-[0_0_30px_color-mix(in_srgb,var(--color-primary)_25%,transparent)] sm:h-32"
    >
      {@html logo}
    </span>
    <span
      class="font-display text-3xl leading-none font-bold tracking-[0.04em]"
    >
      RD<span class="text-primary">streamer</span>
    </span>
  </span>
{:else}
  <span class="flex items-center gap-3 text-fg">
    <span class="block shrink-0 {small ? 'h-7' : 'h-9'}">
      {@html mark}
    </span>
    <span class="flex flex-col gap-y-1 leading-none">
      <span
        class="font-display font-bold tracking-[0.04em] {small
          ? 'text-[18px]'
          : 'text-[22px]'}"
      >
        RD<span class="text-primary">streamer</span>
      </span>
      {#if !small}
        <span
          class="text-[9px] font-medium tracking-[0.12em] whitespace-nowrap text-muted uppercase"
        >
          Radev Video Production
        </span>
      {/if}
    </span>
  </span>
{/if}

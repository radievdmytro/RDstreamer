<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    title: string;
    description?: string;
    aside?: Snippet;
    children: Snippet;
  }

  let { title, description, aside, children }: Props = $props();
</script>

<section class="card overflow-hidden rounded-xl border border-line bg-surface">
  <header
    class="flex items-center justify-between gap-3 border-b border-line bg-surface-2/40 px-4 py-3"
  >
    <div class="min-w-0">
      <h2 class="font-display text-[15px] leading-tight font-semibold text-fg">
        {title}
      </h2>
      {#if description}
        <p class="mt-0.5 text-[12px] text-muted">{description}</p>
      {/if}
    </div>
    {#if aside}
      <div class="shrink-0">{@render aside()}</div>
    {/if}
  </header>
  <div class="divide-y divide-line">
    {@render children()}
  </div>
</section>

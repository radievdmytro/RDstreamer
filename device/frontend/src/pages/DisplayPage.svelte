<script lang="ts">
  import type { Settings } from "@/types/settings";
  import HdmiCard from "@/components/display/HdmiCard.svelte";
  import SplashCard from "@/components/display/SplashCard.svelte";
  import SplashVideoCard from "@/components/display/SplashVideoCard.svelte";
  import OverlayCard from "@/components/display/OverlayCard.svelte";
  import LookCard from "@/components/display/LookCard.svelte";
  import DewarpCard from "@/components/display/DewarpCard.svelte";
  import StabilizationCard from "@/components/display/StabilizationCard.svelte";
  import PreviewCard from "@/components/display/PreviewCard.svelte";
  let {
    display = $bindable(),
    splash = $bindable(),
    overlay = $bindable(),
    tab,
  }: {
    display: Settings["display"];
    splash: Settings["splash"];
    overlay: Settings["overlay"];
    tab: string;
  } = $props();
</script>

<div class="flex flex-col gap-4">
  {#if tab === "hdmi"}
    <HdmiCard bind:display />
  {:else if tab === "splash"}
    <SplashCard bind:splash />
    <SplashVideoCard bind:splash />
  {:else if tab === "logo"}
    <OverlayCard bind:overlay />
  {:else if tab === "image"}
    <PreviewCard {display} />
    <LookCard bind:display />
    <DewarpCard bind:display />
    <StabilizationCard bind:display />
  {/if}
</div>

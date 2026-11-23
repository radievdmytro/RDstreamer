<script lang="ts">
  import Field from "@/components/ui/Field.svelte";
  import Switch from "@/components/ui/Switch.svelte";
  import ProcessingState from "./ProcessingState.svelte";

  let {
    id,
    checked = $bindable(),
    disabled = false,
    browser = false,
    status = true,
  }: {
    id: string;
    checked: boolean;
    disabled?: boolean;
    browser?: boolean;
    /** Whether the Pi's processing is what this output would get */
    status?: boolean;
  } = $props();
</script>

<Field
  label="Processing"
  for={id}
  help={checked && browser
    ? "Each browser applies the LUT and Dewarp itself to the goggles' picture: no extra work for the Pi. No stabilisation."
    : checked
      ? `The same picture as HDMI: LUT, Dewarp and Stabilisation as set on the Display page, encoded again by the Pi (up to 1080p30, ~0.1 s later, plus the look-ahead if it's on).${browser ? " Light quality isn't offered with it: browsers get this one." : ""} If processing can't run, the original goes out.`
      : "The goggles' own picture, untouched (flat, for grading in OBS or elsewhere), at its full frame rate."}
  {disabled}
>
  <Switch {id} bind:checked {disabled} />
</Field>
<ProcessingState on={checked && !disabled && status} />

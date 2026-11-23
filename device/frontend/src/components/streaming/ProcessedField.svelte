<script lang="ts">
  import Field from "@/components/ui/Field.svelte";
  import Switch from "@/components/ui/Switch.svelte";

  let {
    id,
    checked = $bindable(),
    disabled = false,
    browser = false,
  }: {
    id: string;
    checked: boolean;
    disabled?: boolean;
    browser?: boolean;
  } = $props();
</script>

<Field
  label="Image processing"
  for={id}
  help={checked
    ? `The same picture as HDMI: LUT, Dewarp and Stabilisation as set on the Display page, encoded again by the Pi (up to 1080p30, ~0.1 s later, plus the look-ahead if it's on).${browser ? " Light quality isn't offered with it: browsers get this one." : ""} If processing can't run, the original goes out.`
    : "The goggles' own picture, untouched (flat, for grading in OBS or elsewhere), at its full frame rate."}
  {disabled}
>
  <Switch {id} bind:checked {disabled} />
</Field>

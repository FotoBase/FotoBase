<!--
  Caption-mode switcher for the grid toolbar (TASK-03-015). Writes straight
  into the shared grid store, so both renderers re-render instantly; the
  shell persists the choice to the `caption_mode` setting.
-->
<script lang="ts">
  import { CAPTION_MODES, isCaptionMode } from "./captions";
  import { gridState } from "./grid-state.svelte.js";
  import { t } from "./i18n.svelte.js";

  function onchange(e: Event): void {
    const value = (e.currentTarget as HTMLSelectElement).value;
    if (isCaptionMode(value)) gridState.captionMode = value;
  }
</script>

<label for="caption-mode">{t("grid.captions")}</label>
<select
  id="caption-mode"
  data-testid="caption-mode"
  value={gridState.captionMode}
  {onchange}
>
  {#each CAPTION_MODES as mode (mode.value)}
    <option value={mode.value}>{t(mode.labelKey)}</option>
  {/each}
</select>

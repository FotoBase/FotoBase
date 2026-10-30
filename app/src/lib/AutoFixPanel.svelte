<!--
  Basic Fixes — one-click auto fixes (TASK-04-007, PLAN §2E).
  I'm Feeling Lucky = combined auto color + contrast pass; Auto Contrast and
  Auto Color are each their own one-click. The panel only records the active
  passes — the pixel work is done by the shared Rust engine (golden-tested)
  at preview/export resolution. Never touches the original file.
-->
<script lang="ts">
  import { t } from "./i18n.svelte.js";
  import { type AutoFixState } from "./auto-fix";

  interface Props {
    /// `preview://` URI for the image being edited.
    previewSrc: string;
    /// Fired on every state change with the current auto-fix state.
    onchange: (state: AutoFixState) => void;
  }

  let { previewSrc, onchange }: Props = $props();

  let state = $state<AutoFixState>({ autoColor: false, autoContrast: false });

  // The edit view needs the initial (untouched) state to know the photo has
  // no auto-fix ops yet — emit once on mount.
  let announced = false;
  $effect(() => {
    if (announced) return;
    announced = true;
    emit();
  });

  function emit(): void {
    // Report failures instead of failing silently; state stays consistent.
    Promise.resolve()
      .then(() => onchange(state))
      .catch((e) => console.error("auto-fix change rejected", e));
  }

  function lucky(): void {
    state = { autoColor: true, autoContrast: true };
    emit();
  }

  function toggle(key: "autoColor" | "autoContrast"): void {
    state = { ...state, [key]: !state[key] };
    emit();
  }

  function clear(): void {
    state = { autoColor: false, autoContrast: false };
    emit();
  }
</script>

<div class="auto-fix" data-testid="auto-fix">
  <div class="controls">
    <button
      type="button"
      class:selected={state.autoColor && state.autoContrast}
      data-testid="feeling-lucky"
      onclick={lucky}
    >
      {t("edit.autoFix.lucky")}
    </button>
    <button
      type="button"
      class:selected={state.autoContrast}
      data-testid="auto-contrast"
      onclick={() => toggle("autoContrast")}
    >
      {t("edit.autoFix.contrast")}
    </button>
    <button
      type="button"
      class:selected={state.autoColor}
      data-testid="auto-color"
      onclick={() => toggle("autoColor")}
    >
      {t("edit.autoFix.color")}
    </button>
    <button
      type="button"
      data-testid="clear"
      disabled={!state.autoColor && !state.autoContrast}
      onclick={clear}
    >
      {t("edit.autoFix.clear")}
    </button>
  </div>
</div>

<style>
  .auto-fix {
    display: grid;
    gap: 0.5rem;
  }
  .controls {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex-wrap: wrap;
  }
  button {
    padding: 0.25rem 0.75rem;
    border: 1px solid var(--fb-border, #ccc);
    background: var(--fb-surface, #fff);
    cursor: pointer;
  }
  button.selected {
    border-color: var(--fb-accent, #4a9eff);
    background: var(--fb-accent-soft, #e6f1ff);
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>

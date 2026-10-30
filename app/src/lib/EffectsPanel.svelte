<!--
  Basic Fixes — Effects tab (TASK-04-009, PLAN §2E Effects). One-click
  starter effects — Sharpen, Sepia, B&W, Saturation — each a recipe op with
  a strength slider. All pixel work is done by the shared Rust engine
  (golden-tested) at preview/export resolution. Never touches the original
  file.
-->
<script lang="ts">
  import { t } from "./i18n.svelte.js";
  import {
    clampedStrength,
    EFFECTS,
    type EffectName,
    type EffectsState,
    toggleEffect,
  } from "./effects";

  interface Props {
    /// `preview://` URI for the image being edited.
    previewSrc: string;
    /// Fired on every state change with the current effects state.
    onchange: (state: EffectsState) => void;
    /// State to start from (TASK-04-010): the edit view passes the state it
    /// restored from the photo's recipe so reopening keeps the effects.
    initial?: EffectsState;
  }

  let { previewSrc, onchange, initial }: Props = $props();

  let state = $state<EffectsState>({ ...(initial ?? {}) });

  let announced = false;
  $effect(() => {
    if (announced) return;
    announced = true;
    emit();
  });

  function emit(): void {
    Promise.resolve()
      .then(() => onchange(state))
      .catch((e) => console.error("effects change rejected", e));
  }

  function click(effect: EffectName): void {
    state = toggleEffect(state, effect);
    emit();
  }

  function slide(effect: EffectName, value: string): void {
    state = { ...state, [effect]: clampedStrength(effect, Number(value)) };
    emit();
  }
</script>

<div class="effects" data-testid="effects">
  <div class="stage">
    <img class="base" src={previewSrc} alt="" draggable="false" />
  </div>

  <div class="controls">
    {#each EFFECTS as effect (effect)}
      <div class="effect">
        <button
          type="button"
          class:selected={effect in state}
          data-testid={`effect-${effect}`}
          onclick={() => click(effect)}
        >
          {t(`edit.effects.${effect}`)}
        </button>
        {#if effect in state}
          <label>
            {t("edit.effects.strength")}
            <input
              type="range"
              min={effect === "saturation" ? -1 : 0}
              max="1"
              step="0.05"
              value={state[effect]}
              data-testid={`strength-${effect}`}
              oninput={(e) => slide(effect, e.currentTarget.value)}
            />
          </label>
        {/if}
      </div>
    {/each}
  </div>
</div>

<style>
  .effects {
    display: grid;
    gap: 0.5rem;
  }
  .stage {
    position: relative;
    background: var(--fb-surface-alt, #ececec);
  }
  .base {
    display: block;
    width: 100%;
    height: auto;
    user-select: none;
    -webkit-user-drag: none;
  }
  .controls {
    display: grid;
    gap: 0.35rem;
  }
  .effect {
    display: grid;
    gap: 0.25rem;
  }
  button {
    justify-self: start;
    padding: 0.25rem 0.75rem;
    border: 1px solid var(--fb-border, #ccc);
    background: var(--fb-surface, #fff);
    cursor: pointer;
  }
  button.selected {
    border-color: var(--fb-accent, #4a9eff);
    background: var(--fb-accent-soft, #e6f1ff);
  }
  label {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  input[type="range"] {
    flex: 1;
  }
</style>

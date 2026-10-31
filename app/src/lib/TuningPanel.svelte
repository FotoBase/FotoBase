<!--
  Basic Fixes — Tuning tab (TASK-04-008, PLAN §2E). Four sliders (Fill
  Light, Highlights, Shadows, Color Temperature) plus the neutral-color
  (white balance) picker: arm the picker, click a pixel that should be
  neutral, and the Rust engine computes the temperature/tint params that
  neutralize the sampled pixel. All pixel work is done by the shared Rust
  engine (golden-tested) at preview/export resolution. Never touches the
  original file.
-->
<script lang="ts">
  import { t } from "./i18n.svelte.js";
  import {
    applyBalance,
    balanceFromSample,
    clampedSlider,
    DEFAULT_TUNING,
    sampleNeutral,
    type TuningState,
  } from "./tuning";

  interface Props {
    /// `preview://` URI for the image being edited.
    previewSrc: string;
    /// Fired on every state change with the current tuning state.
    onchange: (state: TuningState) => void;
    /// Pixel sampler seam (normalized coordinates); the default draws the
    /// preview into an offscreen canvas and averages a 3×3 window. Tests
    /// inject a deterministic sampler.
    samplePixel?: (nx: number, ny: number) => Promise<[number, number, number]>;
    /// State to start from (TASK-04-010): the edit view passes the state it
    /// restored from the photo's recipe so reopening keeps the sliders.
    initial?: TuningState;
  }

  let { previewSrc, onchange, samplePixel, initial }: Props = $props();

  let state = $state<TuningState>({ ...(initial ?? DEFAULT_TUNING) });
  // Picker armed: the next click on the preview defines neutral.
  let picking = $state(false);
  let baseImg: HTMLImageElement | undefined;

  // The edit view needs the initial (untouched) state to know the photo has
  // no tuning ops yet — emit once on mount.
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
      .catch((e) => console.error("tuning change rejected", e));
  }

  function slide(key: keyof TuningState, value: string): void {
    state = { ...state, [key]: clampedSlider(key, Number(value)) };
    emit();
  }

  function togglePicker(): void {
    picking = !picking;
  }

  async function defaultSamplePixel(nx: number, ny: number): Promise<[number, number, number]> {
    const img = baseImg;
    if (!img || !img.naturalWidth) throw new Error("preview not loaded");
    const w = img.naturalWidth;
    const h = img.naturalHeight;
    const canvas = document.createElement("canvas");
    canvas.width = w;
    canvas.height = h;
    const ctx = canvas.getContext("2d", { willReadFrequently: true });
    if (!ctx) throw new Error("canvas unavailable");
    ctx.drawImage(img, 0, 0);
    const cx = Math.min(w - 1, Math.max(0, Math.floor(nx * w)));
    const cy = Math.min(h - 1, Math.max(0, Math.floor(ny * h)));
    // Clamp the 3×3 window's origin so it stays inside the image.
    const x0 = Math.min(Math.max(cx - 1, 0), Math.max(0, w - 3));
    const y0 = Math.min(Math.max(cy - 1, 0), Math.max(0, h - 3));
    const ww = Math.min(3, w);
    const wh = Math.min(3, h);
    const data = ctx.getImageData(x0, y0, ww, wh).data;
    return sampleNeutral(data, ww, wh, cx - x0, cy - y0);
  }

  async function pick(e: MouseEvent): Promise<void> {
    if (!picking) return;
    picking = false;
    const box = baseImg?.getBoundingClientRect();
    const nx = box && box.width > 0 ? (e.clientX - box.left) / box.width : 0;
    const ny = box && box.height > 0 ? (e.clientY - box.top) / box.height : 0;
    const sample = samplePixel
      ? await samplePixel(Math.min(1, Math.max(0, nx)), Math.min(1, Math.max(0, ny)))
      : await defaultSamplePixel(nx, ny);
    const balance = await balanceFromSample(sample);
    state = applyBalance(state, balance);
    emit();
  }
</script>

<div class="tuning" data-testid="tuning">
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="stage" data-testid="stage" class:picking onclick={pick}>
    <img bind:this={baseImg} class="base" src={previewSrc} alt="" draggable="false" />
  </div>

  <div class="controls">
    <label>
      {t("edit.tuning.fillLight")}
      <input
        type="range"
        min="0"
        max="1"
        step="0.05"
        value={state.fillLight}
        data-testid="fill-light"
        oninput={(e) => slide("fillLight", e.currentTarget.value)}
      />
    </label>
    <label>
      {t("edit.tuning.highlights")}
      <input
        type="range"
        min="0"
        max="1"
        step="0.05"
        value={state.highlights}
        data-testid="highlights"
        oninput={(e) => slide("highlights", e.currentTarget.value)}
      />
    </label>
    <label>
      {t("edit.tuning.shadows")}
      <input
        type="range"
        min="0"
        max="1"
        step="0.05"
        value={state.shadows}
        data-testid="shadows"
        oninput={(e) => slide("shadows", e.currentTarget.value)}
      />
    </label>
    <label>
      {t("edit.tuning.temperature")}
      <input
        type="range"
        min="-1"
        max="1"
        step="0.05"
        value={state.temperature}
        data-testid="temperature"
        oninput={(e) => slide("temperature", e.currentTarget.value)}
      />
    </label>
    <button
      type="button"
      class:selected={picking}
      data-testid="neutral-picker"
      onclick={togglePicker}
    >
      {t("edit.tuning.picker")}
    </button>
    <span class="hint" data-testid="hint">
      {picking ? t("edit.tuning.pickerHint") : t("edit.tuning.hint")}
    </span>
  </div>
</div>

<style>
  .tuning {
    display: grid;
    gap: 0.5rem;
  }
  .stage {
    position: relative;
    background: var(--fb-surface-alt, #ececec);
  }
  .stage.picking {
    cursor: crosshair;
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
  label {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  input[type="range"] {
    flex: 1;
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
  .hint {
    font-size: 0.85rem;
    opacity: 0.75;
  }
</style>

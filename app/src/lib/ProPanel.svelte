<script lang="ts">
  import { t } from "./i18n.svelte.js";
  import { DEFAULT_PRO, type ProState, type RecipeOp } from "./edit-recipe";

  interface Props {
    previewSrc: string;
    onchange: (state: ProState) => void;
    ops: RecipeOp[];
    onreorder: (from: number, to: number) => void;
    onduplicate: (index: number) => void;
    onremove: (index: number) => void;
    /// Resolve the photo's lens profile from EXIF via Lensfun (backend
    /// `lens_profile`); rejects with the reason when there is none.
    detectLens: () => Promise<Record<string, unknown>>;
    initial?: ProState;
  }

  let { previewSrc, onchange, initial, ops, onreorder, onduplicate, onremove, detectLens }: Props = $props();
  let state = $state<ProState>({
    ...DEFAULT_PRO,
    ...(initial ?? {}),
    curves: { ...DEFAULT_PRO.curves, ...(initial?.curves ?? {}) },
    levels: { ...DEFAULT_PRO.levels, ...(initial?.levels ?? {}) },
    hsl: { ...DEFAULT_PRO.hsl, ...(initial?.hsl ?? {}) },
    keystone: { ...DEFAULT_PRO.keystone, ...(initial?.keystone ?? {}) },
    lensCorrection: initial?.lensCorrection ? { ...initial.lensCorrection } : null,
    noiseReduction: { ...DEFAULT_PRO.noiseReduction, ...(initial?.noiseReduction ?? {}) },
  });

  let announced = false;
  $effect(() => {
    if (announced) return;
    announced = true;
    emit();
  });

  function emit(): void {
    Promise.resolve().then(() => onchange(state)).catch((e) => console.error("pro change rejected", e));
  }

  function setExposure(value: string): void {
    state = { ...state, exposure: Number(value) };
    emit();
  }

  function setVibrance(value: string): void {
    state = { ...state, vibrance: Number(value) };
    emit();
  }

  function setLevels(key: "black" | "white" | "gamma", value: string): void {
    state = { ...state, levels: { ...state.levels, [key]: Number(value) } };
    emit();
  }

  function setHsl(key: "hue" | "saturation" | "luminance", value: string): void {
    state = { ...state, hsl: { ...state.hsl, [key]: Number(value) } };
    emit();
  }

  function setNoise(value: string): void {
    state = { ...state, noiseReduction: { ...state.noiseReduction, amount: Number(value) } };
    emit();
  }

  function toggleCurves(): void {
    state = { ...state, curves: { ...state.curves, enabled: !state.curves.enabled } };
    emit();
  }

  function setCurvePoint(index: number, axis: 0 | 1, value: string): void {
    const points = state.curves.points.map((point, i) =>
      i === index ? point.map((coordinate, j) => j === axis ? Number(value) : coordinate) : point,
    );
    state = { ...state, curves: { ...state.curves, points } };
    emit();
  }

  let lensBusy = $state(false);
  let lensError = $state<string | null>(null);

  async function detect(): Promise<void> {
    lensBusy = true;
    lensError = null;
    try {
      state = { ...state, lensCorrection: await detectLens() };
      emit();
    } catch (e) {
      lensError = String(e);
    } finally {
      lensBusy = false;
    }
  }

  function toggleLens(key: "distortion" | "vignetting" | "tca"): void {
    if (!state.lensCorrection) return;
    const on = state.lensCorrection[key] !== false;
    state = { ...state, lensCorrection: { ...state.lensCorrection, [key]: !on } };
    emit();
  }

  function clearLens(): void {
    state = { ...state, lensCorrection: null };
    emit();
  }

  function setKeystone(key: "top" | "bottom" | "left" | "right", value: string): void {
    state = { ...state, keystone: { ...state.keystone, [key]: Number(value) } };
    emit();
  }
</script>

<div class="pro" data-testid="pro">
  <div class="stage"><img src={previewSrc} alt="" draggable="false" /></div>
  {#if ops.length > 0}
    <section aria-label={t("edit.pro.stack")}>
      <h3>{t("edit.pro.stack")}</h3>
      <ol>
        {#each ops as op, index}
          <li>
            <span>{op.type}</span>
            <button type="button" disabled={index === 0} data-testid={`pro-move-up-${index}`} aria-label={t("edit.pro.moveUp", { n: index + 1, type: op.type })} onclick={() => onreorder(index, index - 1)}>{t("edit.pro.up")}</button>
            <button type="button" disabled={index === ops.length - 1} data-testid={`pro-move-down-${index}`} aria-label={t("edit.pro.moveDown", { n: index + 1, type: op.type })} onclick={() => onreorder(index, index + 1)}>{t("edit.pro.down")}</button>
            <button type="button" data-testid={`pro-duplicate-${index}`} aria-label={t("edit.pro.duplicateOp", { n: index + 1, type: op.type })} onclick={() => onduplicate(index)}>{t("edit.pro.duplicate")}</button>
            <button type="button" data-testid={`pro-remove-${index}`} aria-label={t("edit.pro.removeOp", { n: index + 1, type: op.type })} onclick={() => onremove(index)}>{t("edit.pro.remove")}</button>
          </li>
        {/each}
      </ol>
    </section>
  {/if}
  <div class="controls">
    <button type="button" data-testid="pro-curves" aria-pressed={state.curves.enabled} onclick={toggleCurves}>{t("edit.pro.curves")}</button>
    {#if state.curves.enabled}
      {#each state.curves.points as point, index}
        <label>{t("edit.pro.curveInput", { n: index + 1 })}<input type="range" min="0" max="1" step="0.01" value={point[0]} data-testid={`pro-curve-x-${index}`} oninput={(e) => setCurvePoint(index, 0, e.currentTarget.value)} /></label>
        <label>{t("edit.pro.curveOutput", { n: index + 1 })}<input type="range" min="0" max="1" step="0.01" value={point[1]} data-testid={`pro-curve-y-${index}`} oninput={(e) => setCurvePoint(index, 1, e.currentTarget.value)} /></label>
      {/each}
    {/if}
    <label>{t("edit.pro.exposure")}<input type="range" min="-5" max="5" step="0.1" value={state.exposure} data-testid="pro-exposure" oninput={(e) => setExposure(e.currentTarget.value)} /></label>
    <label>{t("edit.pro.vibrance")}<input type="range" min="-1" max="1" step="0.05" value={state.vibrance} data-testid="pro-vibrance" oninput={(e) => setVibrance(e.currentTarget.value)} /></label>
    <label>{t("edit.pro.levelsBlack")}<input type="range" min="0" max="1" step="0.01" value={state.levels.black} data-testid="pro-levels-black" oninput={(e) => setLevels("black", e.currentTarget.value)} /></label>
    <label>{t("edit.pro.levelsWhite")}<input type="range" min="0" max="1" step="0.01" value={state.levels.white} data-testid="pro-levels-white" oninput={(e) => setLevels("white", e.currentTarget.value)} /></label>
    <label>{t("edit.pro.levelsGamma")}<input type="range" min="0.1" max="10" step="0.1" value={state.levels.gamma} data-testid="pro-levels-gamma" oninput={(e) => setLevels("gamma", e.currentTarget.value)} /></label>
    <label>{t("edit.pro.hue")}<input type="range" min="-1" max="1" step="0.05" value={state.hsl.hue} data-testid="pro-hue" oninput={(e) => setHsl("hue", e.currentTarget.value)} /></label>
    <label>{t("edit.pro.hslSaturation")}<input type="range" min="-1" max="1" step="0.05" value={state.hsl.saturation} data-testid="pro-hsl-saturation" oninput={(e) => setHsl("saturation", e.currentTarget.value)} /></label>
    <label>{t("edit.pro.luminance")}<input type="range" min="-1" max="1" step="0.05" value={state.hsl.luminance} data-testid="pro-luminance" oninput={(e) => setHsl("luminance", e.currentTarget.value)} /></label>
    <label>{t("edit.pro.noise")}<input type="range" min="0" max="1" step="0.05" value={state.noiseReduction.amount} data-testid="pro-noise" oninput={(e) => setNoise(e.currentTarget.value)} /></label>
    <fieldset class="lens">
      <legend>{t("edit.pro.lens")}</legend>
      <button type="button" data-testid="pro-lens-detect" disabled={lensBusy} onclick={() => void detect()}>{t("edit.pro.lensDetect")}</button>
      {#if lensError}<p role="alert" data-testid="pro-lens-error">{lensError}</p>{/if}
      {#if state.lensCorrection}
        <p data-testid="pro-lens-name">{String(state.lensCorrection.lens ?? "")}</p>
        {#each ["distortion", "vignetting", "tca"] as key}
          <label><input type="checkbox" checked={state.lensCorrection[key] !== false} data-testid={`pro-lens-${key}`} onchange={() => toggleLens(key as "distortion" | "vignetting" | "tca")} />{t(`edit.pro.lens.${key}`)}</label>
        {/each}
        <button type="button" data-testid="pro-lens-clear" onclick={clearLens}>{t("edit.pro.lensClear")}</button>
      {/if}
    </fieldset>
    {#each ["top", "bottom", "left", "right"] as key}
      <label>{t(`edit.pro.keystone.${key}`)}<input type="range" min="-1" max="1" step="0.05" value={state.keystone[key as "top" | "bottom" | "left" | "right"]} data-testid={`pro-keystone-${key}`} oninput={(e) => setKeystone(key as "top" | "bottom" | "left" | "right", e.currentTarget.value)} /></label>
    {/each}
  </div>
</div>

<style>
  .pro { display: grid; gap: 0.5rem; }
  .stage { height: 100px; overflow: hidden; }
  .stage img { width: 100%; height: 100%; object-fit: contain; }
  .controls { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 0.25rem 0.75rem; }
  label { display: grid; gap: 0.15rem; font-size: 0.75rem; }
  input { width: 100%; }
</style>

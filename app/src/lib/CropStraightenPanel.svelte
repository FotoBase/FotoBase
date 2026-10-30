<!--
  Basic Fixes — crop & straighten (TASK-04-005, PLAN §2E).
  Crop with draggable handles + preset ratios, straighten slider with
  auto-crop. The live preview is a CSS transform/crop over the `preview://`
  image: interactive speeds, no IPC, no engine round-trip — the recipe ops
  emitted here are applied by the shared Rust engine (golden-tested) at
  preview and export resolution. Never touches the original file.
-->
<script lang="ts">
  import { t } from "./i18n.svelte.js";
  import {
    applyAspectPreset,
    ASPECT_PRESETS,
    autoFillScale,
    moveCrop,
    resizeCrop,
    type CropRect,
    type CropStraightenState,
    type Handle,
  } from "./crop-straighten";

  interface Props {
    /// `preview://` URI for the image being edited.
    previewSrc: string;
    /// Fired on every state change with the current crop/straighten state.
    onchange: (state: CropStraightenState) => void;
  }

  let { previewSrc, onchange }: Props = $props();

  let state = $state<CropStraightenState>({
    crop: { x: 0, y: 0, w: 1, h: 1 },
    angle: 0,
    autoFill: true,
  });
  let activePreset = $state<string>("free");
  // Stage box in px (the full image's display size). Measured from the base
  // image; jsdom-safe because tests stub getBoundingClientRect.
  let stageW = $state(100);
  let stageH = $state(50);
  let baseImg: HTMLImageElement | undefined;

  function measure(): void {
    const box = baseImg?.getBoundingClientRect();
    if (box && box.width > 0 && box.height > 0) {
      stageW = box.width;
      stageH = box.height;
    }
  }

  // The edit view needs the initial (untouched) state to know the photo has
  // no crop/straighten ops yet — emit once on mount.
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
      .catch((e) => console.error("crop/straighten change rejected", e));
  }

  function choosePreset(preset: (typeof ASPECT_PRESETS)[number]): void {
    activePreset = preset.id;
    const aspect = stageH > 0 ? stageW / stageH : 1;
    state = { ...state, crop: applyAspectPreset(state.crop, preset.ratio, aspect) };
    emit();
  }

  function setAngle(value: string): void {
    state = { ...state, angle: Number(value) };
    emit();
  }

  function setAutoFill(value: boolean): void {
    state = { ...state, autoFill: value };
    emit();
  }

  // --- Dragging --------------------------------------------------------------
  type Drag =
    | { kind: "resize"; handle: Handle; startX: number; startY: number; crop: CropRect }
    | { kind: "move"; startX: number; startY: number; crop: CropRect };

  let drag: Drag | null = null;

  function startResize(handle: Handle): (e: PointerEvent) => void {
    return (e) => {
      measure();
      drag = { kind: "resize", handle, startX: e.clientX, startY: e.clientY, crop: state.crop };
      e.preventDefault();
    };
  }

  function startMove(e: PointerEvent): void {
    if (state.crop.w >= 1 && state.crop.h >= 1) return; // nothing to move
    measure();
    drag = { kind: "move", startX: e.clientX, startY: e.clientY, crop: state.crop };
  }

  function onMove(e: PointerEvent): void {
    if (drag === null) return;
    const dx = (e.clientX - drag.startX) / stageW;
    const dy = (e.clientY - drag.startY) / stageH;
    if (drag.kind === "resize") {
      const ratio = ASPECT_PRESETS.find((p) => p.id === activePreset)?.ratio ?? null;
      state = { ...state, crop: resizeCrop(drag.crop, drag.handle, dx, dy, ratio) };
    } else {
      state = { ...state, crop: moveCrop(drag.crop, dx, dy) };
    }
    emit();
  }

  function endDrag(): void {
    drag = null;
  }

  // --- Derived preview -------------------------------------------------------
  const windowStyle = $derived(
    `left: ${state.crop.x * 100}%; top: ${state.crop.y * 100}%;` +
      ` width: ${state.crop.w * 100}%; height: ${state.crop.h * 100}%;`,
  );
  // The inner image is the full image shifted so the crop region fills the
  // window; rotating around the crop region's center keeps it centered.
  const innerTransform = $derived(
    `rotate(${state.angle}deg) scale(${autoFillScale(state.angle, state.crop.w, state.crop.h, state.autoFill)})`,
  );
</script>

<div class="crop-straighten" data-testid="crop-straighten">
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="stage"
    data-testid="stage"
    onpointermove={onMove}
    onpointerup={endDrag}
    onpointercancel={endDrag}
  >
    <img
      bind:this={baseImg}
      class="base"
      src={previewSrc}
      alt=""
      draggable="false"
    />
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="crop-window"
      style={windowStyle}
      data-testid="crop-overlay"
      data-x={state.crop.x}
      data-y={state.crop.y}
      data-w={state.crop.w}
      data-h={state.crop.h}
      onpointerdown={startMove}
    >
      <img
        class="inner"
        data-testid="preview-img"
        src={previewSrc}
        alt=""
        draggable="false"
        style:width={`${stageW}px`}
        style:height={`${stageH}px`}
        style:left={`${-state.crop.x * stageW}px`}
        style:top={`${-state.crop.y * stageH}px`}
        style:transform={innerTransform}
        style:transform-origin={`${(state.crop.x + state.crop.w / 2) * stageW}px ${(state.crop.y + state.crop.h / 2) * stageH}px`}
      />
      {#each [["tl", "nw-resize"], ["tr", "ne-resize"], ["bl", "sw-resize"], ["br", "se-resize"]] as [h, cursor] (h)}
        <button
          type="button"
          class="handle handle-{h}"
          data-testid={`handle-${h}`}
          aria-label={t("edit.crop.handle")}
          style:cursor={cursor}
          onpointerdown={startResize(h as Handle)}
        ></button>
      {/each}
    </div>
  </div>

  <div class="controls">
    <div class="presets" role="group" aria-label={t("edit.crop.presets")}>
      {#each ASPECT_PRESETS as p (p.id)}
        <button
          type="button"
          class:selected={activePreset === p.id}
          data-testid={`preset-${p.id}`}
          onclick={() => choosePreset(p)}
        >
          {p.id === "free" ? t("edit.crop.free") : p.id}
        </button>
      {/each}
    </div>
    <label class="straighten">
      {t("edit.straighten.label")}
      <input
        type="range"
        min="-10"
        max="10"
        step="0.5"
        value={state.angle}
        data-testid="angle-slider"
        oninput={(e) => setAngle((e.currentTarget as HTMLInputElement).value)}
      />
      <span data-testid="angle-value">{state.angle}°</span>
    </label>
    <label class="auto-fill">
      <input
        type="checkbox"
        checked={state.autoFill}
        data-testid="auto-fill"
        onchange={(e) => setAutoFill((e.currentTarget as HTMLInputElement).checked)}
      />
      {t("edit.straighten.autoFill")}
    </label>
  </div>
</div>

<style>
  .crop-straighten {
    display: grid;
    gap: 0.5rem;
  }
  .stage {
    position: relative;
    touch-action: none;
    background: var(--fb-surface-alt, #ececec);
  }
  .base {
    display: block;
    width: 100%;
    height: auto;
    user-select: none;
    -webkit-user-drag: none;
  }
  .crop-window {
    position: absolute;
    overflow: hidden;
    outline: 2px solid var(--fb-accent, #4a9eff);
    touch-action: none;
  }
  .inner {
    position: absolute;
    display: block;
    user-select: none;
    -webkit-user-drag: none;
  }
  .handle {
    position: absolute;
    width: 12px;
    height: 12px;
    padding: 0;
    border: 2px solid var(--fb-accent, #4a9eff);
    background: var(--fb-surface, #fff);
    touch-action: none;
  }
  .handle-tl { left: -6px; top: -6px; }
  .handle-tr { right: -6px; top: -6px; }
  .handle-bl { left: -6px; bottom: -6px; }
  .handle-br { right: -6px; bottom: -6px; }
  .controls {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.75rem;
  }
  .presets {
    display: flex;
    gap: 0.25rem;
  }
  .presets button {
    padding: 0.15rem 0.5rem;
  }
  .presets button.selected {
    background: var(--fb-accent, #4a9eff);
    color: var(--fb-surface, #fff);
  }
</style>

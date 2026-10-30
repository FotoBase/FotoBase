<!--
  Basic Fixes — red-eye removal (TASK-04-006, PLAN §2E). Auto mode is a
  one-click op (`auto: true`) handled by the shared Rust detection
  heuristic; manual mode lets the user drag a circle over an eye region.
  The overlay preview draws the marked regions over the `preview://` image;
  the pixel fix itself is applied by the shared Rust engine (golden-tested)
  at preview/export resolution. Never touches the original file.
-->
<script lang="ts">
  import { t } from "./i18n.svelte.js";
  import {
    regionContains,
    regionFromDrag,
    type RedEyeRegion,
    type RedEyeState,
  } from "./red-eye";

  interface Props {
    /// `preview://` URI for the image being edited.
    previewSrc: string;
    /// Fired on every state change with the current red-eye state.
    onchange: (state: RedEyeState) => void;
    /// State to start from (TASK-04-010): the edit view passes the state it
    /// restored from the photo's recipe so reopening keeps the edits.
    initial?: RedEyeState;
  }

  let { previewSrc, onchange, initial }: Props = $props();

  let state = $state<RedEyeState>(
    initial ? { auto: initial.auto, regions: initial.regions.map((r) => ({ ...r })) } : { auto: false, regions: [] },
  );
  let manual = $state(false);
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
  // no red-eye ops yet — emit once on mount.
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
      .catch((e) => console.error("red-eye change rejected", e));
  }

  function setAuto(on: boolean): void {
    state = { ...state, auto: on };
    emit();
  }

  function toggleManual(): void {
    manual = !manual;
  }

  function removeRegion(index: number): void {
    state = { ...state, regions: state.regions.filter((_, i) => i !== index) };
    emit();
  }

  // --- Dragging --------------------------------------------------------------
  let dragStart = $state<{ x: number; y: number } | null>(null);
  let dragCurrent = $state<{ x: number; y: number } | null>(null);

  function toStage(e: PointerEvent): { x: number; y: number } {
    const box = baseImg?.getBoundingClientRect();
    const left = box?.left ?? 0;
    const top = box?.top ?? 0;
    return {
      x: (e.clientX - left) / stageW,
      y: (e.clientY - top) / stageH,
    };
  }

  function startDrag(e: PointerEvent): void {
    if (!manual) return;
    measure();
    dragStart = toStage(e);
    dragCurrent = dragStart;
    e.preventDefault();
  }

  function onMove(e: PointerEvent): void {
    if (dragStart === null) return;
    dragCurrent = toStage(e);
  }

  function endDrag(): void {
    if (dragStart === null || dragCurrent === null) {
      dragStart = null;
      dragCurrent = null;
      return;
    }
    const region = regionFromDrag(dragStart, dragCurrent);
    state = { ...state, regions: [...state.regions, region] };
    dragStart = null;
    dragCurrent = null;
    emit();
  }

  function removeAt(e: PointerEvent): void {
    if (!manual) return;
    const p = toStage(e);
    // Topmost (last drawn) matching region wins.
    for (let i = state.regions.length - 1; i >= 0; i--) {
      if (regionContains(state.regions[i], p.x, p.y)) {
        removeRegion(i);
        return;
      }
    }
  }

  const windowStyle = $derived.by(() => {
    if (dragStart === null || dragCurrent === null) return "";
    const r = regionFromDrag(dragStart, dragCurrent);
    return redEyeCircleStyle(r);
  });

  function redEyeCircleStyle(r: RedEyeRegion): string {
    return (
      `left: ${(r.x - r.r) * 100}%; top: ${(r.y - r.r) * 100}%;` +
      ` width: ${r.r * 2 * 100}%; height: ${r.r * 2 * 100}%;`
    );
  }
</script>

<div class="red-eye" data-testid="red-eye">
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="stage"
    data-testid="stage"
    class:manual
    onpointerdown={startDrag}
    onpointermove={onMove}
    onpointerup={endDrag}
    onpointercancel={endDrag}
  >
    <img bind:this={baseImg} class="base" src={previewSrc} alt="" draggable="false" />
    {#each state.regions as r, i (i)}
      <div
        class="region"
        style={redEyeCircleStyle(r)}
        data-testid={`region-${i}`}
        data-x={r.x}
        data-y={r.y}
        data-r={r.r}
        onpointerdown={(e) => {
          if (manual) {
            e.stopPropagation();
            removeAt(e);
          }
        }}
      ></div>
    {/each}
    {#if dragStart !== null && dragCurrent !== null}
      <div class="region dragging" style={windowStyle}></div>
    {/if}
  </div>

  <div class="controls">
    <button
      type="button"
      class:selected={state.auto}
      data-testid="auto-button"
      onclick={() => setAuto(!state.auto)}
    >
      {t("edit.redEye.auto")}
    </button>
    <button
      type="button"
      class:selected={manual}
      data-testid="manual-button"
      onclick={toggleManual}
    >
      {t("edit.redEye.manual")}
    </button>
    <span class="hint" data-testid="hint">
      {state.auto
        ? t("edit.redEye.autoHint")
        : manual
          ? t("edit.redEye.manualHint")
          : t("edit.redEye.hint")}
    </span>
    <button
      type="button"
      data-testid="clear"
      disabled={state.regions.length === 0 && !state.auto}
      onclick={() => {
        state = { auto: false, regions: [] };
        emit();
      }}
    >
      {t("edit.redEye.clear")}
    </button>
  </div>
</div>

<style>
  .red-eye {
    display: grid;
    gap: 0.5rem;
  }
  .stage {
    position: relative;
    touch-action: none;
    background: var(--fb-surface-alt, #ececec);
  }
  .stage.manual {
    cursor: crosshair;
  }
  .base {
    display: block;
    width: 100%;
    height: auto;
    user-select: none;
    -webkit-user-drag: none;
  }
  .region {
    position: absolute;
    border: 2px solid var(--fb-accent, #4a9eff);
    border-radius: 50%;
    pointer-events: none;
  }
  .manual .region {
    pointer-events: auto;
    cursor: pointer;
  }
  .region.dragging {
    border-style: dashed;
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
  .hint {
    color: var(--fb-text-dim, #777);
    font-size: 0.8rem;
  }
</style>

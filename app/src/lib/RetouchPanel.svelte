<!--
  Basic Fixes — Retouch heal brush (TASK-08-002, PLAN §2E). Paint over a
  blemish on the preview; each drag is recorded as one stroke (normalized
  points + radius) and emitted as state. The pixels are healed by the shared
  Rust engine at preview and export resolution, so nothing here touches the
  image or the original file. Strokes stay editable: the list deletes any
  single stroke, Undo pops the newest.
-->
<script lang="ts">
  import { t } from "./i18n.svelte.js";
  import {
    DEFAULT_RADIUS,
    MAX_RADIUS,
    MIN_RADIUS,
    removeStroke,
    strokeFromPoints,
    undoLastStroke,
    type RetouchState,
    type RetouchStroke,
  } from "./retouch";

  interface Props {
    /// `preview://` URI for the image being edited.
    previewSrc: string;
    /// Fired on every state change with the current retouch state.
    onchange: (state: RetouchState) => void;
    /// State to start from: the edit view passes the state it restored
    /// from the photo's recipe so reopening keeps the strokes.
    initial?: RetouchState;
  }

  let { previewSrc, onchange, initial }: Props = $props();

  let state = $state<RetouchState>({
    strokes: (initial?.strokes ?? []).map((s) => ({
      points: s.points.map((p) => [p[0], p[1]] as [number, number]),
      radius: s.radius,
    })),
  });
  let radius = $state(DEFAULT_RADIUS);
  let stageW = $state(100);
  let stageH = $state(50);
  let stage: HTMLDivElement | undefined;
  let drag = $state<Array<{ x: number; y: number }> | null>(null);

  function measure(): void {
    const box = stage?.getBoundingClientRect();
    if (box && box.width > 0 && box.height > 0) {
      stageW = box.width;
      stageH = box.height;
    }
  }

  $effect(() => {
    measure();
    if (stage === undefined || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(stage);
    return () => observer.disconnect();
  });

  // The edit view needs the initial state to know the photo's strokes.
  let announced = false;
  $effect(() => {
    if (announced) return;
    announced = true;
    emit();
  });

  function emit(): void {
    Promise.resolve()
      .then(() => onchange(state))
      .catch((e) => console.error("retouch change rejected", e));
  }

  function toStage(e: PointerEvent): { x: number; y: number } {
    const box = stage?.getBoundingClientRect();
    return {
      x: (e.clientX - (box?.left ?? 0)) / stageW,
      y: (e.clientY - (box?.top ?? 0)) / stageH,
    };
  }

  function startStroke(e: PointerEvent): void {
    measure();
    drag = [toStage(e)];
    stage?.setPointerCapture?.(e.pointerId);
    e.preventDefault();
  }

  function moveStroke(e: PointerEvent): void {
    if (drag === null) return;
    drag = [...drag, toStage(e)];
  }

  function endStroke(e: PointerEvent): void {
    if (drag === null) return;
    const points = e.type === "pointerup" ? [...drag, toStage(e)] : drag;
    drag = null;
    const stroke = strokeFromPoints(points, radius);
    if (stroke === null) return;
    state = { strokes: [...state.strokes, stroke] };
    emit();
  }

  function remove(index: number): void {
    state = removeStroke(state, index);
    emit();
  }

  function undoLast(): void {
    state = undoLastStroke(state);
    emit();
  }

  function clear(): void {
    state = { strokes: [] };
    emit();
  }

  const brushPx = (r: number): number => 2 * r * Math.min(stageW, stageH);

  function polyline(stroke: RetouchStroke): string {
    // A one-point stroke is a dab: a zero-length segment renders as a dot
    // with round caps.
    const pts = stroke.points.length === 1 ? [stroke.points[0], stroke.points[0]] : stroke.points;
    return pts.map(([x, y]) => `${x * stageW},${y * stageH}`).join(" ");
  }

  const draft = $derived(drag === null ? null : strokeFromPoints(drag, radius));
</script>

<div class="retouch" data-testid="retouch">
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    bind:this={stage}
    class="stage"
    data-testid="retouch-stage"
    onpointerdown={startStroke}
    onpointermove={moveStroke}
    onpointerup={endStroke}
    onpointercancel={endStroke}
  >
    <img class="base" src={previewSrc} alt="" draggable="false" onload={measure} />
    <svg class="overlay" viewBox={`0 0 ${stageW} ${stageH}`} aria-hidden="true">
      {#each state.strokes as stroke, i (i)}
        <polyline class="stroke" points={polyline(stroke)} stroke-width={brushPx(stroke.radius)} />
      {/each}
      {#if draft !== null}
        <polyline class="stroke draft" points={polyline(draft)} stroke-width={brushPx(draft.radius)} />
      {/if}
    </svg>
  </div>

  <div class="controls">
    <label class="size">
      <span>{t("edit.retouch.size")}</span>
      <input
        type="range"
        data-testid="retouch-size"
        min={MIN_RADIUS}
        max={MAX_RADIUS}
        step="0.005"
        bind:value={radius}
      />
    </label>
    <button type="button" data-testid="retouch-undo" disabled={state.strokes.length === 0} onclick={undoLast}>
      {t("edit.retouch.undo")}
    </button>
    <button type="button" data-testid="retouch-clear" disabled={state.strokes.length === 0} onclick={clear}>
      {t("edit.retouch.clear")}
    </button>
    <span class="hint">{t("edit.retouch.hint")}</span>
  </div>

  {#if state.strokes.length > 0}
    <ul class="strokes" data-testid="retouch-strokes">
      {#each state.strokes as stroke, i (i)}
        <li data-testid={`retouch-stroke-${i}`}>
          <span>{t("edit.retouch.stroke", { n: i + 1, size: Math.round(stroke.radius * 200) })}</span>
          <button
            type="button"
            data-testid={`retouch-delete-${i}`}
            aria-label={t("edit.retouch.delete", { n: i + 1 })}
            onclick={() => remove(i)}
          >
            ✕
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .retouch {
    display: grid;
    gap: 0.5rem;
  }
  .stage {
    position: relative;
    touch-action: none;
    cursor: crosshair;
    background: var(--fb-surface-alt, #ececec);
  }
  .base {
    display: block;
    width: 100%;
    height: auto;
    user-select: none;
    -webkit-user-drag: none;
  }
  .overlay {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
  }
  .stroke {
    fill: none;
    stroke: var(--fb-accent, #4a9eff);
    stroke-opacity: 0.4;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .stroke.draft {
    stroke-opacity: 0.6;
  }
  .controls {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex-wrap: wrap;
  }
  .size {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }
  button {
    padding: 0.25rem 0.75rem;
    border: 1px solid var(--fb-border, #ccc);
    background: var(--fb-surface, #fff);
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .hint {
    color: var(--fb-text-dim, #777);
    font-size: 0.8rem;
  }
  .strokes {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 0.25rem;
  }
  .strokes li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }
  .strokes button {
    padding: 0 0.4rem;
  }
</style>

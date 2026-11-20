<!--
  Basic Fixes — Text tool (TASK-08-003, PLAN §2E). Add text overlays, edit
  font, size, color, outline and transparency, and drag the marker over the
  preview to place them. Every overlay is recipe state (normalized position,
  size as a fraction of the image height), never pixels: the shared Rust
  engine draws the text at preview and export resolution, so the preview the
  edit view shows is the export. The stage shows that rendered preview with a
  marker at each overlay's anchor point.
-->
<script lang="ts">
  import { t } from "./i18n.svelte.js";
  import {
    FONT_FAMILIES,
    MAX_OUTLINE,
    MAX_SIZE,
    MIN_SIZE,
    addOverlay,
    moveOverlay,
    removeOverlay,
    updateOverlay,
    type TextOverlay,
    type TextState,
  } from "./text-overlay";

  interface Props {
    /// `preview://` URI the stage shows (the rendered edit preview).
    previewSrc: string;
    /// Fired on every committed change with the current text state.
    onchange: (state: TextState) => void;
    /// State to start from: the edit view passes the state it restored
    /// from the photo's recipe so reopening keeps the overlays.
    initial?: TextState;
  }

  let { previewSrc, onchange, initial }: Props = $props();

  let state = $state<TextState>({
    overlays: (initial?.overlays ?? []).map((o) => ({ ...o })),
  });
  let selected = $state(initial !== undefined && initial.overlays.length > 0 ? 0 : -1);
  let stage: HTMLDivElement | undefined;
  let dragging = $state(false);

  const current = $derived<TextOverlay | null>(state.overlays[selected] ?? null);

  function emit(): void {
    const snapshot = $state.snapshot(state) as TextState;
    Promise.resolve()
      .then(() => onchange(snapshot))
      .catch((e) => console.error("text change rejected", e));
  }

  function patch(change: Partial<TextOverlay>): void {
    state = updateOverlay(state, selected, change);
    emit();
  }

  function add(): void {
    state = addOverlay(state, t("edit.text.content"));
    selected = state.overlays.length - 1;
    emit();
  }

  function remove(): void {
    state = removeOverlay(state, selected);
    selected = Math.min(selected, state.overlays.length - 1);
    emit();
  }

  function toStage(e: PointerEvent): { x: number; y: number } {
    const box = stage?.getBoundingClientRect();
    const w = box && box.width > 0 ? box.width : 1;
    const h = box && box.height > 0 ? box.height : 1;
    return { x: (e.clientX - (box?.left ?? 0)) / w, y: (e.clientY - (box?.top ?? 0)) / h };
  }

  function startDrag(e: PointerEvent, index: number): void {
    selected = index;
    dragging = true;
    stage?.setPointerCapture?.(e.pointerId);
    e.preventDefault();
  }

  function moveDrag(e: PointerEvent): void {
    if (!dragging) return;
    const { x, y } = toStage(e);
    state = moveOverlay(state, selected, x, y);
  }

  function endDrag(e: PointerEvent): void {
    if (!dragging) return;
    if (e.type === "pointerup") {
      const { x, y } = toStage(e);
      state = moveOverlay(state, selected, x, y);
    }
    dragging = false;
    emit();
  }

  const value = (e: Event): string => (e.currentTarget as HTMLInputElement).value;
  const label = (o: TextOverlay): string => (o.text.length > 18 ? `${o.text.slice(0, 18)}…` : o.text);
</script>

<div class="text-tool" data-testid="text-panel">
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    bind:this={stage}
    class="stage"
    data-testid="text-stage"
    onpointermove={moveDrag}
    onpointerup={endDrag}
    onpointercancel={endDrag}
  >
    <img class="base" src={previewSrc} alt="" draggable="false" />
    {#each state.overlays as overlay, i (i)}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="handle"
        class:selected={i === selected}
        data-testid={`text-handle-${i}`}
        style={`left: ${overlay.x * 100}%; top: ${overlay.y * 100}%;`}
        onpointerdown={(e) => startDrag(e, i)}
      ></div>
    {/each}
  </div>

  <div class="controls">
    <button type="button" data-testid="text-add" onclick={add}>{t("edit.text.add")}</button>
    <span class="hint">{t("edit.text.hint")}</span>
  </div>

  {#if state.overlays.length > 0}
    <ul class="items" data-testid="text-list">
      {#each state.overlays as overlay, i (i)}
        <li>
          <button
            type="button"
            class:selected={i === selected}
            data-testid={`text-item-${i}`}
            onclick={() => (selected = i)}
          >
            {t("edit.text.item", { n: i + 1, text: label(overlay) })}
          </button>
        </li>
      {/each}
    </ul>
  {/if}

  {#if current !== null}
    <div class="fields" data-testid="text-fields">
      <label>
        <span>{t("edit.text.content")}</span>
        <input
          type="text"
          data-testid="text-input"
          value={current.text}
          oninput={(e) => patch({ text: value(e) })}
        />
      </label>
      <label>
        <span>{t("edit.text.font")}</span>
        <input
          type="text"
          list="text-font-families"
          data-testid="text-font"
          value={current.font}
          onchange={(e) => patch({ font: value(e).trim() || "sans" })}
        />
        <datalist id="text-font-families">
          {#each FONT_FAMILIES as family (family)}
            <option value={family}>{t(`edit.text.font.${family}`)}</option>
          {/each}
        </datalist>
      </label>
      <label>
        <span>{t("edit.text.size")}</span>
        <input
          type="range"
          data-testid="text-size"
          min={MIN_SIZE}
          max={MAX_SIZE}
          step="0.005"
          value={current.size}
          oninput={(e) => patch({ size: Number(value(e)) })}
        />
      </label>
      <label>
        <span>{t("edit.text.color")}</span>
        <input
          type="color"
          data-testid="text-color"
          value={current.color}
          oninput={(e) => patch({ color: value(e) })}
        />
      </label>
      <label>
        <span>{t("edit.text.outline")}</span>
        <input
          type="range"
          data-testid="text-outline-width"
          min="0"
          max={MAX_OUTLINE}
          step="0.001"
          value={current.outlineWidth}
          oninput={(e) => patch({ outlineWidth: Number(value(e)) })}
        />
      </label>
      <label>
        <span>{t("edit.text.outlineColor")}</span>
        <input
          type="color"
          data-testid="text-outline-color"
          value={current.outlineColor}
          oninput={(e) => patch({ outlineColor: value(e) })}
        />
      </label>
      <label>
        <span>{t("edit.text.transparency")}</span>
        <input
          type="range"
          data-testid="text-transparency"
          min="0"
          max="1"
          step="0.05"
          value={current.transparency}
          oninput={(e) => patch({ transparency: Number(value(e)) })}
        />
      </label>
      <button type="button" data-testid="text-delete" onclick={remove}>{t("edit.text.delete")}</button>
    </div>
  {/if}
</div>

<style>
  .text-tool {
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
  .handle {
    position: absolute;
    width: 18px;
    height: 18px;
    margin: -9px 0 0 -9px;
    border: 2px dashed var(--fb-accent, #4a9eff);
    border-radius: 50%;
    background: rgba(255, 255, 255, 0.35);
    cursor: move;
  }
  .handle.selected {
    border-style: solid;
  }
  .controls {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex-wrap: wrap;
  }
  .fields {
    display: grid;
    gap: 0.4rem;
  }
  .fields label {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }
  .items {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 0.25rem;
  }
  button {
    padding: 0.25rem 0.75rem;
    border: 1px solid var(--fb-border, #ccc);
    background: var(--fb-surface, #fff);
    cursor: pointer;
  }
  button.selected {
    border-color: var(--fb-accent, #4a9eff);
  }
  .hint {
    color: var(--fb-text-dim, #777);
    font-size: 0.8rem;
  }
</style>

<!--
  Single-photo edit view (TASK-04-010, PLAN §2D "Edit view — single photo
  with edit tabs and filmstrip", §6 P2b). Opens over the grid like the
  viewer; shows the three edit tabs (Basic Fixes, Tuning, Effects), a
  filmstrip of the siblings, and a live preview.

  Live preview: the panels only record state; this component folds it into
  one v1 recipe (edit-recipe.ts), debounces, and asks the backend to render
  it at screen resolution in the bounded job queue (visible-first priority).
  The rendered pixels come back as a `preview://…?r=<recipe-id>` URI the
  WebView fetches straight from disk — no image bytes over IPC (PLAN §3
  rule 3). The original file is never modified (§9; Save is FEAT-04-06).
-->
<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { thumbUri, type GridItem } from "./grid";
  import { gridState } from "./grid-state.svelte.js";
  import { t } from "./i18n.svelte.js";
  import AutoFixPanel from "./AutoFixPanel.svelte";
  import CropStraightenPanel from "./CropStraightenPanel.svelte";
  import EffectsPanel from "./EffectsPanel.svelte";
  import RedEyePanel from "./RedEyePanel.svelte";
  import TuningPanel from "./TuningPanel.svelte";
  import {
    EDIT_TABS,
    EMPTY_EDIT_STATE as EMPTY,
    editHistory,
    isEmptyState,
    pushEditState,
    recipeFromState,
    redo,
    stateFromRecipe,
    undo,
    type EditHistory,
    type EditState,
    type EditTab,
  } from "./edit-recipe";
  import { isEditableTarget, nextIndex, previewUri, prevIndex } from "./viewer";

  interface Props {
    items?: GridItem[];
    /// Which photo the grid opened (index into `items`).
    startIndex?: number;
    onclose?: () => void;
  }

  let { items = [], startIndex = 0, onclose = () => {} }: Props = $props();

  let index = $state(Math.max(0, Math.min(startIndex, items.length - 1)));
  let tab = $state<EditTab>("basic");
  let history = $state<EditHistory>(editHistory(EMPTY));
  let previewSrc = $state<string | null>(null);
  let renderError = $state<string | null>(null);
  // Bump to remount the panels (photo change / undo / redo): the panels own
  // their internal state between emissions, so a fresh mount adopts the
  // restored state via their `initial` prop.
  let panelEpoch = $state(0);

  const current = $derived(items[index] ?? null);
  const baseUri = $derived(previewUri(current?.hash ?? null));
  const state = $derived(history.present);
  const filmstrip = $derived(
    items.map((item, i) => ({
      item,
      uri: thumbUri(item.hash, gridState.thumbVersions[item.hash] ?? 0) ?? "",
    })),
  );

  // --- Load an existing recipe when the photo changes ------------------------

  let loadSeq = 0;
  $effect(() => {
    const photo = current;
    if (photo === null) return;
    const seq = ++loadSeq;
    previewSrc = baseUri;
    invoke<string | null>("get_recipe", { fileId: photo.id })
      .then((raw) => {
        if (seq !== loadSeq) return; // photo moved on meanwhile
        let restored: EditState = EMPTY;
        if (raw) {
          try {
            restored = stateFromRecipe(JSON.parse(raw));
          } catch {
            restored = EMPTY; // unreadable recipe: start clean, never fail
          }
        }
        history = editHistory(restored);
        panelEpoch++;
        // Existing edits need their preview rendered; an untouched photo
        // just shows the original.
        if (!isEmptyState(restored)) scheduleRender();
      })
      .catch((e) => {
        if (seq === loadSeq) renderError = String(e);
      });
  });

  // --- Panel emissions → recipe → debounced render + persist -----------------

  function mergeTab(partial: Partial<EditState>): void {
    renderError = null;
    const next: EditState = { ...state, ...partial };
    history = pushEditState(history, next);
    scheduleRender();
  }

  let timer: ReturnType<typeof setTimeout> | undefined;
  let renderSeq = 0;

  function scheduleRender(): void {
    if (timer !== undefined) clearTimeout(timer);
    timer = setTimeout(() => void renderNow(), 250);
  }

  async function renderNow(): Promise<void> {
    const photo = current;
    if (photo === null) return;
    const recipe = recipeFromState(state);
    const recipeJson = JSON.stringify(recipe);
    const seq = ++renderSeq;
    try {
      // Persist the recipe and render share one debounced tick; the render
      // result decides what the <img> shows.
      invoke("set_recipe", { fileId: photo.id, recipe: recipeJson }).catch((e) => {
        if (seq === renderSeq) renderError = String(e);
      });
      if (isEmptyState(state)) {
        // No edits: the plain original preview is the preview.
        if (seq === renderSeq) previewSrc = baseUri;
        return;
      }
      const uri = await invoke<string>("render_edit_preview", {
        fileId: photo.id,
        recipe: recipeJson,
      });
      if (seq === renderSeq && uri) {
        previewSrc = uri;
        renderError = null;
      }
    } catch (e) {
      if (seq === renderSeq) renderError = String(e);
    }
  }

  // --- Undo / redo (session history; Ctrl/Cmd+Z, Shift for redo) -------------

  function doUndo(): void {
    const h = undo(history);
    if (h === null) return;
    history = h;
    panelEpoch++;
    scheduleRender();
  }

  function doRedo(): void {
    const h = redo(history);
    if (h === null) return;
    history = h;
    panelEpoch++;
    scheduleRender();
  }

  // --- Navigation -------------------------------------------------------------

  function goTo(i: number): void {
    index = Math.max(0, Math.min(i, items.length - 1));
  }

  function onKeydown(e: KeyboardEvent): void {
    if (isEditableTarget(e.target)) return;
    if ((e.metaKey || e.ctrlKey) && (e.key === "z" || e.key === "Z")) {
      e.preventDefault();
      if (e.shiftKey) doRedo();
      else doUndo();
      return;
    }
    switch (e.key) {
      case "ArrowRight":
        goTo(nextIndex(index, items.length));
        break;
      case "ArrowLeft":
        goTo(prevIndex(index, items.length));
        break;
      case "Escape":
        onclose();
        break;
    }
  }

  $effect(() => {
    window.addEventListener("keydown", onKeydown);
    return () => window.removeEventListener("keydown", onKeydown);
  });
</script>

<div class="edit-view" data-testid="edit-view" data-file-id={current?.id ?? ""}>
  <div class="edit-view-header">
    <div class="edit-view-tabs" role="tablist">
      {#each EDIT_TABS as tb (tb)}
        <button
          type="button"
          role="tab"
          class="edit-view-tab{tab === tb ? ' on' : ''}"
          data-testid={`edit-tab-${tb}`}
          aria-selected={tab === tb}
          onclick={() => (tab = tb)}
        >
          {t(`edit.view.tab.${tb}`)}
        </button>
      {/each}
    </div>
    <span class="edit-view-count" data-testid="edit-count">{index + 1}/{items.length}</span>
    <div class="edit-view-history">
      <button
        type="button"
        data-testid="edit-undo"
        title={t("edit.view.undo")}
        onclick={doUndo}
        disabled={history.past.length === 0}
      >
        ↶
      </button>
      <button
        type="button"
        data-testid="edit-redo"
        title={t("edit.view.redo")}
        onclick={doRedo}
        disabled={history.future.length === 0}
      >
        ↷
      </button>
    </div>
    <button
      type="button"
      class="edit-view-close"
      data-testid="edit-close"
      title={t("edit.view.close")}
      onclick={onclose}
    >
      ✕
    </button>
  </div>
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="edit-view-stage" data-testid="edit-stage">
    {#if previewSrc}
      <img
        class="edit-view-main"
        data-testid="edit-preview"
        src={previewSrc}
        alt={current?.filename ?? ""}
        draggable="false"
      />
    {:else}
      <div class="edit-view-placeholder" data-testid="edit-placeholder"></div>
    {/if}
    {#if renderError}
      <span class="edit-view-error" data-testid="edit-error" role="alert">
        {t("edit.view.renderFailed", { error: renderError })}
      </span>
    {/if}
  </div>
  {#key panelEpoch}
    <div class="edit-view-panel">
      {#if tab === "basic"}
        <CropStraightenPanel
          previewSrc={baseUri ?? ""}
          initial={state.crop}
          onchange={(crop) => mergeTab({ crop })}
        />
        <RedEyePanel
          previewSrc={baseUri ?? ""}
          initial={state.redEye}
          onchange={(redEye) => mergeTab({ redEye })}
        />
        <AutoFixPanel
          previewSrc={baseUri ?? ""}
          initial={state.autoFix}
          onchange={(autoFix) => mergeTab({ autoFix })}
        />
      {:else if tab === "tuning"}
        <TuningPanel
          previewSrc={baseUri ?? ""}
          initial={state.tuning}
          onchange={(tuning) => mergeTab({ tuning })}
        />
      {:else}
        <EffectsPanel
          previewSrc={baseUri ?? ""}
          initial={state.effects}
          onchange={(effects) => mergeTab({ effects })}
        />
      {/if}
    </div>
  {/key}
  <div class="edit-view-filmstrip" data-testid="edit-filmstrip">
    {#each filmstrip as entry, i (entry.item.id)}
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <img
        class="edit-view-thumb{i === index ? ' current' : ''}"
        data-testid="edit-filmstrip-thumb"
        src={entry.uri}
        alt={entry.item.filename}
        draggable="false"
        onclick={() => goTo(i)}
      />
    {/each}
  </div>
</div>

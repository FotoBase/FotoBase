<!--
  Batch rename / move dialog (TASK-03-011, PLAN §2 B "Batch rename / move").
  Rename: a name pattern with counter settings; Move: a destination folder.
  Both preview the backend's plan before anything touches disk, show
  collisions, and let the user refuse them or have them auto-resolved. Apply
  stays disabled until the plan is clean. The backend re-plans on apply and
  is all-or-nothing; its error is shown and the dialog stays open.
-->
<script lang="ts">
  import {
    baseName,
    batchMove,
    batchRename,
    DEFAULT_SPEC,
    isApplicable,
    previewBatchMove,
    previewBatchRename,
    type BatchPlan,
    type CollisionPolicy,
    type PlannedMove,
    type RenameSpec,
  } from "./batch";
  import type { FolderInfo } from "./folder-tree";
  import { t } from "./i18n.svelte.js";

  /// Rows drawn in the preview; the plan itself is never truncated.
  const MAX_ROWS = 200;
  const PREVIEW_DEBOUNCE_MS = 120;

  let {
    open = $bindable(false),
    mode,
    ids,
    folders = [],
    onapplied,
  }: {
    open?: boolean;
    mode: "rename" | "move";
    ids: number[];
    folders?: FolderInfo[];
    onapplied: (moves: PlannedMove[]) => void;
  } = $props();

  let pattern = $state(DEFAULT_SPEC.pattern);
  let start = $state(DEFAULT_SPEC.start);
  let step = $state(DEFAULT_SPEC.step);
  let pad = $state(DEFAULT_SPEC.pad);
  let policy = $state<CollisionPolicy>("refuse");
  let dest = $state("");
  let plan = $state<BatchPlan | null>(null);
  let pending = $state(false);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let latest = 0;

  const count = (v: unknown, max: number): number =>
    Math.min(max, Math.max(0, Math.trunc(Number(v) || 0)));

  function currentSpec(): RenameSpec {
    return {
      pattern,
      start: count(start, 1_000_000_000),
      step: count(step, 1_000_000),
      pad: count(pad, 12),
    };
  }

  // Re-seed the form every time the dialog opens.
  $effect(() => {
    if (!open) return;
    pattern = DEFAULT_SPEC.pattern;
    start = DEFAULT_SPEC.start;
    step = DEFAULT_SPEC.step;
    pad = DEFAULT_SPEC.pad;
    policy = "refuse";
    dest = "";
    plan = null;
    error = null;
  });

  // Preview: re-asked (debounced) whenever an input changes. A reply that
  // lost the race against a newer request is dropped.
  $effect(() => {
    if (!open) return;
    const spec = currentSpec();
    const chosen = policy;
    const target = dest;
    const selection = ids;
    const kind = mode;
    if (kind === "move" && target === "") {
      plan = null;
      pending = false;
      return;
    }
    pending = true;
    const mine = ++latest;
    const timer = setTimeout(async () => {
      try {
        const result =
          kind === "rename"
            ? await previewBatchRename(selection, spec, chosen)
            : await previewBatchMove(selection, target, chosen);
        if (mine !== latest) return;
        plan = result;
        error = null;
      } catch (e) {
        if (mine !== latest) return;
        plan = null;
        error = t("batch.failed", { error: String(e) });
      } finally {
        if (mine === latest) pending = false;
      }
    }, PREVIEW_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  });

  // Escape closes the open dialog, like the other overlays.
  $effect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") open = false;
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const canApply = $derived(!busy && !pending && plan !== null && isApplicable(plan));

  async function apply(): Promise<void> {
    if (!canApply) return;
    busy = true;
    error = null;
    try {
      const moves =
        mode === "rename"
          ? await batchRename(ids, currentSpec(), policy)
          : await batchMove(ids, dest, policy);
      open = false;
      onapplied(moves);
    } catch (e) {
      error = t("batch.failed", { error: String(e) });
    } finally {
      busy = false;
    }
  }

  const title = $derived(mode === "rename" ? t("batch.renameTitle") : t("batch.moveTitle"));
  const sortedFolders = $derived([...folders].sort((a, b) => a.path.localeCompare(b.path)));
</script>

{#if open}
  <div class="batch-backdrop" data-testid="batch-backdrop">
    <div class="batch-dialog" data-testid="batch-dialog" role="dialog" aria-label={title}>
      <form
        onsubmit={(e) => {
          e.preventDefault();
          void apply();
        }}
      >
        <h2>{title}</h2>
        {#if mode === "rename"}
          <label>
            {t("batch.pattern")}
            <input data-testid="batch-pattern" type="text" bind:value={pattern} />
            <small>{t("batch.patternHint")}</small>
          </label>
          <div class="batch-counter">
            <label>
              {t("batch.start")}
              <input data-testid="batch-start" type="number" min="0" bind:value={start} />
            </label>
            <label>
              {t("batch.step")}
              <input data-testid="batch-step" type="number" min="0" bind:value={step} />
            </label>
            <label>
              {t("batch.pad")}
              <input data-testid="batch-pad" type="number" min="0" max="12" bind:value={pad} />
            </label>
          </div>
        {:else}
          <label>
            {t("batch.folder")}
            <select data-testid="batch-folder" bind:value={dest}>
              <option value="">{t("batch.chooseFolder")}</option>
              {#each sortedFolders as folder (folder.id)}
                <option value={folder.path}>{folder.path}</option>
              {/each}
            </select>
          </label>
        {/if}
        <label>
          {t("batch.policy")}
          <select data-testid="batch-policy" bind:value={policy}>
            <option value="refuse">{t("batch.refuse")}</option>
            <option value="auto_suffix">{t("batch.autoSuffix")}</option>
          </select>
        </label>

        {#if plan}
          <ul class="batch-preview" data-testid="batch-preview">
            {#each plan.moves.slice(0, MAX_ROWS) as move (move.id)}
              <li data-testid="batch-row" title={move.to}>
                {baseName(move.from)} → {baseName(move.to)}
              </li>
            {/each}
            {#each plan.collisions.slice(0, MAX_ROWS) as c (c.id)}
              <li class="batch-collision" data-testid="batch-collision" title={c.to}>
                {baseName(c.from)} → {baseName(c.to)}: {t(`batch.collision.${c.reason}`)}
              </li>
            {/each}
          </ul>
          {#if plan.moves.length === 0 && plan.collisions.length === 0}
            <p data-testid="batch-nothing">{t("batch.nothingToDo")}</p>
          {/if}
        {/if}
        {#if error}
          <p class="batch-error" data-testid="batch-error" role="alert">{error}</p>
        {/if}
        <footer>
          <button
            type="button"
            data-testid="batch-cancel"
            onclick={() => {
              open = false;
            }}>{t("batch.cancel")}</button
          >
          <button type="submit" data-testid="batch-apply" disabled={!canApply}
            >{t("batch.apply")}</button
          >
        </footer>
      </form>
    </div>
  </div>
{/if}

<style>
  .batch-backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    background: rgb(0 0 0 / 40%);
    display: grid;
    place-items: center;
  }
  .batch-dialog {
    background: var(--fb-surface, #fff);
    color: var(--fb-text, #111);
    border-radius: 8px;
    min-width: 26rem;
    max-width: 90vw;
    padding: 1rem 1.25rem;
    box-shadow: 0 8px 32px rgb(0 0 0 / 30%);
  }
  form {
    display: grid;
    gap: 0.6rem;
  }
  h2 {
    margin: 0;
  }
  label {
    display: grid;
    gap: 0.2rem;
    font-size: 0.85rem;
  }
  small {
    color: var(--fb-muted, #666);
  }
  input,
  select {
    font: inherit;
    padding: 0.3rem 0.5rem;
    box-sizing: border-box;
    width: 100%;
  }
  .batch-counter {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 0.5rem;
  }
  .batch-preview {
    margin: 0;
    padding: 0.3rem 0.5rem;
    list-style: none;
    max-height: 14rem;
    overflow-y: auto;
    border: 1px solid var(--fb-border, #ccc);
    border-radius: 6px;
    font-size: 0.8rem;
  }
  .batch-collision,
  .batch-error {
    color: #b00020;
  }
  p {
    margin: 0;
  }
  footer {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }
</style>

<!--
  Batch edit dialog (TASK-04-011, PLAN §2E "Batch edit"): one §2E op applied
  to every photo in the selection (the Photo Tray or a grid multi-selection).
  Picking an op runs the batch immediately — the backend queues it through
  the job queue at background priority, writes each photo's sidecar recipe
  (the single-photo transform, so batch equals per-photo), streams a
  progress counter, and can be cancelled between photos: every processed
  photo keeps a valid recipe, unprocessed ones are untouched.
-->
<script lang="ts">
  import {
    batchEdit,
    cancelBatchEdit,
    BATCH_OPS,
    onBatchEditProgress,
    type BatchEditProgress,
    type BatchEditReport,
  } from "./batch-edit";
  import { t } from "./i18n.svelte.js";

  let {
    open = $bindable(false),
    ids,
    onapplied,
  }: {
    open?: boolean;
    ids: number[];
    onapplied: (report: BatchEditReport) => void;
  } = $props();

  let running = $state(false);
  let progress = $state<BatchEditProgress | null>(null);
  let report = $state<BatchEditReport | null>(null);
  let error = $state<string | null>(null);

  // Re-seed the dialog every time it opens.
  $effect(() => {
    if (!open) return;
    running = false;
    progress = null;
    report = null;
    error = null;
  });

  // Escape closes the open dialog unless a batch is writing sidecars.
  $effect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent): void => {
      e.stopPropagation();
      if (e.key === "Escape" && !running) open = false;
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });

  async function apply(op: string): Promise<void> {
    if (running || ids.length === 0) return;
    running = true;
    error = null;
    report = null;
    progress = null;
    let unlisten: () => void = () => {};
    try {
      // Progress is a nicety: a failed subscription never blocks the job.
      unlisten = await onBatchEditProgress((p) => (progress = p)).catch(() => () => {});
      const result = await batchEdit(ids, op);
      report = result;
      onapplied(result);
    } catch (e) {
      error = t("batchEdit.error", { error: String(e) });
    } finally {
      unlisten();
      running = false;
      progress = null;
    }
  }

  async function cancel(): Promise<void> {
    try {
      await cancelBatchEdit();
    } catch {
      // Best-effort: a finished batch cannot be cancelled.
    }
  }
</script>

{#if open}
  <div class="be-backdrop" data-testid="batch-edit-backdrop">
    <div
      class="be-dialog"
      data-testid="batch-edit-dialog"
      role="dialog"
      aria-label={t("batchEdit.title")}
    >
      <h2>{t("batchEdit.title")}</h2>
      <p class="be-count">{t("tray.count", { count: ids.length, held: ids.length })}</p>

      <div class="be-ops">
        {#each BATCH_OPS as op (op.id)}
          <button
            type="button"
            data-testid={`batch-edit-op-${op.id}`}
            disabled={running}
            onclick={() => void apply(op.id)}
          >
            {t(op.labelKey)}
          </button>
        {/each}
      </div>

      {#if progress}
        <p data-testid="batch-edit-progress">
          {t("batchEdit.progress", { done: progress.done, total: progress.total })}
        </p>
      {/if}
      {#if report}
        <p data-testid="batch-edit-done">
          {report.failed.length === 0
            ? t("batchEdit.done", { applied: report.applied })
            : t("batchEdit.doneFailed", { applied: report.applied, failed: report.failed.length })}
        </p>
      {/if}
      {#if report?.cancelled}
        <p data-testid="batch-edit-cancelled">{t("batchEdit.cancelledNote")}</p>
      {/if}
      {#if report && report.failed.length > 0}
        <ul class="be-failures" data-testid="batch-edit-failures">
          {#each report.failed as failure (failure.id)}
            <li>{t("batchEdit.failure", { error: failure.error })}</li>
          {/each}
        </ul>
      {/if}
      {#if error}
        <p class="be-error" data-testid="batch-edit-error" role="alert">{error}</p>
      {/if}

      <footer>
        {#if running}
          <button type="button" data-testid="batch-edit-cancel" onclick={() => void cancel()}>
            {t("batch.cancel")}
          </button>
        {:else}
          <button
            type="button"
            data-testid="batch-edit-close"
            onclick={() => (open = false)}
          >
            {t("batchEdit.close")}
          </button>
        {/if}
      </footer>
    </div>
  </div>
{/if}

<style>
  .be-backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    background: rgb(0 0 0 / 40%);
    display: grid;
    place-items: center;
  }
  .be-dialog {
    background: var(--fb-surface, #fff);
    color: var(--fb-text, #111);
    border-radius: 8px;
    min-width: 24rem;
    max-width: 90vw;
    max-height: 90vh;
    overflow-y: auto;
    padding: 1rem 1.25rem;
    box-shadow: 0 8px 32px rgb(0 0 0 / 30%);
    display: grid;
    gap: 0.6rem;
  }
  h2,
  p {
    margin: 0;
  }
  .be-count {
    font-size: 0.85rem;
    color: var(--fb-muted, #666);
  }
  .be-ops {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(9rem, 1fr));
    gap: 0.4rem;
  }
  .be-ops button {
    font: inherit;
    padding: 0.4rem 0.6rem;
  }
  .be-failures {
    margin: 0;
    padding-left: 1.2rem;
    font-size: 0.85rem;
    color: var(--fb-danger, #b00);
  }
  .be-error {
    color: var(--fb-danger, #b00);
    font-size: 0.85rem;
  }
  footer {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }
  footer button {
    font: inherit;
    padding: 0.35rem 0.9rem;
  }
</style>

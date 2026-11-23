<!--
  Backup Manager (TASK-08-014, PLAN §2L "Backup Pictures", §6 P4): pick an
  external destination and a schedule, back up now with progress, and see
  the sets already stored there. The first set is full; later sets hold only
  what changed. A full or unplugged destination stops cleanly — no set is
  recorded — and the error says why.

  Restore (TASK-08-015): each set can be restored. Replacing the current
  library needs an explicit confirmation; the backend validates the whole set
  before writing anything, so a corrupt set is rejected with the broken
  files named and the current library stays as it was.
-->
<script lang="ts">
  import {
    cancelBackup,
    formatBytes,
    getBackupConfig,
    listBackupSets,
    onBackupProgress,
    onRestoreProgress,
    restoreBackup,
    runBackup,
    setBackupConfig,
    SCHEDULES,
    type BackupProgress,
    type BackupReport,
    type BackupSchedule,
    type BackupSet,
    type RestoreProgress,
    type RestoreReport,
  } from "./backup";
  import { t } from "./i18n.svelte.js";

  let { open = $bindable(false) }: { open?: boolean } = $props();

  let destination = $state("");
  let schedule = $state<BackupSchedule>("off");
  let sets = $state<BackupSet[]>([]);
  let running = $state(false);
  let progress = $state<BackupProgress | null>(null);
  let report = $state<BackupReport | null>(null);
  let error = $state<string | null>(null);
  let restoring = $state(false);
  let restoreProgress = $state<RestoreProgress | null>(null);
  let restoreReport = $state<RestoreReport | null>(null);
  // Set the user asked to restore; non-null while the confirmation shows.
  let confirmSet = $state<BackupSet | null>(null);

  let busy = $derived(running || restoring);
  let canRun = $derived(!busy && destination.trim() !== "");

  $effect(() => {
    if (!open) return;
    report = null;
    restoreReport = null;
    confirmSet = null;
    error = null;
    progress = null;
    void load();
  });

  async function load(): Promise<void> {
    try {
      const config = await getBackupConfig();
      destination = config.destination ?? "";
      schedule = config.schedule;
      await refreshSets();
    } catch (e) {
      error = t("backup.error", { error: String(e) });
    }
  }

  async function refreshSets(): Promise<void> {
    const dest = destination.trim();
    sets = dest === "" ? [] : await listBackupSets(dest).catch(() => []);
  }

  async function saveConfig(): Promise<void> {
    const dest = destination.trim();
    await setBackupConfig({ destination: dest === "" ? null : dest, schedule });
  }

  async function choose(): Promise<void> {
    const { open: pick } = await import("@tauri-apps/plugin-dialog");
    const picked = await pick({ directory: true, multiple: false });
    if (typeof picked === "string") {
      destination = picked;
      await refreshSets();
    }
  }

  async function changeSchedule(e: Event): Promise<void> {
    schedule = (e.currentTarget as HTMLSelectElement).value as BackupSchedule;
    try {
      await saveConfig();
    } catch (err) {
      error = t("backup.error", { error: String(err) });
    }
  }

  async function run(): Promise<void> {
    if (!canRun) return;
    running = true;
    error = null;
    report = null;
    progress = null;
    let unlisten: () => void = () => {};
    try {
      await saveConfig();
      // Progress is a nicety: a failed subscription never blocks the run.
      unlisten = await onBackupProgress((p) => (progress = p)).catch(() => () => {});
      report = await runBackup(destination.trim());
      await refreshSets();
    } catch (e) {
      error = t("backup.error", { error: String(e) });
    } finally {
      unlisten();
      running = false;
      progress = null;
    }
  }

  async function restore(set: BackupSet): Promise<void> {
    confirmSet = null;
    restoring = true;
    error = null;
    report = null;
    restoreReport = null;
    restoreProgress = null;
    let unlisten: () => void = () => {};
    try {
      unlisten = await onRestoreProgress((p) => (restoreProgress = p)).catch(() => () => {});
      // The confirmation the user just gave is the overwrite consent.
      restoreReport = await restoreBackup(destination.trim(), set.id, true);
    } catch (e) {
      error = t("restore.error", { error: String(e) });
    } finally {
      unlisten();
      restoring = false;
      restoreProgress = null;
    }
  }

  async function cancel(): Promise<void> {
    try {
      await cancelBackup();
    } catch (e) {
      error = t("backup.error", { error: String(e) });
    }
  }
</script>

{#if open}
  <div class="bm-backdrop">
    <div class="bm-dialog" role="dialog" aria-label={t("backup.title")}>
      <h2>{t("backup.title")}</h2>
      <p>{t("backup.intro")}</p>

      <label class="bm-field">
        {t("backup.destination")}
        <span class="bm-row">
          <input
            type="text"
            data-testid="backup-destination"
            placeholder={t("backup.destination.placeholder")}
            bind:value={destination}
            onchange={() => void refreshSets()}
            disabled={busy}
          />
          <button type="button" data-testid="backup-choose" onclick={choose} disabled={busy}>
            {t("backup.choose")}
          </button>
        </span>
      </label>

      <label class="bm-field">
        {t("backup.schedule")}
        <select
          data-testid="backup-schedule"
          value={schedule}
          onchange={changeSchedule}
          disabled={busy}
        >
          {#each SCHEDULES as s (s)}
            <option value={s}>{t(`backup.schedule.${s}`)}</option>
          {/each}
        </select>
      </label>

      {#if running}
        <div class="bm-progress">
          <progress
            data-testid="backup-progress"
            value={progress?.done ?? 0}
            max={progress?.total || 1}
          ></progress>
          {#if progress}
            <span>{t("backup.progress", { done: progress.done, total: progress.total })}</span>
          {/if}
        </div>
      {/if}

      {#if restoring}
        <div class="bm-progress">
          <progress
            data-testid="backup-progress"
            value={restoreProgress?.done ?? 0}
            max={restoreProgress?.total || 1}
          ></progress>
          <span>
            {restoreProgress
              ? t("restore.progress", { done: restoreProgress.done, total: restoreProgress.total })
              : t("restore.running")}
          </span>
        </div>
      {/if}

      {#if restoreReport}
        <p class="bm-done" data-testid="restore-result">
          {t("restore.done", {
            restored: restoreReport.files_restored,
            unchanged: restoreReport.files_unchanged,
          })}
          {#if restoreReport.skipped.length > 0}
            {t("restore.skipped", { count: restoreReport.skipped.length })}
          {/if}
        </p>
      {/if}

      {#if confirmSet}
        <div class="bm-confirm" role="group" data-testid="restore-confirm">
          <p>
            {t("restore.confirm", {
              date: confirmSet.created_at.replace("T", " ").replace("Z", " UTC"),
            })}
          </p>
          <div class="bm-actions">
            <button
              type="button"
              data-testid="restore-confirm-no"
              onclick={() => (confirmSet = null)}
            >
              {t("restore.confirm.no")}
            </button>
            <button
              type="button"
              data-testid="restore-confirm-yes"
              onclick={() => restore(confirmSet!)}
            >
              {t("restore.confirm.yes")}
            </button>
          </div>
        </div>
      {/if}

      {#if report}
        <p class="bm-done" data-testid="backup-result">
          {t("backup.done", {
            copied: report.files_copied,
            reused: report.files_reused,
            bytes: formatBytes(report.bytes_copied),
          })}
        </p>
      {/if}
      {#if error}
        <p class="bm-error" role="alert">{error}</p>
      {/if}

      <h3>{t("backup.sets")}</h3>
      {#if sets.length === 0}
        <p class="bm-muted">{t("backup.sets.none")}</p>
      {:else}
        <table class="bm-sets">
          <thead>
            <tr>
              <th>{t("backup.sets.date")}</th>
              <th>{t("backup.sets.files")}</th>
              <th>{t("backup.sets.new")}</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {#each [...sets].reverse() as s (s.id)}
              <tr data-testid="backup-set">
                <td>{s.created_at.replace("T", " ").replace("Z", " UTC")}</td>
                <td>{s.files}</td>
                <td>{s.new_files} ({formatBytes(s.new_bytes)})</td>
                <td>
                  <button
                    type="button"
                    data-testid="backup-restore"
                    disabled={busy}
                    onclick={() => (confirmSet = s)}
                  >
                    {t("restore.action")}
                  </button>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}

      <div class="bm-actions">
        {#if running}
          <button type="button" data-testid="backup-cancel" onclick={cancel}>
            {t("backup.cancel")}
          </button>
        {/if}
        <button type="button" data-testid="backup-run" disabled={!canRun} onclick={run}>
          {running ? t("backup.running") : t("backup.run")}
        </button>
        <button
          type="button"
          data-testid="backup-close"
          disabled={busy}
          onclick={() => (open = false)}
        >
          {t("backup.close")}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .bm-backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
    background: var(--fb-backdrop, rgb(0 0 0 / 40%));
    display: grid;
    place-items: center;
  }
  .bm-dialog {
    background: var(--fb-surface, #fff);
    color: var(--fb-text, #111);
    border-radius: 8px;
    min-width: 30rem;
    max-width: 90vw;
    max-height: 90vh;
    overflow-y: auto;
    padding: 1rem 1.25rem;
    box-shadow: 0 8px 32px var(--fb-shadow, rgb(0 0 0 / 30%));
    text-align: left;
  }
  h2,
  h3,
  p {
    margin: 0 0 0.6rem;
  }
  h3 {
    font-size: 0.95rem;
    margin-top: 0.8rem;
  }
  .bm-field {
    display: grid;
    gap: 0.25rem;
    margin-bottom: 0.6rem;
  }
  .bm-row {
    display: flex;
    gap: 0.4rem;
  }
  .bm-row input {
    flex: 1;
  }
  .bm-progress {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.85rem;
  }
  .bm-progress progress {
    flex: 1;
  }
  .bm-done,
  .bm-muted {
    font-size: 0.85rem;
  }
  .bm-muted {
    color: var(--fb-muted, #6b6b6b);
  }
  .bm-error {
    color: var(--fb-error, #b00020);
    font-size: 0.85rem;
  }
  .bm-sets {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.85rem;
  }
  .bm-sets th,
  .bm-sets td {
    text-align: left;
    padding: 0.2rem 0.4rem;
    border-bottom: 1px solid var(--fb-border, #ddd);
  }
  .bm-confirm {
    border: 1px solid var(--fb-error, #b00020);
    border-radius: 6px;
    padding: 0.5rem 0.75rem;
    margin-bottom: 0.6rem;
    font-size: 0.85rem;
  }
  .bm-actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 0.8rem;
  }
</style>

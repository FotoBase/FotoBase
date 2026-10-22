<script lang="ts">
  // Folder Manager dialog (TASK-01-016, PLAN §2A / §2 Tools menu). Per
  // folder: Scan Once (scan now, unwatched), Scan Always (watched by the
  // notify watcher), Remove from catalog (confirmed; files stay on disk).
  // Every change applies immediately — the parent refreshes the library
  // through `onchanged`, so a newly scanned folder shows up at once. The
  // face-detection toggle is a disabled P3 placeholder (§2 Tools note).
  import { t } from "./i18n.svelte.js";
  import { removeFolder, setFolderMode, type FolderMode } from "./folder-manager";
  import type { FolderInfo } from "./folder-tree";

  let {
    open = $bindable(false),
    folders,
    onchanged,
  }: { open?: boolean; folders: FolderInfo[]; onchanged: () => void } = $props();

  const MODES: { value: FolderMode; labelKey: string }[] = [
    { value: "once", labelKey: "folderManager.mode.once" },
    { value: "always", labelKey: "folderManager.mode.always" },
  ];

  let confirmingRemove = $state<string | null>(null);
  let error = $state<string | null>(null);
  let busy = $state<string | null>(null);

  async function applyMode(folder: FolderInfo, mode: FolderMode): Promise<void> {
    error = null;
    busy = folder.path;
    try {
      await setFolderMode(folder.path, mode);
      onchanged();
    } catch (e) {
      error = t("folderManager.error", { error: String(e) });
    } finally {
      busy = null;
    }
  }

  async function doRemove(folder: FolderInfo): Promise<void> {
    error = null;
    busy = folder.path;
    try {
      await removeFolder(folder.path);
      confirmingRemove = null;
      onchanged();
    } catch (e) {
      error = t("folderManager.error", { error: String(e) });
    } finally {
      busy = null;
    }
  }
</script>

{#if open}
  <div class="folder-manager-backdrop" data-testid="folder-manager-backdrop">
    <div class="folder-manager-dialog" role="dialog" aria-label={t("folderManager.title")}>
      <h2>{t("folderManager.title")}</h2>
      <p class="folder-manager-hint">{t("folderManager.hint")}</p>

      {#if folders.length === 0}
        <p>{t("tree.empty")}</p>
      {:else}
        <ul class="folder-list">
          {#each folders as folder (folder.id)}
            <li class="folder-row" data-testid={`folder-row-${folder.path}`}>
              <span class="folder-path" title={folder.path}>{folder.path}</span>
              <label class="face-toggle">
                <input
                  type="checkbox"
                  disabled
                  data-testid={`face-toggle-${folder.path}`}
                  title={t("folderManager.face.placeholder")}
                />
                {t("folderManager.face")}
              </label>
              <select
                value={folder.scan_mode}
                disabled={busy === folder.path}
                onchange={(e) => applyMode(folder, e.currentTarget.value as FolderMode)}
                data-testid={`folder-mode-${folder.path}`}
              >
                {#each MODES as mode (mode.value)}
                  <option value={mode.value}>{t(mode.labelKey)}</option>
                {/each}
              </select>
              {#if confirmingRemove === folder.path}
                <span class="remove-confirm">
                  <span>{t("folderManager.remove.confirm")}</span>
                  <button
                    type="button"
                    disabled={busy === folder.path}
                    onclick={() => doRemove(folder)}
                    data-testid={`folder-confirm-remove-${folder.path}`}
                  >
                    {t("folderManager.remove.confirmYes")}
                  </button>
                  <button
                    type="button"
                    onclick={() => (confirmingRemove = null)}
                    data-testid={`folder-cancel-remove-${folder.path}`}
                  >
                    {t("folderManager.remove.confirmNo")}
                  </button>
                </span>
              {:else}
                <button
                  type="button"
                  disabled={busy === folder.path}
                  onclick={() => (confirmingRemove = folder.path)}
                  data-testid={`folder-remove-${folder.path}`}
                >
                  {t("folderManager.remove")}
                </button>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}

      {#if error}
        <p class="folder-manager-error" data-testid="folder-manager-error">{error}</p>
      {/if}

      <footer class="folder-manager-footer">
        <button type="button" onclick={() => (open = false)} data-testid="folder-manager-close">
          {t("options.close")}
        </button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .folder-manager-backdrop {
    position: fixed;
    inset: 0;
    background: rgb(0 0 0 / 40%);
    display: grid;
    place-items: center;
  }
  .folder-manager-dialog {
    background: var(--fb-surface, #fff);
    color: var(--fb-text, #111);
    border-radius: 8px;
    min-width: 34rem;
    max-width: 90vw;
    max-height: 80vh;
    overflow: auto;
    padding: 1rem 1.25rem;
    box-shadow: 0 8px 32px rgb(0 0 0 / 30%);
  }
  .folder-manager-hint {
    color: var(--fb-muted, #555);
    margin: 0.25rem 0 0.75rem;
  }
  .folder-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 0.4rem;
  }
  .folder-row {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.25rem 0;
    border-bottom: 1px solid rgb(128 128 128 / 20%);
  }
  .folder-path {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .face-toggle {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    color: var(--fb-muted, #555);
    white-space: nowrap;
  }
  .remove-confirm {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    white-space: nowrap;
  }
  .folder-manager-error {
    color: #b00020;
  }
  .folder-manager-footer {
    display: flex;
    justify-content: flex-end;
    margin-top: 1rem;
  }
</style>

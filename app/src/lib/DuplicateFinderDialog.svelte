<!--
  Near-duplicate finder (TASK-04-019, PLAN §2 2026 additions "Near-duplicate
  detection"): groups near-duplicates across the whole catalog for review.
  Keepers are the default — a member is only deleted when the user marks it
  AND confirms; the delete routes through the backend's OS-trash path, so
  "cancel" and closing the dialog delete nothing. Thumbnails come from the
  `thumb://` scheme like the grid (no image bytes over IPC).
-->
<script lang="ts">
  import { t } from "./i18n.svelte.js";
  import {
    canDelete,
    deleteDuplicateFiles,
    duplicateGroups,
    findDuplicates,
    markedIds,
    type DuplicateGroup,
  } from "./duplicates";

  let {
    open = $bindable(false),
    onapplied,
  }: {
    open?: boolean;
    /// At least one file was trashed: the shell re-reads the grid.
    onapplied: () => void;
  } = $props();

  let groups = $state<DuplicateGroup[]>([]);
  let scanning = $state(false);
  let loaded = $state(false);
  let error = $state<string | null>(null);
  let status = $state<string | null>(null);
  /// Per-group marks (file ids the user suggests deleting).
  let marks = $state<Map<number, Set<number>>>(new Map());
  /// Pending delete: group index whose marked subset awaits confirmation.
  let confirming = $state<number | null>(null);

  $effect(() => {
    if (!open) return;
    loaded = false;
    error = null;
    status = null;
    confirming = null;
    marks = new Map();
    groups = [];
    void (async () => {
      try {
        groups = await duplicateGroups();
      } catch (e) {
        error = t("dupes.error", { error: String(e) });
      } finally {
        loaded = true;
      }
    })();
  });

  async function run(): Promise<void> {
    if (scanning) return;
    scanning = true;
    error = null;
    try {
      const report = await findDuplicates();
      status = t("dupes.report", { hashed: report.hashed, failed: report.failed });
      groups = await duplicateGroups();
      marks = new Map();
    } catch (e) {
      console.error("DBG-RUN", e);
      error = t("dupes.error", { error: e instanceof Error ? (e.stack ?? String(e)) : String(e) });
    } finally {
      scanning = false;
    }
  }

  function toggle(groupIndex: number, id: number): void {
    const set = new Set(marks.get(groupIndex) ?? []);
    if (set.has(id)) {
      set.delete(id);
    } else {
      set.add(id);
    }
    marks = new Map(marks).set(groupIndex, set);
  }

  function mark(groupIndex: number): Set<number> {
    return marks.get(groupIndex) ?? new Set();
  }

  async function confirmDelete(): Promise<void> {
    if (confirming === null) return;
    const group = groups[confirming];
    const ids = markedIds(group, mark(confirming));
    if (ids.length === 0 || ids.length === group.file_ids.length) return;
    try {
      const deleted = await deleteDuplicateFiles(ids);
      groups = groups.filter((_, i) => i !== confirming);
      status = t("dupes.deleted", { n: deleted });
      onapplied();
    } catch (e) {
      error = t("dupes.error", { error: String(e) });
    } finally {
      confirming = null;
    }
  }

  function thumbUri(hash: string | null, version: number): string {
    // Same URI shape the grid uses; the version cache-busts after a delete.
    return hash ? `thumb://${hash}?v=${version}` : "";
  }
</script>

{#if open}
  <div class="df-backdrop" data-testid="dupes-backdrop">
    <div class="df-dialog" role="dialog" aria-label={t("dupes.title")}>
      <h2>{t("dupes.title")}</h2>
      {#if status}
        <p data-testid="dupes-status">{status}</p>
      {/if}
      {#if loaded && groups.length === 0 && !error && !scanning}
        <p data-testid="dupes-empty">{t("dupes.empty")}</p>
      {/if}
      {#if scanning}
        <p data-testid="dupes-scanning">{t("dupes.scanning")}</p>
      {/if}
      <ul>
        {#each groups as group, i (group.file_ids.join(","))}
          <li data-testid="dupe-group">
            <div class="df-members">
              {#each group.file_ids as id, m (id)}
                <div data-testid="dupe-member">
                  <button
                    type="button"
                    class="df-member"
                    class:marked={mark(i).has(id)}
                    data-testid="dupe-mark"
                    onclick={() => toggle(i, id)}
                    title={t("dupes.mark")}
                  >
                    {#if group.hashes[m]}
                      <img src={thumbUri(group.hashes[m], id)} alt={group.filenames[m]} />
                    {/if}
                    <span class="df-name">{group.filenames[m]}</span>
                    <span class="df-role">{mark(i).has(id) ? t("dupes.delete") : t("dupes.keep")}</span>
                  </button>
                </div>
              {/each}
            </div>
          </li>
        {/each}
      </ul>
      {#if error}
        <p class="df-error" role="alert">{error}</p>
      {/if}
      {#if confirming !== null}
        <div class="df-confirm" data-testid="dupes-confirm-box">
          <p>{t("dupes.confirm", { n: markedIds(groups[confirming], mark(confirming)).length })}</p>
          <div class="df-actions">
            <button type="button" data-testid="dupes-confirm" onclick={() => void confirmDelete()}>
              {t("dupes.confirmYes")}
            </button>
            <button type="button" data-testid="dupes-cancel-delete" onclick={() => (confirming = null)}>
              {t("dupes.confirmNo")}
            </button>
          </div>
        </div>
      {/if}
      <div class="df-actions">
        <button type="button" data-testid="dupes-run" disabled={scanning} onclick={() => void run()}>
          {t("dupes.run")}
        </button>
        {#each groups as group, i (group.file_ids.join(","))}
          {#if canDelete(group, mark(i))}
            <button
              type="button"
              data-testid="dupes-delete"
              onclick={() => (confirming = i)}
            >
              {t("dupes.deleteMarked", { n: markedIds(group, mark(i)).length })}
            </button>
          {/if}
        {/each}
        <button type="button" data-testid="dupes-close" onclick={() => (open = false)}>
          {t("dupes.close")}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .df-backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    background: var(--fb-backdrop, rgb(0 0 0 / 40%));
    display: grid;
    place-items: center;
  }
  .df-dialog {
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
  p {
    margin: 0 0 0.6rem;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 0.8rem;
  }
  .df-members {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
  }
  .df-member {
    display: grid;
    gap: 0.15rem;
    justify-items: center;
    padding: 0.3rem;
    border: 2px solid var(--fb-border, #ccc);
    border-radius: 6px;
    background: none;
    cursor: pointer;
  }
  .df-member.marked {
    border-color: var(--fb-error, #b00020);
  }
  .df-member img {
    width: 96px;
    height: 96px;
    object-fit: cover;
    border-radius: 4px;
  }
  .df-name {
    max-width: 7rem;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: 0.8rem;
  }
  .df-role {
    font-size: 0.7rem;
    color: var(--fb-muted, #6b6b6b);
  }
  .df-error {
    color: var(--fb-error, #b00020);
    font-size: 0.85rem;
  }
  .df-confirm {
    border-top: 1px solid var(--fb-border, #ccc);
    margin-top: 0.6rem;
    padding-top: 0.6rem;
  }
  .df-actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 0.8rem;
  }
</style>

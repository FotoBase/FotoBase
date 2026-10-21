<script lang="ts">
  // Folders view (TASK-01-015, PLAN §2B): the real on-disk folder tree,
  // grouped by year. Clicking a folder toggles it in a multi-select; the
  // resulting id list is the filter the grid renders. Right-click opens
  // the context-menu skeleton.
  import { folderName, groupFoldersByYear, type FolderInfo } from "./folder-tree";
  import { t } from "./i18n.svelte.js";

  let {
    folders,
    selected = [],
    onselect,
    onfoldercontextmenu,
  }: {
    folders: FolderInfo[];
    selected?: number[];
    onselect: (ids: number[]) => void;
    onfoldercontextmenu?: (e: { folder: FolderInfo; x: number; y: number }) => void;
  } = $props();

  const nodes = $derived(groupFoldersByYear(folders));

  // The component owns the multi-select between clicks (the parent's
  // round-trip is async); the prop stays the source of truth whenever the
  // parent changes it.
  let ids = $state<number[]>([]);
  $effect(() => {
    ids = [...selected];
  });

  function toggle(id: number): void {
    const set = new Set(ids);
    if (set.has(id)) {
      set.delete(id);
    } else {
      set.add(id);
    }
    ids = [...set];
    onselect(ids);
  }
</script>

<aside class="folder-tree" data-testid="folder-tree" aria-label={t("tree.label")}>
  {#if nodes.length === 0}
    <p data-testid="tree-empty">{t("tree.empty")}</p>
  {:else}
    {#each nodes as node (node.year ?? "unknown")}
      <section class="year-group">
        <h3 data-testid={node.year === null ? "year-unknown" : `year-${node.year}`}>
          {node.year === null ? t("tree.unknownYear") : node.year}
        </h3>
        <ul>
          {#each node.folders as folder (folder.id)}
            <li>
              <button
                type="button"
                data-testid={`folder-${folder.id}`}
                aria-pressed={ids.includes(folder.id)}
                onclick={() => toggle(folder.id)}
                oncontextmenu={(e) =>
                  onfoldercontextmenu?.({ folder, x: e.clientX, y: e.clientY })}
              >
                {folderName(folder.path)}
              </button>
            </li>
          {/each}
        </ul>
      </section>
    {/each}
  {/if}
</aside>

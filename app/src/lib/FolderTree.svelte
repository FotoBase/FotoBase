<script lang="ts">
  // Folders view (TASK-01-015, PLAN §2B): the real on-disk folder tree,
  // grouped by year. Clicking a folder toggles it in a multi-select; the
  // resulting id list is the filter the grid renders. Right-click opens
  // the context-menu skeleton. TASK-03-016: a toolbar picks the sort key,
  // reverse, and flat-vs-tree layout; the parent owns (and persists) them.
  import {
    DEFAULT_FOLDER_VIEW,
    FOLDER_SORTS,
    FOLDER_VIEWS,
    isFolderSortKey,
    sortFolders,
    type FolderViewOptions,
  } from "./folder-sort";
  import { folderName, groupFoldersByYear, type FolderInfo } from "./folder-tree";
  import { t } from "./i18n.svelte.js";

  let {
    folders,
    selected = [],
    onselect,
    onfoldercontextmenu,
    options = DEFAULT_FOLDER_VIEW,
    onoptionschange,
  }: {
    folders: FolderInfo[];
    selected?: number[];
    onselect: (ids: number[]) => void;
    onfoldercontextmenu?: (e: { folder: FolderInfo; x: number; y: number }) => void;
    options?: FolderViewOptions;
    onoptionschange?: (options: FolderViewOptions) => void;
  } = $props();

  const nodes = $derived(groupFoldersByYear(folders, options));
  const flat = $derived(sortFolders(folders, options.sort, options.reverse));

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

{#snippet folderButton(folder: FolderInfo)}
  <button
    type="button"
    data-testid={`folder-${folder.id}`}
    data-hidden={folder.hidden ? "true" : undefined}
    aria-pressed={ids.includes(folder.id)}
    title={folder.path}
    onclick={() => toggle(folder.id)}
    oncontextmenu={(e) => onfoldercontextmenu?.({ folder, x: e.clientX, y: e.clientY })}
  >
    {folderName(folder.path)}
  </button>
{/snippet}

<aside class="folder-tree" data-testid="folder-tree" aria-label={t("tree.label")}>
  <div class="folder-options">
    {#each FOLDER_VIEWS as v (v.value)}
      <button
        type="button"
        data-testid={`folder-view-${v.value}`}
        aria-pressed={options.view === v.value}
        onclick={() => onoptionschange?.({ ...options, view: v.value })}
      >
        {t(v.labelKey)}
      </button>
    {/each}
    <select
      data-testid="folder-sort"
      aria-label={t("tree.sort")}
      value={options.sort}
      onchange={(e) => {
        const sort = e.currentTarget.value;
        if (isFolderSortKey(sort)) onoptionschange?.({ ...options, sort });
      }}
    >
      {#each FOLDER_SORTS as o (o.value)}
        <option value={o.value}>{t(o.labelKey)}</option>
      {/each}
    </select>
    <button
      type="button"
      data-testid="folder-sort-reverse"
      aria-pressed={options.reverse}
      aria-label={t("tree.sort.reverse")}
      title={t("tree.sort.reverse")}
      onclick={() => onoptionschange?.({ ...options, reverse: !options.reverse })}
    >
      ⇅
    </button>
  </div>
  {#if folders.length === 0}
    <p data-testid="tree-empty">{t("tree.empty")}</p>
  {:else if options.view === "flat"}
    <ul class="folder-flat" data-testid="folder-flat">
      {#each flat as folder (folder.id)}
        <li>{@render folderButton(folder)}</li>
      {/each}
    </ul>
  {:else}
    {#each nodes as node (node.year ?? "unknown")}
      <section class="year-group">
        <h3 data-testid={node.year === null ? "year-unknown" : `year-${node.year}`}>
          {node.year === null ? t("tree.unknownYear") : node.year}
        </h3>
        <ul>
          {#each node.folders as folder (folder.id)}
            <li>
              {@render folderButton(folder)}
            </li>
          {/each}
        </ul>
      </section>
    {/each}
  {/if}
</aside>

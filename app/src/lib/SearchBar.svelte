<script lang="ts">
  // Live search bar (TASK-01-018, PLAN §2C "Live search bar filters while
  // typing"). Typing is debounced (tens of ms) into the shared grid store's
  // `searchQuery`; the shell re-queries FTS and the grid narrows live.
  // Clearing writes the store immediately — no pending debounce may
  // resurrect the old query after a clear.
  import { t } from "./i18n.svelte.js";
  import { gridState } from "./grid-state.svelte.js";

  /// Tens of ms per the task spec: fast enough to feel live, slow enough to
  /// collapse a typing burst into one query.
  const DEBOUNCE_MS = 80;

  let text = $state("");
  let timer: ReturnType<typeof setTimeout> | undefined;

  function oninput(event: Event & { currentTarget: HTMLInputElement }): void {
    text = event.currentTarget.value;
    clearTimeout(timer);
    timer = setTimeout(() => {
      gridState.searchQuery = text.trim();
    }, DEBOUNCE_MS);
  }

  function clear(): void {
    clearTimeout(timer);
    text = "";
    gridState.searchQuery = "";
  }
</script>

<div class="search-bar">
  <input
    type="search"
    data-testid="search-input"
    placeholder={t("search.placeholder")}
    aria-label={t("search.placeholder")}
    value={text}
    oninput={oninput}
  />
  {#if text !== ""}
    <button type="button" data-testid="search-clear" onclick={clear} title={t("search.clear")}>
      {t("search.clear")}
    </button>
  {/if}
</div>

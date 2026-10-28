<script lang="ts">
  // Filter buttons + date slider (TASK-03-014, PLAN §2 C). The toggles write
  // the shared grid store immediately; the slider is debounced (tens of ms)
  // so a drag collapses into one re-query. The shell's single query effect
  // reads the store, so grid and viewer filter identically. Each filter
  // clears in one click: the toggle itself, or the date "clear" button.
  import { t } from "./i18n.svelte.js";
  import { ALL_TIME, DATE_STEPS, dateStepLabel } from "./filters";
  import { gridState } from "./grid-state.svelte.js";

  /// Same tens-of-ms window as the search box.
  const DEBOUNCE_MS = 80;

  // Local slider position: follows the thumb instantly, the store follows
  // after the debounce. Re-synced when the store changes elsewhere.
  let position = $state(gridState.dateStep);
  let timer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    position = gridState.dateStep;
  });

  function onslide(event: Event & { currentTarget: HTMLInputElement }): void {
    position = Number(event.currentTarget.value);
    clearTimeout(timer);
    timer = setTimeout(() => {
      gridState.dateStep = position;
    }, DEBOUNCE_MS);
  }

  function clearDate(): void {
    clearTimeout(timer);
    position = ALL_TIME;
    gridState.dateStep = ALL_TIME;
  }
</script>

<div class="filter-bar" role="group" aria-label={t("filters.label")}>
  <button
    type="button"
    class="filter-toggle"
    data-testid="filter-starred"
    aria-pressed={gridState.starredOnly}
    onclick={() => (gridState.starredOnly = !gridState.starredOnly)}
  >
    {t("filters.starred")}
  </button>
  <button
    type="button"
    class="filter-toggle"
    data-testid="filter-videos"
    aria-pressed={gridState.videosOnly}
    onclick={() => (gridState.videosOnly = !gridState.videosOnly)}
  >
    {t("filters.videos")}
  </button>
  <label class="filter-date">
    <span class="filter-date-label" data-testid="filter-date-label">{dateStepLabel(position)}</span>
    <input
      type="range"
      data-testid="filter-date"
      min="0"
      max={DATE_STEPS.length - 1}
      step="1"
      value={position}
      aria-label={t("filters.date")}
      aria-valuetext={dateStepLabel(position)}
      oninput={onslide}
    />
  </label>
  {#if gridState.dateStep !== ALL_TIME}
    <button
      type="button"
      data-testid="filter-date-clear"
      onclick={clearDate}
      title={t("filters.clearDate")}
    >
      {t("filters.clearDate")}
    </button>
  {/if}
</div>

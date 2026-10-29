<!--
  Date/time adjustment dialog (TASK-03-019, PLAN §6 P2a). Three modes act on
  the photos in `ids`: shift the capture time by a delta, fix a wrong
  timezone, or merge a second camera by entering what both clocks showed at
  one moment. A preview (sampled — never the whole selection) shows
  before → after before anything is written. Applying runs as a background
  job: the backend writes each photo's sidecar then its catalog row, streams
  progress, and reports the photos it had to skip.
-->
<script lang="ts">
  import {
    adjustDateTime,
    formatCaptureTime,
    mergeShiftSeconds,
    onDateTimeProgress,
    parseOffset,
    previewDateTime,
    shiftSeconds,
    type AdjustReport,
    type DateTimeProgress,
    type TimeAdjustment,
    type TimePreview,
  } from "./datetime";
  import { t } from "./i18n.svelte.js";

  /// Photos previewed; the adjustment itself always covers the whole selection.
  const PREVIEW_SAMPLE = 20;
  const PREVIEW_DEBOUNCE_MS = 120;

  type Mode = "shift" | "timezone" | "merge";

  let {
    open = $bindable(false),
    ids,
    onapplied,
  }: {
    open?: boolean;
    ids: number[];
    onapplied: (report: AdjustReport) => void;
  } = $props();

  let mode = $state<Mode>("shift");
  let direction = $state<"later" | "earlier">("later");
  let days = $state<number | null>(0);
  let hours = $state<number | null>(0);
  let minutes = $state<number | null>(0);
  let seconds = $state<number | null>(0);
  let offsetText = $state("");
  let keep = $state<"moment" | "clock">("moment");
  let reference = $state("");
  let other = $state("");

  let rows = $state<TimePreview[]>([]);
  let running = $state(false);
  let progress = $state<DateTimeProgress | null>(null);
  let report = $state<AdjustReport | null>(null);
  let error = $state<string | null>(null);
  let latest = 0;

  // Re-seed the form every time the dialog opens.
  $effect(() => {
    if (!open) return;
    mode = "shift";
    direction = "later";
    days = hours = minutes = seconds = 0;
    offsetText = "";
    keep = "moment";
    reference = "";
    other = "";
    rows = [];
    running = false;
    progress = null;
    report = null;
    error = null;
  });

  const offset = $derived(parseOffset(offsetText));
  const mergeSeconds = $derived(mergeShiftSeconds(reference, other));

  /// The adjustment the form describes; `null` while it is incomplete or
  /// would change nothing.
  const adjustment = $derived.by((): TimeAdjustment | null => {
    if (mode === "shift") {
      const total = shiftSeconds({
        direction,
        days: days ?? 0,
        hours: hours ?? 0,
        minutes: minutes ?? 0,
        seconds: seconds ?? 0,
      });
      return total === 0 ? null : { kind: "shift", seconds: total };
    }
    if (mode === "timezone") {
      if (offset === null) return null;
      return keep === "moment"
        ? { kind: "convert_offset", offset_minutes: offset }
        : { kind: "relabel_offset", offset_minutes: offset };
    }
    return mergeSeconds === null || mergeSeconds === 0
      ? null
      : { kind: "shift", seconds: mergeSeconds };
  });

  // Preview: re-asked (debounced) whenever the adjustment changes; a reply
  // that lost the race against a newer one is dropped.
  $effect(() => {
    if (!open) return;
    const current = adjustment;
    const sample = ids.slice(0, PREVIEW_SAMPLE);
    if (current === null || sample.length === 0) {
      rows = [];
      return;
    }
    const mine = ++latest;
    const timer = setTimeout(async () => {
      try {
        const result = await previewDateTime(sample, current);
        if (mine === latest) rows = result;
      } catch (e) {
        if (mine !== latest) return;
        rows = [];
        error = t("datetime.error", { error: String(e) });
      }
    }, PREVIEW_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  });

  // Escape closes the open dialog unless a job is writing sidecars. Modal:
  // the capture-phase listener keeps every key away from the viewer / grid
  // shortcuts underneath (the info pane lives inside the viewer, whose own
  // Escape would otherwise close it too).
  $effect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent): void => {
      e.stopPropagation();
      if (e.key === "Escape" && !running) open = false;
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });

  const canApply = $derived(!running && report === null && adjustment !== null && ids.length > 0);
  const offsetInvalid = $derived(mode === "timezone" && offsetText.trim() !== "" && offset === null);

  async function apply(): Promise<void> {
    const current = adjustment;
    if (!canApply || current === null) return;
    running = true;
    error = null;
    progress = null;
    let unlisten: () => void = () => {};
    try {
      // Progress is a nicety: a failed subscription never blocks the job.
      unlisten = await onDateTimeProgress((p) => (progress = p)).catch(() => () => {});
      const result = await adjustDateTime(ids, current);
      report = result;
      onapplied(result);
    } catch (e) {
      error = t("datetime.error", { error: String(e) });
    } finally {
      unlisten();
      running = false;
      progress = null;
    }
  }

  const skipped = $derived(rows.filter((r) => r.error !== null));
  const shown = $derived(rows.filter((r) => r.error === null));
</script>

{#if open}
  <div class="dt-backdrop" data-testid="dt-backdrop">
    <div class="dt-dialog" data-testid="dt-dialog" role="dialog" aria-label={t("datetime.title")}>
      <form
        onsubmit={(e) => {
          e.preventDefault();
          void apply();
        }}
      >
        <h2>{t("datetime.title")}</h2>
        <p class="dt-count">{t("datetime.count", { count: ids.length })}</p>

        <fieldset disabled={running || report !== null}>
          <legend>{t("datetime.mode")}</legend>
          {#each ["shift", "timezone", "merge"] as const as m (m)}
            <label class="dt-radio">
              <input
                type="radio"
                name="dt-mode"
                value={m}
                bind:group={mode}
                data-testid={`dt-mode-${m}`}
              />
              {t(`datetime.mode.${m}`)}
            </label>
          {/each}
        </fieldset>

        <fieldset disabled={running || report !== null}>
          {#if mode === "shift"}
            <div class="dt-grid" data-testid="dt-shift">
              <label>
                {t("datetime.shift.direction")}
                <select data-testid="dt-direction" bind:value={direction}>
                  <option value="later">{t("datetime.shift.later")}</option>
                  <option value="earlier">{t("datetime.shift.earlier")}</option>
                </select>
              </label>
              <label>
                {t("datetime.shift.days")}
                <input data-testid="dt-days" type="number" min="0" bind:value={days} />
              </label>
              <label>
                {t("datetime.shift.hours")}
                <input data-testid="dt-hours" type="number" min="0" bind:value={hours} />
              </label>
              <label>
                {t("datetime.shift.minutes")}
                <input data-testid="dt-minutes" type="number" min="0" bind:value={minutes} />
              </label>
              <label>
                {t("datetime.shift.seconds")}
                <input data-testid="dt-seconds" type="number" min="0" bind:value={seconds} />
              </label>
            </div>
          {:else if mode === "timezone"}
            <label>
              {t("datetime.tz.offset")}
              <input
                data-testid="dt-offset"
                type="text"
                placeholder="+02:00"
                bind:value={offsetText}
              />
              <small>{t("datetime.tz.hint")}</small>
            </label>
            {#if offsetInvalid}
              <p class="dt-error" data-testid="dt-offset-error" role="alert">
                {t("datetime.tz.invalid")}
              </p>
            {/if}
            <label class="dt-radio">
              <input
                type="radio"
                name="dt-keep"
                value="moment"
                bind:group={keep}
                data-testid="dt-keep-moment"
              />
              {t("datetime.tz.keepMoment")}
            </label>
            <label class="dt-radio">
              <input
                type="radio"
                name="dt-keep"
                value="clock"
                bind:group={keep}
                data-testid="dt-keep-clock"
              />
              {t("datetime.tz.keepClock")}
            </label>
          {:else}
            <small>{t("datetime.merge.hint")}</small>
            <label>
              {t("datetime.merge.reference")}
              <input
                data-testid="dt-reference"
                type="datetime-local"
                step="1"
                bind:value={reference}
              />
            </label>
            <label>
              {t("datetime.merge.other")}
              <input data-testid="dt-other" type="datetime-local" step="1" bind:value={other} />
            </label>
            {#if mergeSeconds !== null}
              <small data-testid="dt-merge-shift"
                >{t("datetime.merge.shift", { seconds: mergeSeconds })}</small
              >
            {/if}
          {/if}
        </fieldset>

        {#if adjustment === null && !offsetInvalid}
          <p data-testid="dt-nothing">{t("datetime.nothing")}</p>
        {/if}

        {#if shown.length > 0 || skipped.length > 0}
          <section>
            <h3>{t("datetime.preview")}</h3>
            <ul class="dt-preview" data-testid="dt-preview">
              {#each shown as row (row.id)}
                <li data-testid="dt-row">
                  {formatCaptureTime(row.before)} → {formatCaptureTime(row.after)}
                </li>
              {/each}
              {#each skipped as row (row.id)}
                <li class="dt-error" data-testid="dt-skipped">
                  {t("datetime.preview.skipped", { error: row.error ?? "" })}
                </li>
              {/each}
              {#if ids.length > rows.length}
                <li data-testid="dt-more">{t("datetime.preview.more", { n: ids.length - rows.length })}</li>
              {/if}
            </ul>
          </section>
        {/if}

        {#if progress}
          <p data-testid="dt-progress">
            {t("datetime.running", { done: progress.done, total: progress.total })}
          </p>
        {/if}
        {#if report}
          <p data-testid="dt-result">
            {report.failed.length === 0
              ? t("datetime.done", { adjusted: report.adjusted })
              : t("datetime.doneFailed", {
                  adjusted: report.adjusted,
                  failed: report.failed.length,
                })}
          </p>
        {/if}
        {#if error}
          <p class="dt-error" data-testid="dt-error" role="alert">{error}</p>
        {/if}

        <footer>
          <button
            type="button"
            data-testid="dt-cancel"
            disabled={running}
            onclick={() => {
              open = false;
            }}>{report ? t("datetime.close") : t("datetime.cancel")}</button
          >
          <button type="submit" data-testid="dt-apply" disabled={!canApply}
            >{t("datetime.apply")}</button
          >
        </footer>
      </form>
    </div>
  </div>
{/if}

<style>
  .dt-backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    background: rgb(0 0 0 / 40%);
    display: grid;
    place-items: center;
  }
  .dt-dialog {
    background: var(--fb-surface, #fff);
    color: var(--fb-text, #111);
    border-radius: 8px;
    min-width: 26rem;
    max-width: 90vw;
    max-height: 90vh;
    overflow-y: auto;
    padding: 1rem 1.25rem;
    box-shadow: 0 8px 32px rgb(0 0 0 / 30%);
  }
  form {
    display: grid;
    gap: 0.6rem;
  }
  h2,
  h3,
  p {
    margin: 0;
  }
  h3 {
    font-size: 0.9rem;
  }
  fieldset {
    display: grid;
    gap: 0.5rem;
    margin: 0;
    padding: 0;
    border: 0;
    min-width: 0;
  }
  legend {
    font-size: 0.85rem;
    padding: 0;
    margin-bottom: 0.25rem;
  }
  label {
    display: grid;
    gap: 0.2rem;
    font-size: 0.85rem;
  }
  .dt-radio {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }
  .dt-radio input {
    width: auto;
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
  .dt-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(5.5rem, 1fr));
    gap: 0.5rem;
  }
  .dt-preview {
    margin: 0;
    padding: 0.3rem 0.5rem;
    list-style: none;
    max-height: 12rem;
    overflow-y: auto;
    border: 1px solid var(--fb-border, #ccc);
    border-radius: 6px;
    font-size: 0.8rem;
  }
  .dt-error {
    color: #b00020;
  }
  footer {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }
</style>

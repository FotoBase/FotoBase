<!--
  Print dialog (TASK-08-004, PLAN §2J Print): layout (wallet, 3.5×5, 4×6,
  5×7, 8×10, full page, contact sheet with images-per-page), shrink-to-fit
  vs crop-to-fit, border and text options, copies per photo, and a live
  paper preview. The preview is drawn from the backend page plan (the same
  plan the final page raster uses) with the edit-view renders as images, so
  it shows the edits and cannot disagree with the paper. TASK-08-005: the
  destination is an installed printer, a PDF, or (TASK-08-004) PNG pages —
  all fed by that same plan; a printer that does not take the chosen paper
  warns before printing, and a running job shows its sheet progress and can
  be cancelled.
-->
<script lang="ts">
  import { t } from "./i18n.svelte.js";
  import {
    DEFAULT_OPTIONS,
    LAYOUTS,
    MAX_COPIES,
    MAX_IMAGES_PER_PAGE,
    cancelPrint,
    clampInt,
    exportPrintPdf,
    fontStack,
    listPrinters,
    onPrintProgress,
    pageRectStyle,
    photoPreviewUri,
    photoStyle,
    printPages,
    printPlan,
    printToPrinter,
    type PrintBatch,
    type PrintOptions,
    type PrintPlan,
    type Printer,
  } from "./print";

  let {
    open = $bindable(false),
    ids = $bindable<number[]>([]),
  }: { open?: boolean; ids?: number[] } = $props();

  // Preview scale: CSS pixels per inch of paper.
  const PREVIEW_PPI = 34;
  const FONTS = ["sans", "serif", "mono"] as const;

  let options = $state<PrintOptions>(structuredClone(DEFAULT_OPTIONS));
  let copies = $state(1);
  let plan = $state<PrintPlan | null>(null);
  let uris = $state<Record<number, string>>({});
  let folder = $state("");
  let pdfPath = $state("");
  // "printer:<name>", "pdf" or "pages".
  let destination = $state("pages");
  let printers = $state<Printer[]>([]);
  let busy = $state(false);
  let sheetsDone = $state(0);
  let doneText = $state<string | null>(null);
  let failed = $state(0);
  let error = $state<string | null>(null);

  let generation = 0;
  const requested = new Set<string>();

  let pageCount = $derived(plan?.pages.length ?? 0);
  let printer = $derived(
    destination.startsWith("printer:")
      ? printers.find((p) => p.name === destination.slice("printer:".length))
      : undefined,
  );
  let paperMismatch = $derived(printer?.mismatch.includes(options.paper) ?? false);
  let target = $derived(
    destination === "pdf" ? pdfPath : destination === "pages" ? folder : (printer?.name ?? ""),
  );
  let canRun = $derived(!busy && plan !== null && target.trim() !== "" && ids.length > 0);

  // The installed printers are read on every opening; the default printer is
  // the initial destination, the PNG pages when there is none.
  $effect(() => {
    if (!open) return;
    let stale = false;
    listPrinters().then((list) => {
      if (stale) return;
      printers = list;
      const initial = list.find((p) => p.is_default) ?? list[0];
      destination = initial ? `printer:${initial.name}` : "pages";
    });
    return () => {
      stale = true;
    };
  });

  // Re-plan whenever an option, the copies or the selection changes; a
  // slow answer for an older state is dropped.
  $effect(() => {
    if (!open || ids.length === 0) return;
    const snapshot: PrintOptions = $state.snapshot(options) as PrintOptions;
    snapshot.images_per_page = clampInt(snapshot.images_per_page, 1, MAX_IMAGES_PER_PAGE);
    snapshot.border.width_in = Math.max(0, Number(snapshot.border.width_in) || 0);
    const count = clampInt(copies, 1, MAX_COPIES);
    const selection = [...ids];
    const mine = ++generation;
    printPlan(selection, count, snapshot)
      .then((next) => {
        if (mine !== generation) return;
        plan = next;
        error = null;
        for (const photo of next.photos) {
          const key = `${photo.file_id}:${photo.recipe}`;
          if (requested.has(key)) continue;
          requested.add(key);
          photoPreviewUri(photo)
            .then((uri) => {
              uris[photo.file_id] = uri;
            })
            .catch(() => requested.delete(key));
        }
      })
      .catch((e) => {
        if (mine !== generation) return;
        error = t("print.error", { error: e instanceof Error ? e.message : String(e) });
      });
  });

  // Edits may change between openings: drop the cached previews on close.
  $effect(() => {
    if (open) return;
    generation++;
    plan = null;
    uris = {};
    requested.clear();
    doneText = null;
    sheetsDone = 0;
    failed = 0;
    error = null;
  });

  function sentence(batch: PrintBatch): string {
    if (destination === "pdf") {
      return t("print.pdfDone", { n: batch.pages, file: batch.files[0] ?? pdfPath.trim() });
    }
    if (printer) {
      return batch.job_id
        ? t("print.sentJob", { n: batch.pages, printer: printer.name, job: batch.job_id })
        : t("print.sent", { n: batch.pages, printer: printer.name });
    }
    return t("print.done", { n: batch.files.length });
  }

  async function run(): Promise<void> {
    if (!canRun) return;
    busy = true;
    error = null;
    doneText = null;
    sheetsDone = 0;
    const count = clampInt(copies, 1, MAX_COPIES);
    const snapshot = $state.snapshot(options) as PrintOptions;
    const unlisten = await onPrintProgress((done) => (sheetsDone = done)).catch(() => () => {});
    try {
      const batch =
        destination === "pdf"
          ? await exportPrintPdf(ids, pdfPath.trim(), count, snapshot)
          : printer
            ? await printToPrinter(ids, printer.name, count, snapshot)
            : await printPages(ids, folder.trim(), count, snapshot);
      doneText = sentence(batch);
      failed = batch.failed.length;
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      error = /cancelled/i.test(message) ? t("print.cancelled") : t("print.error", { error: message });
    } finally {
      unlisten();
      busy = false;
    }
  }

  function runLabel(): string {
    if (destination === "pdf") return busy ? t("print.runningPdf") : t("print.runPdf");
    if (printer) {
      return busy ? t("print.runningPrint") : paperMismatch ? t("print.runAnyway") : t("print.runPrint");
    }
    return busy ? t("print.running") : t("print.run");
  }
</script>

{#if open}
  <div class="pr-backdrop" data-testid="print-backdrop">
    <div class="pr-dialog" role="dialog" aria-label={t("print.title")}>
      <h2>{t("print.title")}</h2>

      <div class="pr-body">
        <div class="pr-options">
          <label>
            {t("print.paper")}
            <select data-testid="print-paper" bind:value={options.paper} disabled={busy}>
              <option value="letter">{t("print.paper.letter")}</option>
              <option value="a4">{t("print.paper.a4")}</option>
            </select>
          </label>

          <label>
            {t("print.layout")}
            <select data-testid="print-layout" bind:value={options.layout} disabled={busy}>
              {#each LAYOUTS as layout (layout)}
                <option value={layout}>{t(`print.layout.${layout}`)}</option>
              {/each}
            </select>
          </label>

          {#if options.layout === "contact_sheet"}
            <label>
              {t("print.perPage")}
              <input
                type="number"
                min="1"
                max={MAX_IMAGES_PER_PAGE}
                data-testid="print-per-page"
                bind:value={options.images_per_page}
                disabled={busy}
              />
            </label>
          {/if}

          <fieldset>
            <legend>{t("print.fit")}</legend>
            <label>
              <input
                type="radio"
                name="print-fit"
                value="shrink"
                data-testid="print-fit-shrink"
                bind:group={options.fit}
                disabled={busy}
              />
              {t("print.fit.shrink")}
            </label>
            <label>
              <input
                type="radio"
                name="print-fit"
                value="crop"
                data-testid="print-fit-crop"
                bind:group={options.fit}
                disabled={busy}
              />
              {t("print.fit.crop")}
            </label>
          </fieldset>

          <label>
            <input type="checkbox" bind:checked={options.auto_rotate} disabled={busy} />
            {t("print.rotate")}
          </label>

          <label>
            {t("print.copies")}
            <input
              type="number"
              min="1"
              max={MAX_COPIES}
              data-testid="print-copies"
              bind:value={copies}
              disabled={busy}
            />
          </label>

          <fieldset>
            <legend>{t("print.border")}</legend>
            <label>
              {t("print.border.width")}
              <input
                type="number"
                min="0"
                max="0.5"
                step="0.05"
                data-testid="print-border-width"
                bind:value={options.border.width_in}
                disabled={busy}
              />
            </label>
            <label>
              {t("print.color")}
              <input type="color" bind:value={options.border.color} disabled={busy} />
            </label>
          </fieldset>

          <fieldset>
            <legend>{t("print.text")}</legend>
            <label>
              {t("print.text.source")}
              <select data-testid="print-text-source" bind:value={options.text.source} disabled={busy}>
                <option value="none">{t("print.text.none")}</option>
                <option value="caption">{t("print.text.caption")}</option>
                <option value="filename">{t("print.text.filename")}</option>
                <option value="exif">{t("print.text.exif")}</option>
              </select>
            </label>
            {#if options.text.source !== "none"}
              <label>
                {t("print.text.position")}
                <select
                  data-testid="print-text-position"
                  bind:value={options.text.position}
                  disabled={busy}
                >
                  <option value="above">{t("print.text.above")}</option>
                  <option value="below">{t("print.text.below")}</option>
                </select>
              </label>
              <label>
                {t("print.text.font")}
                <select bind:value={options.text.font} disabled={busy}>
                  {#each FONTS as f (f)}
                    <option value={f}>{f}</option>
                  {/each}
                </select>
              </label>
              <label>
                {t("print.text.size")}
                <input
                  type="number"
                  min="4"
                  max="72"
                  data-testid="print-text-size"
                  bind:value={options.text.size_pt}
                  disabled={busy}
                />
              </label>
              <label>
                {t("print.color")}
                <input type="color" bind:value={options.text.color} disabled={busy} />
              </label>
              <label>
                <input
                  type="checkbox"
                  data-testid="print-text-wrap"
                  bind:checked={options.text.wrap}
                  disabled={busy}
                />
                {t("print.text.wrap")}
              </label>
            {/if}
          </fieldset>
        </div>

        <div class="pr-preview" aria-label={t("print.preview")}>
          <p class="pr-count" data-testid="print-page-count">{t("print.pages", { n: pageCount })}</p>
          <div class="pr-pages">
            {#each plan?.pages ?? [] as page, p (p)}
              <div
                class="pr-page"
                data-testid="print-page"
                style="width:{page.width * PREVIEW_PPI}px;height:{page.height * PREVIEW_PPI}px"
              >
                {#each page.cells as cell, c (c)}
                  <div class="pr-cell" data-testid="print-cell">
                    <div class="pr-guide" style={pageRectStyle(cell.cell, page)}></div>
                    {#if cell.border}
                      <div
                        class="pr-border"
                        data-testid="print-border"
                        style="{pageRectStyle(cell.border, page)};background:{options.border.color}"
                      ></div>
                    {/if}
                    <div class="pr-clip" style={pageRectStyle(cell.fit.dst, page)}>
                      {#if uris[cell.file_id]}
                        <img
                          src={uris[cell.file_id]}
                          alt={plan?.photos.find((ph) => ph.file_id === cell.file_id)?.filename ?? ""}
                          style={photoStyle(cell)}
                        />
                      {/if}
                    </div>
                    {#if cell.text}
                      <div
                        class="pr-text"
                        style="{pageRectStyle(cell.text.rect, page)};color:{options.text
                          .color};font-family:{fontStack(options.text.font)};font-size:{(Math.max(
                          4,
                          options.text.size_pt,
                        ) /
                          72) *
                          PREVIEW_PPI}px"
                      >{cell.text.text}</div>
                    {/if}
                  </div>
                {/each}
              </div>
            {/each}
          </div>
        </div>
      </div>

      <label class="pr-folder">
        {t("print.destination")}
        <select data-testid="print-destination" bind:value={destination} disabled={busy}>
          {#each printers as p (p.name)}
            <option value={`printer:${p.name}`}>
              {p.is_default ? t("print.destination.default", { name: p.name }) : p.name}
            </option>
          {/each}
          <option value="pdf">{t("print.destination.pdf")}</option>
          <option value="pages">{t("print.destination.pages")}</option>
        </select>
      </label>

      {#if destination === "pdf"}
        <label class="pr-folder">
          {t("print.pdfPath")}
          <input
            type="text"
            data-testid="print-pdf-path"
            placeholder={t("print.pdfPath.placeholder")}
            bind:value={pdfPath}
            disabled={busy}
          />
        </label>
      {:else if destination === "pages"}
        <label class="pr-folder">
          {t("print.folder")}
          <input
            type="text"
            data-testid="print-folder"
            placeholder={t("print.folder.placeholder")}
            bind:value={folder}
            disabled={busy}
          />
        </label>
      {/if}

      {#if printer && paperMismatch}
        <p class="pr-error" role="alert" data-testid="print-paper-warning">
          {t("print.paperWarning", {
            printer: printer.name,
            paper: t(`print.paper.${options.paper}`),
          })}
        </p>
      {/if}

      {#if busy}
        <p data-testid="print-progress">
          {t("print.progress", { done: sheetsDone, total: pageCount })}
        </p>
      {/if}

      {#if doneText !== null}
        <p data-testid="print-done">{doneText}</p>
        {#if failed > 0}
          <p class="pr-error" role="alert">{t("print.partial", { n: failed })}</p>
        {/if}
      {/if}
      {#if error}
        <p class="pr-error" role="alert" data-testid="print-error">{error}</p>
      {/if}

      <div class="pr-actions">
        <button type="button" data-testid="print-run" disabled={!canRun} onclick={run}>
          {runLabel()}
        </button>
        {#if busy}
          <button type="button" data-testid="print-cancel" onclick={() => void cancelPrint()}>
            {t("print.cancel")}
          </button>
        {/if}
        <button type="button" data-testid="print-close" onclick={() => (open = false)}>
          {t("print.close")}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .pr-backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
    background: var(--fb-backdrop, rgb(0 0 0 / 40%));
    display: grid;
    place-items: center;
  }
  .pr-dialog {
    background: var(--fb-surface, #fff);
    color: var(--fb-text, #111);
    border-radius: 8px;
    max-width: 95vw;
    max-height: 92vh;
    overflow-y: auto;
    padding: 1rem 1.25rem;
    box-shadow: 0 8px 32px var(--fb-shadow, rgb(0 0 0 / 30%));
    text-align: left;
    display: grid;
    gap: 0.6rem;
  }
  h2,
  p {
    margin: 0;
  }
  .pr-body {
    display: flex;
    gap: 1.25rem;
    align-items: flex-start;
  }
  .pr-options {
    display: grid;
    gap: 0.5rem;
    min-width: 15rem;
  }
  .pr-options label {
    display: grid;
    gap: 0.2rem;
  }
  .pr-options label:has(input[type='checkbox']),
  .pr-options label:has(input[type='radio']) {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }
  fieldset {
    margin: 0;
    display: grid;
    gap: 0.4rem;
  }
  .pr-preview {
    max-height: 70vh;
    overflow-y: auto;
    background: var(--fb-surface-alt, #e8e8e8);
    padding: 0.6rem;
    border-radius: 4px;
  }
  .pr-pages {
    display: flex;
    flex-wrap: wrap;
    gap: 0.75rem;
    margin-top: 0.4rem;
  }
  .pr-page {
    position: relative;
    background: #fff;
    box-shadow: 0 1px 4px rgb(0 0 0 / 35%);
    flex: none;
  }
  .pr-cell {
    display: contents;
  }
  .pr-guide,
  .pr-border,
  .pr-clip,
  .pr-text {
    position: absolute;
  }
  .pr-guide {
    outline: 1px dashed rgb(0 0 0 / 15%);
  }
  .pr-clip {
    overflow: hidden;
  }
  .pr-clip img {
    position: absolute;
    max-width: none;
    max-height: none;
  }
  .pr-text {
    overflow: hidden;
    text-align: center;
    white-space: pre;
    line-height: 1.2;
    padding-top: 0.5px;
  }
  .pr-folder {
    display: grid;
    gap: 0.25rem;
  }
  .pr-error {
    color: var(--fb-error, #b00020);
    font-size: 0.85rem;
  }
  .pr-actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }
</style>

<!--
  Info pane (TASK-01-020, PLAN §2D): EXIF (camera, lens, exposure), RGB
  histogram and file properties for the photo the viewer currently shows.
  Data comes from the catalog via the `photo_info` command (properties +
  scan-time EXIF cache only — no image bytes over IPC, PLAN §3 rule 3); the
  histogram is computed in the WebView from the already-displayed preview
  pixels, downscaled into an offscreen canvas before binning.
-->
<script lang="ts">
  import type { GridItem } from "./grid";
  import { t } from "./i18n.svelte.js";
  import AlbumMembership from "./AlbumMembership.svelte";
  import CaptionEditor from "./CaptionEditor.svelte";
  import TagEditor from "./TagEditor.svelte";
  import { previewUri } from "./viewer";
  import {
    formatBytes,
    formatDate,
    formatDimensions,
    formatExposure,
    formatFormat,
    histogram,
    loadPhotoInfo,
    type PhotoInfo,
  } from "./info-pane";

  interface Props {
    /// The photo the viewer currently shows; `null` renders placeholders.
    photo?: GridItem | null;
    /// Fired after the photo's album membership changed (TASK-03-008).
    onalbumschanged?: () => void;
  }

  let { photo = null, onalbumschanged = () => {} }: Props = $props();

  let info = $state<PhotoInfo | null>(null);

  // Reload whenever the viewer navigates (functional spec: the pane updates
  // live as photos change). A monotonically increasing token discards stale
  // responses when navigation outpaces the IPC round-trip.
  let requestSeq = 0;
  $effect(() => {
    const p = photo;
    const seq = ++requestSeq;
    void (async () => {
      const loaded = p ? await loadPhotoInfo(p.id) : null;
      if (seq === requestSeq) info = loaded;
    })();
  });

  type Bins = { r: number[]; g: number[]; b: number[] };
  let bins = $state<Bins | null>(null);
  let chart: HTMLCanvasElement | null = $state(null);

  // Histogram from the displayed image (technical spec: computed in the
  // frontend from the rendered preview). The preview bytes are already in
  // the WebView via preview://; they are downscaled to 256×256 before
  // binning. Environments without canvas (jsdom tests) simply keep the
  // placeholder — the text data never depends on the histogram.
  $effect(() => {
    const uri = previewUri(photo?.hash ?? null);
    let cancelled = false;
    bins = null;
    if (!uri) return;
    const img = new Image();
    img.onload = () => {
      if (cancelled) return;
      try {
        const off = document.createElement("canvas");
        off.width = 256;
        off.height = 256;
        const ctx = off.getContext("2d");
        if (!ctx) return;
        ctx.drawImage(img, 0, 0, 256, 256);
        bins = histogram(ctx.getImageData(0, 0, 256, 256).data, 64);
      } catch {
        bins = null;
      }
    };
    img.src = uri;
    return () => {
      cancelled = true;
    };
  });

  // Paint the bins once they exist. Overlapping translucent channels read
  // like Picasa's RGB overlay; max-normalized so any exposure stays visible.
  $effect(() => {
    if (!chart) return;
    const ctx = chart.getContext("2d");
    if (!ctx) return;
    ctx.clearRect(0, 0, chart.width, chart.height);
    const b = bins;
    if (!b) return;
    const max = Math.max(1, ...b.r, ...b.g, ...b.b);
    const channels: Array<[number[], string]> = [
      [b.r, "rgba(255, 64, 64, 0.55)"],
      [b.g, "rgba(64, 255, 64, 0.55)"],
      [b.b, "rgba(64, 128, 255, 0.55)"],
    ];
    const barW = chart.width / b.r.length;
    for (const [counts, color] of channels) {
      ctx.fillStyle = color;
      counts.forEach((count, i) => {
        const hgt = (count / max) * (chart.height - 2);
        ctx.fillRect(i * barW, chart.height - hgt, Math.max(1, barW - 1), hgt);
      });
    }
  });

  const MISSING = "—";
</script>

<aside class="info-pane" data-testid="info-pane">
  <section>
    <h3>{t("info.exif")}</h3>
    <dl>
      <dt>{t("info.camera")}</dt>
      <dd data-testid="info-camera">{info?.camera ?? MISSING}</dd>
      <dt>{t("info.lens")}</dt>
      <dd data-testid="info-lens">{info?.lens ?? MISSING}</dd>
      <dt>{t("info.exposure")}</dt>
      <dd data-testid="info-exposure">{(info && formatExposure(info)) ?? MISSING}</dd>
    </dl>
  </section>
  <section>
    <h3>{t("info.histogram")}</h3>
    <canvas class="info-histogram" data-testid="info-histogram" bind:this={chart} width="256" height="96"
    ></canvas>
    {#if !bins}
      <p class="info-pending">{t("info.histogramPending")}</p>
    {/if}
  </section>
  <section>
    <h3>{t("info.file")}</h3>
    <dl>
      <dt>{t("info.path")}</dt>
      <dd data-testid="info-path">{info?.path ?? MISSING}</dd>
      <dt>{t("info.size")}</dt>
      <dd data-testid="info-size">{info ? formatBytes(info.size) : MISSING}</dd>
      <dt>{t("info.dimensions")}</dt>
      <dd data-testid="info-dimensions">
        {(info && formatDimensions(info.width, info.height)) ?? MISSING}
      </dd>
      <dt>{t("info.format")}</dt>
      <dd data-testid="info-format">
        {info ? formatFormat(info.filename, info.media_type) : MISSING}
      </dd>
      <dt>{t("info.modified")}</dt>
      <dd data-testid="info-modified">{info ? formatDate(info.mtime) : MISSING}</dd>
    </dl>
  </section>
  <!-- Caption (TASK-03-007, PLAN §2B): written to dc:description through
       the sidecar by the backend. -->
  <section>
    <h3>{t("caption.title")}</h3>
    <CaptionEditor photoId={photo?.id ?? null} />
  </section>
  <!-- Tag editor (TASK-03-006, PLAN §2B): edit the visible photo's tags;
       every change is written through to the sidecar by the backend. -->
  <section>
    <h3>{t("tags.title")}</h3>
    <TagEditor photoId={photo?.id ?? null} />
  </section>
  <!-- Albums (TASK-03-008, PLAN §2 B): tick the albums this photo is in;
       membership is written to the sidecar's fb:Albums bag. -->
  <section>
    <h3>{t("albums.title")}</h3>
    <AlbumMembership photoId={photo?.id ?? null} onchanged={onalbumschanged} />
  </section>
</aside>

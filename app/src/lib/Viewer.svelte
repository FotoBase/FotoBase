<!--
  Single-photo viewer (TASK-01-019, PLAN §2D — P1 scope is view-only).
  Opens over the grid with the current folder/search selection as its
  collection: the main image is a `preview://` URI (full-size bytes fetched
  straight from disk by the WebView, PLAN §3 rule 3 — no image bytes over
  IPC), the filmstrip shows `thumb://` thumbnails for the whole collection.

  Keyboard: ←/→ navigate (and pause a running slideshow), Esc exits,
  F toggles fullscreen (Tauri window API), space toggles the slideshow,
  S toggles the star (TASK-03-005); with ratings enabled 0–5 rate.
-->
<script lang="ts">
  import { thumbUri, type GridItem } from "./grid";
  import { gridState } from "./grid-state.svelte.js";
  import { t } from "./i18n.svelte.js";
  import InfoPane from "./InfoPane.svelte";
  import VideoPlayer from "./VideoPlayer.svelte";
  import { isEditableTarget, nextIndex, preloadUris, previewUri, prevIndex } from "./viewer";
  import { probeAndCache } from "./codec-probe";
  import { ratingForKey, setRating, toggleStar } from "./rating";

  interface Props {
    items?: GridItem[];
    /// Which photo the grid opened (index into `items`).
    startIndex?: number;
    /// Slideshow interval in ms (functional spec: configurable interval).
    slideshowMs?: number;
    /// The context menu's "View slideshow" opens with the slideshow running.
    startSlideshow?: boolean;
    /// Opt-in 1–5 ratings (ADR-0002); off = single star.
    ratingsEnabled?: boolean;
    /// Fired after the backend persisted a rating, so the shell can update
    /// its item list.
    onrate?: (id: number, rating: number) => void;
    onclose?: () => void;
  }

  let {
    items = [],
    startIndex = 0,
    slideshowMs = 3000,
    startSlideshow = false,
    ratingsEnabled = false,
    onrate = () => {},
    onclose = () => {},
  }: Props = $props();

  let index = $state(Math.max(0, Math.min(startIndex, items.length - 1)));
  let slideshowRunning = $state(startSlideshow);
  let fullscreen = $state(false);
  // Info pane (TASK-01-020, PLAN §2 View menu "Show image properties"). The
  // menu bar itself arrives with the file-menu work (01-021); until then the
  // toggle lives in the viewer's status bar and on the `i` key.
  let infoOpen = $state(false);

  const current = $derived(items[index] ?? null);
  const mainUri = $derived(previewUri(current?.hash ?? null));

  // --- Star / rating (TASK-03-005) -------------------------------------------
  // The backend writes the sidecar first; the UI reflects a rating only once
  // `set_rating` resolved. `rated` overlays the items prop until the shell
  // refreshes the list.
  let rated = $state<Record<number, number>>({});
  let ratingError = $state<string | null>(null);
  const currentRating = $derived(
    current === null ? 0 : (rated[current.id] ?? current.rating ?? 0),
  );

  async function rate(value: number): Promise<void> {
    const item = current;
    if (item === null) return;
    ratingError = null;
    try {
      const saved = await setRating(item.id, value);
      rated[item.id] = saved;
      onrate(item.id, saved);
    } catch (e) {
      ratingError = t("viewer.ratingFailed", { error: String(e) });
    }
  }

  // Clicking the current rating again clears it, like the star toggle.
  function rateOrClear(value: number): void {
    void rate(value === currentRating ? 0 : value);
  }

  // --- Video (TASK-02-007, PLAN §2 Video strategy) ---------------------------
  // A video item plays in the player component instead of the `<img>`: the
  // src resolver picks the asset-protocol direct-play URL (codec supported)
  // or `stream://` (02-006). The payload comes from the catalog's ffprobe
  // metadata (02-003) via `video_info`; `null` = image or unknown item.

  type VideoInfoPayload = {
    path: string;
    meta: {
      duration_secs: number;
      codec: string;
      profile: string | null;
      rotation_deg: number;
    } | null;
  } | null;

  let videoInfo = $state<
    | (NonNullable<VideoInfoPayload> & { directSrc: string | null })
    | null
  >(null);
  const isVideo = $derived(
    current !== null && current.media_type === "video" && videoInfo !== null,
  );

  // Fetch the payload for the current item and build the direct-play URL;
  // stale answers are dropped. Browser dev / backend down → treated as an
  // image, never a crash.
  $effect(() => {
    const item = current;
    let stale = false;
    void (async () => {
      try {
        const { invoke, convertFileSrc } = await import("@tauri-apps/api/core");
        const info = await invoke<NonNullable<VideoInfoPayload>>("video_info", {
          fileId: item?.id ?? -1,
        });
        if (!stale) {
          videoInfo =
            info === null
              ? null
              : { ...info, directSrc: convertFileSrc(info.path) };
        }
      } catch {
        if (!stale) videoInfo = null;
      }
    })();
    return () => {
      stale = true;
    };
  });

  const videoSupport = $derived(probeAndCache());

  // Preload the neighbors (functional spec: navigation feels instant). The
  // browser fetches them through the preview:// scheme handler; nothing
  // crosses IPC. Video neighbors are skipped — `preview://` would buffer the
  // whole clip, and videos preload nothing (the player streams on demand).
  $effect(() => {
    void index;
    void items;
    const uris = items.map((it, i) =>
      it.media_type === "video" ? null : previewUri(it.hash),
    );
    for (const uri of preloadUris(uris, index)) {
      const img = new Image();
      img.src = uri;
    }
  });

  // Slideshow timer (technical spec: frontend-only). Re-created whenever the
  // running flag or the interval changes; torn down on pause/exit.
  $effect(() => {
    if (!slideshowRunning) return;
    const timer = setInterval(() => {
      index = nextIndex(index, items.length);
    }, slideshowMs);
    return () => clearInterval(timer);
  });

  async function toggleFullscreen(): Promise<void> {
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      fullscreen = !fullscreen;
      await getCurrentWindow().setFullscreen(fullscreen);
    } catch {
      // Browser dev: no Tauri window API — stay windowed, never crash.
    }
  }

  function onKeydown(e: KeyboardEvent): void {
    if (isEditableTarget(e.target)) return;
    // A video is open: playback keys go to the player (space = play/pause,
    // arrows = seek); navigation keys stay with the viewer.
    const playerKeys = isVideo && [" ", "ArrowLeft", "ArrowRight"].includes(e.key);
    if (playerKeys) return;
    const digit = ratingForKey(e.key, ratingsEnabled);
    if (digit !== null) {
      void rate(digit);
      return;
    }
    switch (e.key) {
      case "s":
      case "S":
        void rate(toggleStar(currentRating));
        break;
      case "ArrowRight":
        slideshowRunning = false; // pauses on interaction
        index = nextIndex(index, items.length);
        break;
      case "ArrowLeft":
        slideshowRunning = false;
        index = prevIndex(index, items.length);
        break;
      case "Escape":
        onclose();
        break;
      case "f":
      case "F":
        void toggleFullscreen();
        break;
      case "i":
      case "I":
        infoOpen = !infoOpen;
        break;
      case " ":
        e.preventDefault();
        slideshowRunning = !slideshowRunning;
        break;
    }
  }

  $effect(() => {
    window.addEventListener("keydown", onKeydown);
    return () => window.removeEventListener("keydown", onKeydown);
  });
</script>

<div class="viewer" data-testid="viewer">
  {#if infoOpen}
    <InfoPane photo={current} />
  {/if}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="viewer-stage" data-testid="viewer-stage">
    {#if isVideo && videoInfo}
      <VideoPlayer
        fileId={current!.id}
        codec={videoInfo.meta?.codec ?? ""}
        rotationDeg={videoInfo.meta?.rotation_deg ?? 0}
        durationSecs={videoInfo.meta?.duration_secs ?? 0}
        directSrc={videoInfo.directSrc}
        support={videoSupport}
      />
    {:else if mainUri}
      <img class="viewer-main" data-testid="viewer-main" src={mainUri} alt={current?.filename ?? ""} draggable="false" />
    {:else}
      <div class="viewer-placeholder" data-testid="viewer-placeholder"></div>
    {/if}
  </div>
  <div class="viewer-status">
    <span data-testid="viewer-count">{index + 1}/{items.length}</span>
    {#if slideshowRunning}
      <span data-testid="slideshow-running">{t("viewer.slideshowRunning")}</span>
    {/if}
    {#if ratingsEnabled}
      <span class="viewer-rating" data-testid="rating-buttons">
        {#each [1, 2, 3, 4, 5] as n (n)}
          <button
            type="button"
            class="viewer-star{n <= currentRating ? ' on' : ''}"
            data-testid={`rating-${n}`}
            aria-pressed={n === currentRating}
            title={t("viewer.rating", { n })}
            onclick={() => rateOrClear(n)}
          >
            {n <= currentRating ? "★" : "☆"}
          </button>
        {/each}
      </span>
    {:else}
      <button
        type="button"
        class="viewer-star{currentRating > 0 ? ' on' : ''}"
        data-testid="star-toggle"
        aria-pressed={currentRating > 0}
        title={t("viewer.star")}
        onclick={() => void rate(toggleStar(currentRating))}
      >
        {currentRating > 0 ? "★" : "☆"}
      </button>
    {/if}
    {#if ratingError}
      <span class="viewer-rating-error" data-testid="rating-error" role="alert">{ratingError}</span>
    {/if}
    <button
      type="button"
      class="viewer-info-toggle"
      data-testid="toggle-info"
      title={t("menu.showImageProperties")}
      onclick={() => (infoOpen = !infoOpen)}
    >
      {t("viewer.info")}
    </button>
  </div>
  <div class="filmstrip" data-testid="filmstrip">
    {#each items as item, i (item.id)}
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <img
        class="filmstrip-thumb{i === index ? ' current' : ''}"
        data-testid="filmstrip-thumb"
        src={thumbUri(item.hash, gridState.thumbVersions[item.hash] ?? 0) ?? ""}
        alt={item.filename}
        draggable="false"
        onclick={() => {
          slideshowRunning = false;
          index = i;
        }}
      />
    {/each}
  </div>
</div>

<!--
  Live/Motion photo press-and-hold playback (TASK-08-017, PLAN §2). Sits over
  the viewer's still: while the badge button is held (pointer or Enter/Space)
  the clip plays muted on top of the image; release pauses and the still
  shows again. Photos without a playable clip, or clips the WebView cannot
  decode, render nothing — the still just stays (unsupported variants
  degrade, never error).
-->
<script lang="ts">
  import { fetchLiveClipSrc, liveBadgeKey, type LiveKind } from "./live";
  import { t } from "./i18n.svelte.js";

  interface Props {
    fileId: number;
    kind: LiveKind;
  }

  let { fileId, kind }: Props = $props();

  let src = $state<string | null>(null);
  let video = $state<HTMLVideoElement | null>(null);
  let holding = $state(false);

  $effect(() => {
    const id = fileId;
    let stale = false;
    src = null;
    holding = false;
    void fetchLiveClipSrc(id).then((found) => {
      if (!stale) src = found;
    });
    return () => {
      stale = true;
    };
  });

  function start(): void {
    if (holding || !video) return;
    holding = true;
    video.currentTime = 0;
    // Autoplay policy / decode failure: stay on the still, never throw.
    void Promise.resolve(video.play()).catch(() => {
      holding = false;
    });
  }

  function stop(): void {
    if (!holding) return;
    holding = false;
    video?.pause();
    if (video) video.currentTime = 0;
  }

  function onKeydown(e: KeyboardEvent): void {
    if (e.key !== "Enter" && e.key !== " ") return;
    // The viewer binds space to the slideshow on the window.
    e.stopPropagation();
    e.preventDefault();
    if (!e.repeat) start();
  }

  function onKeyup(e: KeyboardEvent): void {
    if (e.key === "Enter" || e.key === " ") stop();
  }
</script>

{#if src}
  <video
    bind:this={video}
    class="live-video{holding ? ' active' : ''}"
    data-testid="live-video"
    {src}
    muted
    playsinline
    preload="auto"
    onended={stop}
    onerror={() => (src = null)}
  ></video>
  <button
    type="button"
    class="live-hold"
    data-testid="live-hold"
    title={t(liveBadgeKey(kind))}
    aria-label={t("live.hold")}
    onpointerdown={start}
    onpointerup={stop}
    onpointerleave={stop}
    onpointercancel={stop}
    onkeydown={onKeydown}
    onkeyup={onKeyup}
    onblur={stop}
  >
    ◉ {t(liveBadgeKey(kind))}
  </button>
{/if}

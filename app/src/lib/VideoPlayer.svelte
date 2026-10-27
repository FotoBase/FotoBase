<!--
  Video player (TASK-02-007, PLAN §2 Video strategy). Wraps a `<video>`
  element whose `src` the resolver picks: the asset-protocol URL of the
  original for codecs the WebView decodes (02-005), `stream://` otherwise
  (02-006). Rotation from ffprobe metadata is a CSS transform — no
  re-encode. Controls: play/pause, scrub bar, catalog duration. Keyboard is
  viewer-consistent: space = play/pause, ←/→ = seek ±5 s.

  Cleanup on destroy (functional spec: closing the viewer stops playback and
  releases resources): pause, clear the src, and ask the scheme handler to
  cancel the in-flight transcode (02-006 `stop_stream`). No orphan ffmpeg.
-->
<script lang="ts">
  import { rotationTransform, resolveVideoSrc } from "./video-src";
  import { stopStream } from "./stream";

  interface Props {
    fileId: number;
    /// ffprobe codec name (catalog metadata, 02-003).
    codec: string;
    /// Display rotation in degrees (0/90/180/270).
    rotationDeg?: number;
    /// Catalog duration in seconds; shown before the element reports its own.
    durationSecs?: number;
    /// Asset-protocol URL of the original file; `null` when unavailable.
    directSrc: string | null;
    /// Startup codec-support matrix (02-005); `null` = not probed.
    support: { h264: boolean; hevc: boolean; vp9: boolean; av1: boolean } | null;
  }

  let {
    fileId,
    codec,
    rotationDeg = 0,
    durationSecs = 0,
    directSrc,
    support,
  }: Props = $props();

  const decision = $derived(resolveVideoSrc(fileId, codec, support, directSrc));
  const transform = $derived(rotationTransform(rotationDeg));

  let video = $state<HTMLVideoElement | null>(null);
  let playing = $state(false);
  let position = $state(0);
  let elementDuration = $state(0);

  const total = $derived(
    elementDuration > 0 ? elementDuration : durationSecs,
  );

  function fmt(s: number): string {
    const t = Math.max(0, Math.floor(s));
    const m = Math.floor(t / 60);
    return `${m}:${(t % 60).toString().padStart(2, "0")}`;
  }

  async function togglePlay(): Promise<void> {
    if (!video) return;
    try {
      if (video.paused) await video.play();
      else video.pause();
    } catch {
      // Autoplay policy / backend gone: stay paused, never crash.
    }
  }

  function seekBy(delta: number): void {
    if (!video) return;
    const dur = total > 0 ? total : video.duration || 0;
    video.currentTime = Math.min(Math.max(0, video.currentTime + delta), dur);
  }

  function onScrub(e: Event): void {
    const value = Number((e.target as HTMLInputElement).value);
    if (video) video.currentTime = value;
  }

  // Viewer-consistent keyboard (technical spec). Bound to the window like
  // the viewer's own handler; the Viewer suppresses its space/arrow
  // handling while a video is open so the two never fight.
  function onKeydown(e: KeyboardEvent): void {
    switch (e.key) {
      case " ":
        e.preventDefault();
        void togglePlay();
        break;
      case "ArrowRight":
        e.preventDefault();
        seekBy(5);
        break;
      case "ArrowLeft":
        e.preventDefault();
        seekBy(-5);
        break;
    }
  }

  $effect(() => {
    window.addEventListener("keydown", onKeydown);
    return () => window.removeEventListener("keydown", onKeydown);
  });

  // Teardown (functional spec): stop playback, drop the src so the WebView
  // releases the decoder, and cancel the in-flight transcode. The stop is
  // best-effort — the janitor's idle timeout reaps it anyway.
  $effect(() => {
    const el = video;
    return () => {
      if (el) {
        el.pause();
        el.removeAttribute("src");
        el.load();
      }
      void stopStream(fileId);
    };
  });
</script>

<div class="video-player" data-testid="video-player">
  <!-- svelte-ignore a11y_media_has_caption -->
  <div class="video-stage" data-testid="video-stage" style:transform={transform}>
    <video
      bind:this={video}
      src={decision.src}
      data-testid="video-el"
      onplay={() => (playing = true)}
      onpause={() => (playing = false)}
      ontimeupdate={() => position = video?.currentTime ?? 0}
      onloadedmetadata={() => elementDuration = video?.duration ?? 0}
    ></video>
  </div>
  <div class="video-controls" data-testid="video-controls">
    <button
      type="button"
      data-testid="video-play"
      onclick={togglePlay}
    >{playing ? "⏸" : "▶"}</button>
    <input
      type="range"
      class="video-scrub"
      data-testid="video-scrub"
      min="0"
      max={total > 0 ? total : 0}
      step="0.1"
      value={position}
      oninput={onScrub}
    />
    <span data-testid="video-duration">{fmt(position)} / {fmt(total)}</span>
  </div>
</div>

<script lang="ts">
  import { t } from "./i18n.svelte.js";
  import {
    cancelUpload,
    googlePhotosSignIn,
    googlePhotosSignedIn,
    googlePhotosSignOut,
    listUploadAlbums,
    listUploadTargets,
    onUploadProgress,
    uploadPhotos,
    type UploadAlbum,
    type UploadAlbumSelection,
    type UploadBatchResult,
    type UploadFileResult,
    type UploadProgress,
    type UploadRequest,
    type UploadTargetConfig,
    type UploadTargetInfo,
  } from "./upload-photos";

  let {
    open = $bindable(false),
    ids = $bindable<number[]>([]),
  }: { open?: boolean; ids?: number[] } = $props();

  let targets = $state<UploadTargetInfo[]>([]);
  let targetId = $state("local-folder");
  let folder = $state("");
  let immichUrl = $state("");
  let immichKey = $state("");
  let s3Endpoint = $state("");
  let s3Bucket = $state("");
  let s3Region = $state("us-east-1");
  let s3AccessKey = $state("");
  let s3SecretKey = $state("");
  let s3SessionToken = $state("");
  let s3Prefix = $state("");
  let webdavUrl = $state("");
  let webdavUsername = $state("");
  let webdavPassword = $state("");
  let webdavFolder = $state("");
  let allowInsecureHttp = $state(false);
  let googleClientId = $state("");
  let googleClientSecret = $state("");
  let googleSignedIn = $state(false);
  let sizeMode = $state<"original" | "resized">("original");
  let maxEdge = $state(1600);
  let quality = $state(90);
  let albumMode = $state<"none" | "existing" | "create">("none");
  let albums = $state<UploadAlbum[]>([]);
  let selectedAlbumId = $state("");
  let newAlbumName = $state("");
  let albumBusy = $state(false);
  let busy = $state(false);
  let targetsBusy = $state(false);
  let signingIn = $state(false);
  let jobId = $state<string | null>(null);
  let progress = $state<UploadProgress | null>(null);
  let fileResults = $state<Record<number, UploadFileResult>>({});
  let error = $state<string | null>(null);
  let albumError = $state<string | null>(null);
  let wasOpen = false;

  let selectedTarget = $derived(targets.find((target) => target.id === targetId));
  let configReady = $derived(isConfigReady());
  let canRun = $derived(
    !busy &&
      !targetsBusy &&
      !signingIn &&
      ids.length > 0 &&
      Object.keys(fileResults).length === 0 &&
      selectedTarget !== undefined &&
      configReady &&
      (selectedTarget.capabilities.albums === false || albumIsReady()) &&
      (targetId !== "google-photos" || googleSignedIn),
  );
  let currentProgress = $derived(
    progress ? t("upload.progress", { done: progress.index + 1, total: progress.total }) : null,
  );

  $effect(() => {
    if (open && !wasOpen) {
      fileResults = {};
      error = null;
      albumError = null;
      progress = null;
      jobId = null;
      if (targets.length === 0) void loadTargets();
    } else if (!open && wasOpen) {
      clearSecrets();
    }
    wasOpen = open;
  });

  function makeConfig(): UploadTargetConfig {
    switch (targetId) {
      case "immich":
        return { plugin: "immich", server_url: immichUrl.trim(), api_key: immichKey };
      case "s3":
        return {
          plugin: "s3",
          endpoint: s3Endpoint.trim(),
          bucket: s3Bucket.trim(),
          region: s3Region.trim(),
          access_key: s3AccessKey.trim(),
          secret_key: s3SecretKey,
          session_token: s3SessionToken.trim() || null,
          prefix: s3Prefix.trim(),
        };
      case "webdav":
        return {
          plugin: "webdav",
          url: webdavUrl.trim(),
          username: webdavUsername.trim(),
          password: webdavPassword,
          folder: webdavFolder.trim(),
          allow_insecure_http: allowInsecureHttp,
        };
      case "google-photos":
        return {
          plugin: "google-photos",
          client_id: googleClientId.trim(),
          client_secret: googleClientSecret.trim() || null,
        };
      default:
        return { plugin: "local-folder", root: folder.trim() };
    }
  }

  function albumIsReady(): boolean {
    if (albumMode === "none") return true;
    if (albumMode === "create") return newAlbumName.trim() !== "";
    return selectedAlbumId !== "" && albums.some((album) => album.id === selectedAlbumId);
  }

  function isConfigReady(): boolean {
    switch (targetId) {
      case "immich":
        return immichUrl.trim() !== "" && immichKey.trim() !== "";
      case "s3":
        return (
          s3Endpoint.trim() !== "" &&
          s3Bucket.trim() !== "" &&
          s3Region.trim() !== "" &&
          s3AccessKey.trim() !== "" &&
          s3SecretKey.trim() !== ""
        );
      case "webdav":
        return webdavUrl.trim() !== "" && webdavUsername.trim() !== "";
      case "google-photos":
        return googleClientId.trim() !== "";
      default:
        return folder.trim() !== "";
    }
  }

  function makeAlbum(): UploadAlbumSelection | null {
    if (albumMode === "create") {
      const name = newAlbumName.trim();
      return name === "" ? null : { mode: "create", name };
    }
    if (albumMode === "existing") {
      const album = albums.find((item) => item.id === selectedAlbumId);
      return album ? { mode: "existing", id: album.id, name: album.name } : null;
    }
    return null;
  }

  function makeRequest(job: string | null): UploadRequest {
    return {
      config: makeConfig(),
      size: sizeMode === "original"
        ? { mode: "original" }
        : { mode: "resized", max_edge: Number(maxEdge) },
      quality: Number(quality),
      album: makeAlbum(),
      jobId: job,
    };
  }

  async function loadTargets(): Promise<void> {
    targetsBusy = true;
    error = null;
    try {
      targets = await listUploadTargets();
      if (!targets.some((target) => target.id === targetId)) {
        targetId = targets[0]?.id ?? "";
      }
    } catch (e) {
      error = t("upload.error", { error: String(e) });
    } finally {
      targetsBusy = false;
    }
  }

  function changeTarget(event: Event): void {
    targetId = (event.currentTarget as HTMLSelectElement).value;
    albums = [];
    albumMode = "none";
    selectedAlbumId = "";
    albumError = null;
    googleSignedIn = false;
    if (targetId === "google-photos") void checkGoogleAccount();
  }

  async function chooseFolder(): Promise<void> {
    try {
      const { open: pick } = await import("@tauri-apps/plugin-dialog");
      const picked = await pick({ directory: true, multiple: false });
      if (typeof picked === "string") folder = picked;
    } catch (e) {
      error = t("upload.error", { error: String(e) });
    }
  }

  async function loadAlbums(): Promise<void> {
    if (!configReady || !selectedTarget?.capabilities.albums) return;
    albumBusy = true;
    albumError = null;
    try {
      albums = await listUploadAlbums(makeConfig());
      selectedAlbumId = albums[0]?.id ?? "";
      if (albums.length > 0) albumMode = "existing";
    } catch (e) {
      albumError = t("upload.error", { error: String(e) });
    } finally {
      albumBusy = false;
    }
  }

  async function checkGoogleAccount(): Promise<void> {
    const clientId = googleClientId.trim();
    googleSignedIn = false;
    if (clientId === "") return;
    try {
      googleSignedIn = await googlePhotosSignedIn(clientId);
    } catch (e) {
      error = t("upload.error", { error: String(e) });
    }
  }

  async function signInGoogle(): Promise<void> {
    if (googleClientId.trim() === "") return;
    signingIn = true;
    error = null;
    try {
      await googlePhotosSignIn(googleClientId.trim(), googleClientSecret);
      googleSignedIn = true;
      googleClientSecret = "";
    } catch (e) {
      error = t("upload.error", { error: String(e) });
    } finally {
      signingIn = false;
    }
  }

  async function signOutGoogle(): Promise<void> {
    try {
      await googlePhotosSignOut(googleClientId.trim());
      googleSignedIn = false;
    } catch (e) {
      error = t("upload.error", { error: String(e) });
    }
  }

  async function run(idsToUpload: number[], resumeJobId: string | null = null): Promise<void> {
    if (busy || idsToUpload.length === 0) return;
    busy = true;
    error = null;
    progress = null;
    jobId = resumeJobId ?? crypto.randomUUID();
    const activeJobId = jobId;
    let unlisten: (() => void) | null = null;
    try {
      unlisten = await onUploadProgress((event) => {
        if (jobId === null || event.jobId === jobId) {
          jobId = event.jobId;
          progress = event;
        }
      });
      const result: UploadBatchResult = await uploadPhotos(
        idsToUpload,
        makeRequest(activeJobId),
      );
      jobId = result.jobId;
      for (const file of result.files) fileResults[file.fileId] = file;
      if (result.paused) error = t("upload.paused");
      else if (result.cancelled) error = t("upload.cancelled");
    } catch (e) {
      error = t("upload.error", { error: String(e) });
    } finally {
      unlisten?.();
      busy = false;
    }
  }

  async function cancel(): Promise<void> {
    if (!jobId) return;
    try {
      await cancelUpload(jobId);
    } catch (e) {
      error = t("upload.error", { error: String(e) });
    }
  }

  function retry(fileId: number): void {
    void run([fileId], jobId);
  }

  function retryRemaining(): void {
    const retryIds = ids.filter((id) => {
      const state = fileResults[id]?.state;
      return state === "failed" || state === "pending";
    });
    void run(retryIds, jobId);
  }

  function clearSecrets(): void {
    immichKey = "";
    s3SecretKey = "";
    s3SessionToken = "";
    webdavPassword = "";
    googleClientSecret = "";
  }

  function close(): void {
    if (!busy) open = false;
  }
</script>

{#if open}
  <div class="up-backdrop" data-testid="upload-backdrop">
    <div class="up-dialog" role="dialog" aria-label={t("upload.title")}>
      <h2>{t("upload.title")}</h2>
      <p>{t("upload.count", { n: ids.length })}</p>

      <label>
        {t("upload.target")}
        <select
          data-testid="upload-target"
          value={targetId}
          onchange={changeTarget}
          disabled={busy || targetsBusy}
        >
          {#each targets as target (target.id)}
            <option value={target.id}>{target.name}</option>
          {/each}
        </select>
      </label>

      {#if targetId === "local-folder"}
        <label>
          {t("upload.folder")}
          <span class="up-row">
            <input data-testid="upload-local-folder" bind:value={folder} disabled={busy} />
            <button type="button" data-testid="upload-choose-folder" onclick={chooseFolder} disabled={busy}>
              {t("upload.choose")}
            </button>
          </span>
        </label>
      {:else if targetId === "immich"}
        <label>{t("upload.server")}<input bind:value={immichUrl} disabled={busy} /></label>
        <label>{t("upload.apiKey")}<input type="password" bind:value={immichKey} disabled={busy} /></label>
      {:else if targetId === "s3"}
        <label>{t("upload.endpoint")}<input bind:value={s3Endpoint} disabled={busy} /></label>
        <label>{t("upload.bucket")}<input bind:value={s3Bucket} disabled={busy} /></label>
        <label>{t("upload.region")}<input bind:value={s3Region} disabled={busy} /></label>
        <label>{t("upload.prefix")}<input bind:value={s3Prefix} disabled={busy} /></label>
        <label>{t("upload.accessKey")}<input bind:value={s3AccessKey} disabled={busy} /></label>
        <label>{t("upload.secretKey")}<input type="password" bind:value={s3SecretKey} disabled={busy} /></label>
        <label>{t("upload.sessionToken")}<input type="password" bind:value={s3SessionToken} disabled={busy} /></label>
      {:else if targetId === "webdav"}
        <label>{t("upload.webdavUrl")}<input bind:value={webdavUrl} disabled={busy} /></label>
        <label>{t("upload.username")}<input bind:value={webdavUsername} disabled={busy} /></label>
        <label>{t("upload.password")}<input type="password" bind:value={webdavPassword} disabled={busy} /></label>
        <label>{t("upload.folder")}<input bind:value={webdavFolder} disabled={busy} /></label>
        <label class="up-check">
          <input type="checkbox" bind:checked={allowInsecureHttp} disabled={busy} />
          {t("upload.allowHttp")}
        </label>
        {#if allowInsecureHttp}
          <p class="up-warning">{t("upload.httpWarning")}</p>
        {/if}
      {:else if targetId === "google-photos"}
        <p>{t("upload.googleHint")}</p>
        <label>{t("upload.googleClientId")}<input bind:value={googleClientId} oninput={() => (googleSignedIn = false)} onchange={() => void checkGoogleAccount()} disabled={busy} /></label>
        <label>{t("upload.googleClientSecret")}<input type="password" bind:value={googleClientSecret} disabled={busy} /></label>
        {#if googleSignedIn}
          <p>{t("upload.googleConnected")}</p>
          <button type="button" data-testid="upload-google-sign-out" onclick={signOutGoogle} disabled={busy}>
            {t("upload.signOut")}
          </button>
        {:else}
          <button
            type="button"
            data-testid="upload-google-sign-in"
            onclick={signInGoogle}
            disabled={signingIn || googleClientId.trim() === ""}
          >
            {signingIn ? t("upload.signingIn") : t("upload.signIn")}
          </button>
        {/if}
      {/if}

      {#if selectedTarget?.capabilities.albums}
        <fieldset>
          <legend>{t("upload.album")}</legend>
          <label>
            <input type="radio" name="upload-album" value="none" bind:group={albumMode} disabled={busy} />
            {t("upload.album.none")}
          </label>
          <label>
            <input type="radio" name="upload-album" value="existing" bind:group={albumMode} disabled={busy} />
            {t("upload.album.existing")}
          </label>
          <label>
            <input type="radio" name="upload-album" value="create" bind:group={albumMode} disabled={busy} />
            {t("upload.album.create")}
          </label>
          <button
            type="button"
            data-testid="upload-load-albums"
            onclick={loadAlbums}
            disabled={busy || albumBusy || !configReady}
          >
            {albumBusy ? t("upload.loadingAlbums") : t("upload.loadAlbums")}
          </button>
          {#if albumMode === "existing"}
            <select data-testid="upload-existing-album" bind:value={selectedAlbumId} disabled={busy}>
              {#each albums as album (album.id)}
                <option value={album.id}>{album.name}</option>
              {/each}
            </select>
          {:else if albumMode === "create"}
            <input data-testid="upload-new-album" bind:value={newAlbumName} disabled={busy} />
          {/if}
          {#if albumError}<p class="up-error" role="alert">{albumError}</p>{/if}
        </fieldset>
      {/if}

      <fieldset>
        <legend>{t("upload.size")}</legend>
        <label>
          <input type="radio" name="upload-size" value="original" data-testid="upload-size-original" bind:group={sizeMode} disabled={busy} />
          {t("upload.size.original")}
        </label>
        <label>
          <input
            type="radio"
            name="upload-size"
            value="resized"
            bind:group={sizeMode}
            data-testid="upload-size"
            disabled={busy}
          />
          {t("upload.size.resized")}
        </label>
        {#if sizeMode === "resized"}
          <label>
            {t("upload.maxEdge")}
            <select data-testid="upload-max-edge" bind:value={maxEdge} disabled={busy}>
              {#each [1200, 1600, 2048, 2560, 4096] as edge (edge)}
                <option value={edge}>{edge} px</option>
              {/each}
            </select>
          </label>
          <label>
            {t("upload.quality", { n: quality })}
            <input type="range" min="50" max="100" step="5" bind:value={quality} disabled={busy} />
          </label>
        {/if}
      </fieldset>

      {#if busy && currentProgress}
        <p data-testid="upload-progress">{currentProgress}: {progress?.name} ({progress?.state})</p>
      {/if}

      {#if Object.keys(fileResults).length > 0}
        <ul class="up-results" data-testid="upload-results">
          {#each Object.values(fileResults) as file (file.fileId)}
            <li data-testid={`upload-file-${file.fileId}`}>
              <span>{file.name} — {t(`upload.state.${file.state}`)}</span>
              {#if file.error}<span class="up-error">{file.error}</span>{/if}
              {#if file.state === "failed" || file.state === "pending"}
                <button
                  type="button"
                  data-testid={`upload-retry-${file.fileId}`}
                  onclick={() => retry(file.fileId)}
                  disabled={busy || jobId === null}
                >
                  {file.state === "pending" ? t("upload.resumeFile") : t("upload.retryFile")}
                </button>
              {/if}
            </li>
          {/each}
        </ul>
        {#if Object.values(fileResults).some((file) => file.state === "failed" || file.state === "pending")}
          <button type="button" data-testid="upload-retry-remaining" onclick={retryRemaining} disabled={busy || jobId === null}>
            {t("upload.retryRemaining")}
          </button>
        {/if}
      {/if}

      {#if error}<p class="up-error" role="alert" data-testid="upload-error">{error}</p>{/if}

      <p class="up-muted">{t("upload.credentialsHint")}</p>
      <div class="up-actions">
        {#if busy}
          <button type="button" data-testid="upload-cancel" onclick={cancel} disabled={jobId === null}>
            {t("upload.cancel")}
          </button>
        {:else}
          <button type="button" data-testid="upload-run" onclick={() => void run(ids)} disabled={!canRun}>
            {t("upload.run")}
          </button>
        {/if}
        <button type="button" data-testid="upload-close" onclick={close} disabled={busy}>
          {t("upload.close")}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .up-backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
    background: var(--fb-backdrop, rgb(0 0 0 / 40%));
    display: grid;
    place-items: center;
  }
  .up-dialog {
    background: var(--fb-surface, #fff);
    color: var(--fb-text, #111);
    border-radius: 8px;
    width: min(42rem, 92vw);
    max-height: 90vh;
    overflow-y: auto;
    padding: 1rem 1.25rem;
    box-shadow: 0 8px 32px var(--fb-shadow, rgb(0 0 0 / 30%));
    text-align: left;
    display: grid;
    gap: 0.55rem;
  }
  h2,
  p {
    margin: 0;
  }
  label {
    display: grid;
    gap: 0.25rem;
  }
  fieldset {
    margin: 0;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem 1rem;
  }
  .up-row {
    display: flex;
    gap: 0.4rem;
  }
  .up-row input {
    flex: 1;
    min-width: 0;
  }
  .up-check {
    display: flex;
    align-items: center;
  }
  .up-warning,
  .up-error {
    color: var(--fb-error, #b00020);
    font-size: 0.85rem;
  }
  .up-muted {
    color: var(--fb-muted, #666);
    font-size: 0.85rem;
  }
  .up-results {
    margin: 0;
    padding-left: 1.25rem;
    display: grid;
    gap: 0.35rem;
  }
  .up-results li {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 0.25rem 0.75rem;
  }
  .up-results .up-error {
    grid-column: 1 / -1;
  }
  .up-actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 0.3rem;
  }
</style>

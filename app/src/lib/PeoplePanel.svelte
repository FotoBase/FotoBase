<!--
  Unnamed people queue (TASK-05-005, PLAN §2G): review unnamed clusters and
  act on them in bulk. Confirm names the cluster — every face of it is
  stamped and the name is written into each photo's sidecar as a named MWG
  region; reject suppresses the suggestion per face (the faces stay
  detected). Representative thumbnails come from the `thumb://` scheme like
  the grid (no image bytes over IPC); the face box is drawn over the photo.
-->
<script lang="ts">
  import { t } from "./i18n.svelte.js";
  import {
    canConfirm,
    confirmCluster,
    faceBox,
    faceThumbUri,
    peopleQueue,
    rejectCluster,
    type QueueCluster,
  } from "./people";

  let {
    open = $bindable(false),
    onchanged,
  }: {
    open?: boolean;
    /// A bulk action landed: the shell refreshes (names may now filter).
    onchanged: () => void;
  } = $props();

  let clusters = $state<QueueCluster[]>([]);
  let loaded = $state(false);
  let error = $state<string | null>(null);
  let status = $state<string | null>(null);
  /// The name typed for each cluster, keyed by cluster id.
  let names = $state<Map<number, string>>(new Map());
  /// Cluster id whose reject awaits confirmation.
  let confirming = $state<number | null>(null);

  $effect(() => {
    if (!open) return;
    loaded = false;
    error = null;
    status = null;
    confirming = null;
    names = new Map();
    clusters = [];
    void (async () => {
      try {
        clusters = await peopleQueue();
      } catch (e) {
        error = t("people.error", { error: String(e) });
      } finally {
        loaded = true;
      }
    })();
  });

  function nameOf(id: number): string {
    return names.get(id) ?? "";
  }

  function setName(id: number, name: string): void {
    names = new Map(names).set(id, name);
  }

  async function confirm(cluster: QueueCluster): Promise<void> {
    const name = nameOf(cluster.cluster_id);
    if (!canConfirm(name)) return;
    error = null;
    try {
      const n = await confirmCluster(cluster.cluster_id, name);
      dropCluster(cluster.cluster_id);
      status = t("people.confirmed", { name, n });
      onchanged();
    } catch (e) {
      error = t("people.error", { error: String(e) });
    }
  }

  async function reject(cluster: QueueCluster): Promise<void> {
    if (confirming !== cluster.cluster_id) return;
    error = null;
    try {
      const n = await rejectCluster(cluster.cluster_id);
      dropCluster(cluster.cluster_id);
      status = t("people.rejected", { n });
      onchanged();
    } catch (e) {
      error = t("people.error", { error: String(e) });
    } finally {
      confirming = null;
    }
  }

  function dropCluster(id: number): void {
    clusters = clusters.filter((c) => c.cluster_id !== id);
    const next = new Map(names);
    next.delete(id);
    names = next;
  }
</script>

{#if open}
  <div class="pp-backdrop" data-testid="people-backdrop">
    <div class="pp-dialog" role="dialog" aria-label={t("people.title")}>
      <h2>{t("people.title")}</h2>
      {#if status}
        <p data-testid="people-status">{status}</p>
      {/if}
      {#if loaded && clusters.length === 0 && !error}
        <p data-testid="people-empty">{t("people.empty")}</p>
      {/if}
      <ul>
        {#each clusters as cluster (cluster.cluster_id)}
          <li data-testid="people-cluster" data-cluster-id={cluster.cluster_id}>
            <div class="pp-thumb">
              {#if cluster.hash}
                <img src={faceThumbUri(cluster)} alt={t("people.faceAlt", { n: cluster.face_count })} />
                <span class="pp-box" style={Object.entries(faceBox(cluster))
                  .map(([k, v]) => `${k}:${v}`)
                  .join(";")}></span>
              {/if}
            </div>
            <div class="pp-body">
              <p class="pp-count">{t("people.faces", { n: cluster.face_count })}</p>
              <input
                type="text"
                data-testid="people-name"
                placeholder={t("people.namePlaceholder")}
                value={nameOf(cluster.cluster_id)}
                oninput={(e) => setName(cluster.cluster_id, e.currentTarget.value)}
              />
            </div>
            <div class="pp-actions">
              <button
                type="button"
                data-testid="people-confirm"
                disabled={!canConfirm(nameOf(cluster.cluster_id))}
                onclick={() => void confirm(cluster)}
              >
                {t("people.confirm")}
              </button>
              {#if confirming === cluster.cluster_id}
                <span class="pp-confirm-reject">
                  <button
                    type="button"
                    data-testid="people-reject-yes"
                    onclick={() => void reject(cluster)}
                  >
                    {t("people.rejectYes")}
                  </button>
                  <button
                    type="button"
                    data-testid="people-reject-no"
                    onclick={() => (confirming = null)}
                  >
                    {t("people.rejectNo")}
                  </button>
                </span>
              {:else}
                <button
                  type="button"
                  data-testid="people-reject"
                  onclick={() => (confirming = cluster.cluster_id)}
                >
                  {t("people.reject")}
                </button>
              {/if}
            </div>
          </li>
        {/each}
      </ul>
      {#if error}
        <p class="pp-error" role="alert">{error}</p>
      {/if}
      <div class="pp-footer">
        <button type="button" data-testid="people-close" onclick={() => (open = false)}>
          {t("people.close")}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .pp-backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    background: var(--fb-backdrop, rgb(0 0 0 / 40%));
    display: grid;
    place-items: center;
  }
  .pp-dialog {
    background: var(--fb-surface, #fff);
    color: var(--fb-text, #111);
    border-radius: 8px;
    min-width: 32rem;
    max-width: 90vw;
    max-height: 90vh;
    overflow-y: auto;
    padding: 1rem 1.25rem;
    box-shadow: 0 8px 32px var(--fb-shadow, rgb(0 0 0 / 30%));
    text-align: left;
  }
  h2,
  p {
    margin: 0 0 0.6rem;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 0.8rem;
  }
  li {
    display: flex;
    gap: 0.8rem;
    align-items: center;
    border: 1px solid var(--fb-border, #ccc);
    border-radius: 6px;
    padding: 0.5rem;
  }
  .pp-thumb {
    position: relative;
    width: 96px;
    height: 96px;
    flex: none;
  }
  .pp-thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 4px;
  }
  .pp-box {
    position: absolute;
    border: 2px solid var(--fb-accent, #2b6cb0);
    border-radius: 2px;
    pointer-events: none;
  }
  .pp-body {
    flex: 1;
  }
  .pp-count {
    font-size: 0.8rem;
    color: var(--fb-muted, #6b6b6b);
  }
  input {
    width: 100%;
    padding: 0.3rem 0.5rem;
    border: 1px solid var(--fb-border, #ccc);
    border-radius: 4px;
    background: var(--fb-input-bg, #fff);
    color: inherit;
  }
  .pp-actions {
    display: flex;
    gap: 0.5rem;
    align-items: center;
    flex: none;
  }
  .pp-confirm-reject {
    display: flex;
    gap: 0.25rem;
  }
  button {
    padding: 0.3rem 0.7rem;
    border: 1px solid var(--fb-border, #ccc);
    border-radius: 4px;
    background: none;
    color: inherit;
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .pp-error {
    color: var(--fb-error, #b00020);
    font-size: 0.85rem;
  }
  .pp-footer {
    display: flex;
    justify-content: flex-end;
    margin-top: 0.8rem;
  }
</style>

<!--
  People Manager (TASK-05-006, PLAN §2 Tools menu, §6 P3 People): every
  named person and every unnamed cluster, with representative face
  thumbnails (the `thumb://` scheme — no image bytes over IPC, §3 rule 3)
  and face counts. Renaming a person updates all their faces — every
  confirmed face's sidecar MWG region is rewritten sidecar-first; unnamed
  clusters point at the unnamed queue.
-->
<script lang="ts">
  import { t } from "./i18n.svelte.js";
  import {
    canLink,
    canRename,
    contactLabel,
    entryThumbUri,
    faceBox,
    linkContact,
    peopleEntries,
    renamePerson,
    searchContacts,
    unlinkContact,
    type ContactRef,
    type PeopleEntry,
  } from "./people";

  let {
    open = $bindable(false),
    onchanged,
    onOpenQueue,
  }: {
    open?: boolean;
    /// A rename landed: the shell refreshes (names may now filter).
    onchanged: () => void;
    /// The unnamed queue's entry point for unnamed clusters.
    onOpenQueue: () => void;
  } = $props();

  let entries = $state<PeopleEntry[]>([]);
  let loaded = $state(false);
  let error = $state<string | null>(null);
  let status = $state<string | null>(null);
  /// Entry being renamed, keyed by person id.
  let renaming = $state<number | null>(null);
  /// The name typed for the entry being renamed.
  let renameName = $state("");
  /// Entry being contact-linked, keyed by person id (TASK-05-011).
  let linking = $state<number | null>(null);
  /// The query typed into the local contacts search.
  let contactQuery = $state("");
  /// The local contacts store's hits for that query.
  let contactHits = $state<ContactRef[]>([]);

  $effect(() => {
    if (!open) return;
    loaded = false;
    error = null;
    status = null;
    renaming = null;
    linking = null;
    entries = [];
    void (async () => {
      try {
        entries = await peopleEntries();
      } catch (e) {
        error = t("people.managerError", { error: String(e) });
      } finally {
        loaded = true;
      }
    })();
  });

  function beginRename(entry: PeopleEntry): void {
    if (entry.person_id === null) return;
    renaming = entry.person_id;
    renameName = entry.name ?? "";
  }

  function endRename(): void {
    renaming = null;
    renameName = "";
  }

  function beginLink(entry: PeopleEntry): void {
    if (entry.person_id === null) return;
    linking = entry.person_id;
    contactQuery = "";
    contactHits = [];
  }

  function endLink(): void {
    linking = null;
    contactQuery = "";
    contactHits = [];
  }

  async function runContactSearch(): Promise<void> {
    const q = contactQuery.trim();
    if (!q) {
      contactHits = [];
      return;
    }
    try {
      contactHits = await searchContacts(q);
    } catch (e) {
      error = t("people.managerError", { error: String(e) });
    }
  }

  async function saveLink(entry: PeopleEntry, contactId: string): Promise<void> {
    if (!canLink(contactId)) return;
    error = null;
    try {
      const { contact } = await linkContact(entry.person_id ?? 0, contactId);
      entries = entries.map((e) =>
        e.person_id === entry.person_id
          ? { ...e, contact_id: contact ? contactId : null, contact }
          : e,
      );
      status = contact
        ? t("people.linked", { name: contact.name })
        : t("people.linkedNoContact");
      onchanged();
    } catch (e) {
      error = t("people.managerError", { error: String(e) });
    } finally {
      endLink();
    }
  }

  async function removeLink(entry: PeopleEntry): Promise<void> {
    error = null;
    try {
      await unlinkContact(entry.person_id ?? 0);
      entries = entries.map((e) =>
        e.person_id === entry.person_id ? { ...e, contact_id: null, contact: null } : e,
      );
      status = t("people.unlinked");
      onchanged();
    } catch (e) {
      error = t("people.managerError", { error: String(e) });
    }
  }

  async function saveRename(entry: PeopleEntry): Promise<void> {
    const name = renameName.trim();
    if (!canRename(entry.name, name)) return;
    error = null;
    try {
      const renamed = await renamePerson(entry.person_id ?? 0, name);
      entries = entries.map((e) =>
        e.person_id === entry.person_id ? { ...e, name: renamed.name } : e,
      );
      status = t("people.renamed", { name: renamed.name });
      onchanged();
    } catch (e) {
      error = t("people.managerError", { error: String(e) });
    } finally {
      endRename();
    }
  }

  const named = $derived(entries.filter((e) => e.person_id !== null));
  const unnamed = $derived(entries.filter((e) => e.person_id === null));
</script>

{#if open}
  <div class="pm-backdrop" data-testid="pm-backdrop">
    <div class="pm-dialog" role="dialog" aria-label={t("people.managerTitle")}>
      <h2>{t("people.managerTitle")}</h2>
      {#if status}
        <p data-testid="pm-status">{status}</p>
      {/if}
      {#if loaded && entries.length === 0 && !error}
        <p data-testid="pm-empty">{t("people.noPeople")}</p>
      {/if}
      {#if named.length > 0}
        <h3>{t("people.named")}</h3>
        <ul>
          {#each named as entry (entry.person_id)}
            <li data-testid="pm-entry" data-person-id={entry.person_id ?? ""}>
              <div class="pm-thumb">
                {#if entry.representative?.hash}
                  <img
                    src={entryThumbUri(entry)}
                    alt={t("people.faceAlt", { n: entry.face_count })}
                  />
                  <span class="pm-box" style={Object.entries(faceBox(entry.representative))
                    .map(([k, v]) => `${k}:${v}`)
                    .join(";")}></span>
                {/if}
              </div>
              <div class="pm-body">
                {#if renaming === entry.person_id}
                  <input
                    type="text"
                    data-testid="pm-rename-input"
                    value={renameName}
                    oninput={(e) => (renameName = e.currentTarget.value)}
                    onkeydown={(e) => {
                      if (e.key === "Enter") void saveRename(entry);
                      if (e.key === "Escape") endRename();
                    }}
                  />
                {:else}
                  <p class="pm-name">{entry.name}</p>
                {/if}
                <p class="pm-count">{t("people.faces", { n: entry.face_count })}</p>
                {#if entry.contact}
                  <p class="pm-contact" data-testid="pm-contact">
                    {contactLabel(entry.contact)}
                  </p>
                {/if}
                {#if linking === entry.person_id}
                  <div class="pm-linking">
                    <input
                      type="text"
                      data-testid="pm-contact-query"
                      placeholder={t("people.contactSearchPlaceholder")}
                      value={contactQuery}
                      oninput={(e) => (contactQuery = e.currentTarget.value)}
                      onkeydown={(e) => {
                        if (e.key === "Enter") void runContactSearch();
                        if (e.key === "Escape") endLink();
                      }}
                    />
                    <button
                      type="button"
                      data-testid="pm-contact-search"
                      onclick={() => void runContactSearch()}
                    >
                      {t("people.contactSearch")}
                    </button>
                    {#if contactHits.length > 0}
                      <ul class="pm-hits">
                        {#each contactHits as hit (hit.id)}
                          <li>
                            <button
                              type="button"
                              data-testid="pm-contact-hit"
                              onclick={() => void saveLink(entry, hit.id)}
                            >
                              {contactLabel({ name: hit.name, email: hit.email })}
                            </button>
                          </li>
                        {/each}
                      </ul>
                    {:else}
                      <p class="pm-empty-hits" data-testid="pm-contact-empty">
                        {t("people.noContacts")}
                      </p>
                    {/if}
                  </div>
                {/if}
              </div>
              <div class="pm-actions">
                {#if renaming === entry.person_id}
                  <button
                    type="button"
                    data-testid="pm-rename-save"
                    disabled={!canRename(entry.name, renameName)}
                    onclick={() => void saveRename(entry)}
                  >
                    {t("people.renameSave")}
                  </button>
                  <button type="button" data-testid="pm-rename-cancel" onclick={endRename}>
                    {t("people.renameCancel")}
                  </button>
                {:else}
                  <button
                    type="button"
                    data-testid="pm-rename"
                    onclick={() => beginRename(entry)}
                  >
                    {t("people.rename")}
                  </button>
                  {#if entry.contact}
                    <button
                      type="button"
                      data-testid="pm-unlink"
                      onclick={() => void removeLink(entry)}
                    >
                      {t("people.unlink")}
                    </button>
                  {:else}
                    <button
                      type="button"
                      data-testid="pm-link"
                      onclick={() => beginLink(entry)}
                    >
                      {t("people.linkContact")}
                    </button>
                  {/if}
                {/if}
              </div>
            </li>
          {/each}
        </ul>
      {/if}
      {#if unnamed.length > 0}
        <h3>{t("people.unnamed")}</h3>
        <ul>
          {#each unnamed as entry (entry.cluster_id)}
            <li data-testid="pm-entry" data-cluster-id={entry.cluster_id ?? ""}>
              <div class="pm-thumb">
                {#if entry.representative?.hash}
                  <img
                    src={entryThumbUri(entry)}
                    alt={t("people.faceAlt", { n: entry.face_count })}
                  />
                  <span class="pm-box" style={Object.entries(faceBox(entry.representative))
                    .map(([k, v]) => `${k}:${v}`)
                    .join(";")}></span>
                {/if}
              </div>
              <div class="pm-body">
                <p class="pm-count">{t("people.faces", { n: entry.face_count })}</p>
              </div>
              <div class="pm-actions">
                <button
                  type="button"
                  data-testid="pm-review-queue"
                  onclick={onOpenQueue}
                >
                  {t("people.reviewQueue")}
                </button>
              </div>
            </li>
          {/each}
        </ul>
      {/if}
      {#if error}
        <p class="pm-error" role="alert">{error}</p>
      {/if}
      <div class="pm-footer">
        <button type="button" data-testid="pm-close" onclick={() => (open = false)}>
          {t("people.close")}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .pm-backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    background: var(--fb-backdrop, rgb(0 0 0 / 40%));
    display: grid;
    place-items: center;
  }
  .pm-dialog {
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
  h3,
  p {
    margin: 0 0 0.6rem;
  }
  h3 {
    font-size: 0.9rem;
    color: var(--fb-muted, #6b6b6b);
  }
  ul {
    list-style: none;
    margin: 0 0 0.8rem;
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
  .pm-thumb {
    position: relative;
    width: 96px;
    height: 96px;
    flex: none;
  }
  .pm-thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 4px;
  }
  .pm-box {
    position: absolute;
    border: 2px solid var(--fb-accent, #2b6cb0);
    border-radius: 2px;
    pointer-events: none;
  }
  .pm-body {
    flex: 1;
  }
  .pm-name {
    font-weight: 600;
  }
  .pm-count {
    font-size: 0.8rem;
    color: var(--fb-muted, #6b6b6b);
  }
  .pm-contact {
    font-size: 0.8rem;
    color: var(--fb-accent, #2b6cb0);
  }
  .pm-linking {
    display: grid;
    gap: 0.4rem;
    margin-top: 0.4rem;
  }
  .pm-hits {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 0.2rem;
  }
  .pm-empty-hits {
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
  .pm-actions {
    display: flex;
    gap: 0.5rem;
    align-items: center;
    flex: none;
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
  .pm-error {
    color: var(--fb-error, #b00020);
    font-size: 0.85rem;
  }
  .pm-footer {
    display: flex;
    justify-content: flex-end;
    margin-top: 0.8rem;
  }
</style>

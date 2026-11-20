<!--
  Email dialog (TASK-08-006, PLAN §2J Email): pick the longest edge of the
  emailed copies (default 1600 px), then the backend resizes into a scratch
  folder (edits baked in, originals untouched) and opens the OS mail flow.
-->
<script lang="ts">
  import { t } from "./i18n.svelte.js";
  import {
    DEFAULT_EMAIL_EDGE,
    EMAIL_SIZES,
    emailPhotos,
    type EmailReply,
  } from "./email-share";

  let {
    open = $bindable(false),
    ids = $bindable<number[]>([]),
  }: { open?: boolean; ids?: number[] } = $props();

  let maxEdge = $state(DEFAULT_EMAIL_EDGE);
  let busy = $state(false);
  let reply = $state<EmailReply | null>(null);
  let error = $state<string | null>(null);

  let canRun = $derived(!busy && ids.length > 0);

  async function run(): Promise<void> {
    if (!canRun) return;
    busy = true;
    error = null;
    reply = null;
    try {
      reply = await emailPhotos(ids, Number(maxEdge));
    } catch (e) {
      error = t("email.error", { error: String(e) });
    } finally {
      busy = false;
    }
  }
</script>

{#if open}
  <div class="em-backdrop" data-testid="email-backdrop">
    <div class="em-dialog" role="dialog" aria-label={t("email.title")}>
      <h2>{t("email.title")}</h2>
      <p>{t("email.count", { n: ids.length })}</p>

      <label>
        {t("email.size")}
        <select data-testid="email-size" bind:value={maxEdge} disabled={busy}>
          {#each EMAIL_SIZES as size (size)}
            <option value={size}>{t("email.size.option", { n: size })}</option>
          {/each}
        </select>
      </label>

      {#if busy}
        <p data-testid="email-progress">{t("email.running")}</p>
      {/if}
      {#if reply}
        <p data-testid="email-result">
          {reply.delivery === "attached"
            ? t("email.result.attached", { n: reply.attached })
            : t("email.result.mailto")}
          {#if reply.failed > 0}
            {t("email.result.failed", { n: reply.failed })}
          {/if}
        </p>
      {/if}
      {#if error}
        <p class="em-error" role="alert" data-testid="email-error">{error}</p>
      {/if}

      <div class="em-actions">
        <button type="button" data-testid="email-run" disabled={!canRun} onclick={run}>
          {busy ? t("email.running") : t("email.run")}
        </button>
        <button type="button" data-testid="email-close" onclick={() => (open = false)}>
          {t("email.close")}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .em-backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
    background: var(--fb-backdrop, rgb(0 0 0 / 40%));
    display: grid;
    place-items: center;
  }
  .em-dialog {
    background: var(--fb-surface, #fff);
    color: var(--fb-text, #111);
    border-radius: 8px;
    min-width: 22rem;
    max-width: 90vw;
    padding: 1rem 1.25rem;
    box-shadow: 0 8px 32px var(--fb-shadow, rgb(0 0 0 / 30%));
    text-align: left;
    display: grid;
    gap: 0.5rem;
  }
  h2,
  p {
    margin: 0;
  }
  .em-error {
    color: var(--fb-error, #b00020);
    font-size: 0.85rem;
  }
  .em-actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 0.4rem;
  }
</style>

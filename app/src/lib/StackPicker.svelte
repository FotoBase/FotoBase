<!--
  Stack picker (TASK-03-021): the small overlay a stack's badge opens. Lists
  every photo of the stack, top first; any member can become the top (the
  photo the grid shows), and the whole stack can be dissolved. Stacking is
  metadata-only, so nothing here touches a file.
-->
<script lang="ts">
  import type { GridItem } from "./grid";
  import { thumbUri } from "./grid";
  import { gridState } from "./grid-state.svelte.js";
  import { t } from "./i18n.svelte.js";
  import { listStack, setStackTop, unstack } from "./stacks";

  let {
    open = $bindable(false),
    stackId,
    x,
    y,
    onchanged,
  }: {
    open?: boolean;
    stackId: string;
    x: number;
    y: number;
    /// The top changed or the stack dissolved: the shell re-reads the grid.
    onchanged: () => void;
  } = $props();

  let members = $state<GridItem[]>([]);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let loadSeq = 0;

  async function load(): Promise<void> {
    const seq = ++loadSeq;
    try {
      const rows = await listStack(stackId);
      if (seq === loadSeq) members = rows;
    } catch (e) {
      if (seq === loadSeq) error = t("stack.error", { error: String(e) });
    }
  }

  $effect(() => {
    if (!open) return;
    error = null;
    members = [];
    void load();
  });

  // Escape and any pointer-down outside close the overlay, like the context
  // menu; the overlay stops its own pointer events.
  $effect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") open = false;
    };
    const onOutside = (): void => {
      open = false;
    };
    window.addEventListener("keydown", onKey);
    document.addEventListener("pointerdown", onOutside);
    return () => {
      window.removeEventListener("keydown", onKey);
      document.removeEventListener("pointerdown", onOutside);
    };
  });

  async function run(change: () => Promise<void>, closeAfter: boolean): Promise<void> {
    if (busy) return;
    busy = true;
    error = null;
    try {
      await change();
      onchanged();
      if (closeAfter) open = false;
      else await load();
    } catch (e) {
      error = t("stack.error", { error: String(e) });
    } finally {
      busy = false;
    }
  }
</script>

{#if open}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="stack-picker"
    data-testid="stack-picker"
    role="dialog"
    tabindex="-1"
    aria-label={t("stack.picker.title", { n: members.length })}
    style="left: {x}px; top: {y}px"
    onpointerdown={(e) => e.stopPropagation()}
  >
    <h3>{t("stack.picker.title", { n: members.length })}</h3>
    <ul>
      {#each members as m, i (m.id)}
        <li data-testid="stack-member" data-top={i === 0 ? "true" : undefined}>
          {#if thumbUri(m.hash)}
            <img
              src={thumbUri(m.hash, gridState.thumbVersions[m.hash ?? ""] ?? 0) ?? ""}
              alt=""
              draggable="false"
            />
          {:else}
            <span class="stack-thumb-placeholder"></span>
          {/if}
          <span class="stack-name" title={m.filename}>{m.filename}</span>
          {#if i === 0}
            <span class="stack-top-mark">{t("stack.picker.top")}</span>
          {:else}
            <button
              type="button"
              data-testid={`stack-make-top-${m.id}`}
              disabled={busy}
              onclick={() => run(() => setStackTop(m.id), false)}
            >
              {t("stack.picker.makeTop")}
            </button>
          {/if}
        </li>
      {/each}
    </ul>
    {#if error}
      <p class="stack-error" role="alert">{error}</p>
    {/if}
    <div class="stack-actions">
      <button
        type="button"
        data-testid="stack-unstack"
        disabled={busy}
        onclick={() => run(() => unstack(stackId), true)}
      >
        {t("stack.picker.unstack")}
      </button>
      <button type="button" onclick={() => (open = false)}>{t("stack.picker.close")}</button>
    </div>
  </div>
{/if}

<style>
  .stack-picker {
    position: fixed;
    z-index: 60;
    width: 22rem;
    max-width: 90vw;
    max-height: 70vh;
    overflow-y: auto;
    padding: 0.6rem 0.75rem;
    background: var(--fb-surface, #fff);
    color: var(--fb-text, #111);
    border: 1px solid var(--fb-border, #ccc);
    border-radius: 8px;
    box-shadow: 0 8px 32px var(--fb-shadow, rgb(0 0 0 / 30%));
    text-align: left;
  }
  h3 {
    margin: 0 0 0.5rem;
    font-size: 0.9rem;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 0.4rem;
  }
  li {
    display: grid;
    grid-template-columns: 3rem 1fr auto;
    align-items: center;
    gap: 0.5rem;
  }
  img,
  .stack-thumb-placeholder {
    width: 3rem;
    height: 3rem;
    object-fit: cover;
    border-radius: 4px;
    background: var(--fb-surface-alt, #ececec);
  }
  .stack-name {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .stack-top-mark {
    font-size: 0.75rem;
    color: var(--fb-accent, #4a9eff);
  }
  .stack-error {
    margin: 0.5rem 0 0;
    color: var(--fb-error, #b00020);
    font-size: 0.85rem;
  }
  .stack-actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 0.6rem;
  }
</style>

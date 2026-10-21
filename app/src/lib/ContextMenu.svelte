<script lang="ts">
  // Context-menu skeleton (TASK-01-015, PLAN §2B): a generic positioned
  // menu. P1 entries are enabled and emit their id on pick; P2 entries
  // render disabled as placeholders until their tasks land (01-021 wires
  // the overlapping file-menu commands).
  import type { MenuItem } from "./folder-tree";

  let {
    open = $bindable(false),
    x,
    y,
    items,
    onpick,
  }: {
    open?: boolean;
    x: number;
    y: number;
    items: MenuItem[];
    onpick: (id: string) => void;
  } = $props();

  function pick(item: MenuItem): void {
    if (item.disabled) return;
    open = false;
    onpick(item.id);
  }

  // Open menu: Escape and any pointer-down outside the menu close it. The
  // menu stops its own pointer events so picking an item never races the
  // outside-click handler.
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
</script>

{#if open}
  <ul
    class="context-menu"
    data-testid="context-menu"
    role="menu"
    style:left="{x}px"
    style:top="{y}px"
    onpointerdown={(e) => e.stopPropagation()}
  >
    {#each items as item (item.id)}
      <li role="none">
        <button
          type="button"
          role="menuitem"
          data-testid="menu-item-{item.id}"
          disabled={item.disabled}
          onclick={() => pick(item)}
        >
          {item.label}
        </button>
      </li>
    {/each}
  </ul>
{/if}

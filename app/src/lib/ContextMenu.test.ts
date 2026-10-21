// ContextMenu skeleton tests (TASK-01-015): the menu opens at coordinates,
// items are pickable when enabled (P1 commands), and P2 placeholder entries
// render disabled. Escape and outside clicks close it.

import { flushSync } from "svelte";
import { cleanup, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import ContextMenu from "./ContextMenu.svelte";
import type { MenuItem } from "./folder-tree";

afterEach(() => cleanup());

const ITEMS: MenuItem[] = [
  { id: "view-slideshow", label: "View slideshow" },
  { id: "refresh-thumbnails", label: "Refresh thumbnails" },
  { id: "locate-on-disk", label: "Locate on disk" },
  { id: "remove-from-fotobase", label: "Remove from Fotobase" },
  { id: "add-to-album", label: "Add to album", disabled: true },
];

describe("ContextMenu", () => {
  it("renders nothing when closed", () => {
    render(ContextMenu, { open: false, x: 0, y: 0, items: ITEMS, onpick: vi.fn() });
    expect(screen.queryByTestId("context-menu")).toBeNull();
  });

  it("renders every item when open, at the given coordinates", () => {
    render(ContextMenu, { open: true, x: 120, y: 80, items: ITEMS, onpick: vi.fn() });
    const menu = screen.getByTestId("context-menu") as HTMLElement;
    expect(menu.style.left).toBe("120px");
    expect(menu.style.top).toBe("80px");
    for (const item of ITEMS) {
      expect(screen.getByTestId(`menu-item-${item.id}`)).toBeTruthy();
    }
  });

  it("P2 placeholder entries render disabled and are not pickable", () => {
    const onpick = vi.fn();
    render(ContextMenu, { open: true, x: 0, y: 0, items: ITEMS, onpick });
    const placeholder = screen.getByTestId("menu-item-add-to-album") as HTMLButtonElement;
    expect(placeholder.disabled).toBe(true);
    placeholder.click();
    flushSync();
    expect(onpick).not.toHaveBeenCalled();
  });

  it("picking an enabled item emits its id and closes the menu", () => {
    const onpick = vi.fn();
    render(ContextMenu, { open: true, x: 0, y: 0, items: ITEMS, onpick });
    (screen.getByTestId("menu-item-refresh-thumbnails") as HTMLElement).click();
    flushSync();
    expect(onpick).toHaveBeenCalledWith("refresh-thumbnails");
    expect(screen.queryByTestId("context-menu")).toBeNull();
  });

  it("Escape closes the menu without picking", () => {
    render(ContextMenu, { open: true, x: 0, y: 0, items: ITEMS, onpick: vi.fn() });
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    flushSync();
    expect(screen.queryByTestId("context-menu")).toBeNull();
  });

  it("a click outside closes the menu without picking", () => {
    render(ContextMenu, { open: true, x: 0, y: 0, items: ITEMS, onpick: vi.fn() });
    document.body.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true }));
    flushSync();
    expect(screen.queryByTestId("context-menu")).toBeNull();
  });
});

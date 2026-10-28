// Keyboard shortcuts (TASK-01-024, PLAN §2M "Full keyboard control").
// `matchShortcut` is pure (unit-tested here); the Svelte action wires the
// matcher to a real window listener so components only register handlers.

import { describe, expect, it, vi } from "vitest";

import { matchShortcut, shortcuts } from "./shortcuts";

interface Mods {
  ctrl?: boolean;
  meta?: boolean;
  alt?: boolean;
  shift?: boolean;
}

const ev = (key: string, mods: Mods = {}) => ({
  key,
  ctrlKey: mods.ctrl ?? false,
  metaKey: mods.meta ?? false,
  altKey: mods.alt ?? false,
  shiftKey: mods.shift ?? false,
});

describe("matchShortcut", () => {
  it("Cmd+F focuses search on macOS, Ctrl+F elsewhere", () => {
    expect(matchShortcut(ev("f", { meta: true }), true)).toBe("search-focus");
    expect(matchShortcut(ev("f", { ctrl: true }), false)).toBe("search-focus");
  });

  it("the wrong platform modifier never matches", () => {
    expect(matchShortcut(ev("f", { ctrl: true }), true)).toBeNull();
    expect(matchShortcut(ev("f", { meta: true }), false)).toBeNull();
  });

  it("plain keys type — they are never shortcuts", () => {
    expect(matchShortcut(ev("f"), true)).toBeNull();
    expect(matchShortcut(ev("q"), false)).toBeNull();
  });

  it("Cmd/Ctrl+Q exits", () => {
    expect(matchShortcut(ev("q", { meta: true }), true)).toBe("exit");
    expect(matchShortcut(ev("q", { ctrl: true }), false)).toBe("exit");
  });

  it("Cmd/Ctrl+Shift+T clears the unheld tray items (TASK-03-010)", () => {
    expect(matchShortcut(ev("T", { meta: true, shift: true }), true)).toBe("tray-clear");
    expect(matchShortcut(ev("t", { ctrl: true, shift: true }), false)).toBe("tray-clear");
    // Without shift (browser/new-tab territory) or with alt it stays unmatched.
    expect(matchShortcut(ev("t", { ctrl: true }), false)).toBeNull();
    expect(matchShortcut(ev("T", { ctrl: true, shift: true, alt: true }), false)).toBeNull();
  });

  it("alt/shift chords do not match (they mean something else)", () => {
    expect(matchShortcut(ev("f", { meta: true, alt: true }), true)).toBeNull();
    expect(matchShortcut(ev("f", { meta: true, shift: true }), true)).toBeNull();
  });

  it("unmapped chords return null", () => {
    expect(matchShortcut(ev("x", { ctrl: true }), false)).toBeNull();
  });
});

describe("shortcuts action", () => {
  it("dispatches matched actions to handlers and removes them on destroy", () => {
    const onSearch = vi.fn();
    const action = shortcuts(document.body, { "search-focus": onSearch });

    window.dispatchEvent(
      new KeyboardEvent("keydown", { key: "f", ctrlKey: true, bubbles: true }),
    );
    expect(onSearch).toHaveBeenCalledTimes(1);

    action.destroy();
    window.dispatchEvent(
      new KeyboardEvent("keydown", { key: "f", ctrlKey: true, bubbles: true }),
    );
    expect(onSearch).toHaveBeenCalledTimes(1);
  });

  it("prevents the browser default for handled shortcuts only", () => {
    const handled = new KeyboardEvent("keydown", {
      key: "f",
      ctrlKey: true,
      bubbles: true,
      cancelable: true,
    });
    const unhandled = new KeyboardEvent("keydown", { key: "x", bubbles: true });
    const action = shortcuts(document.body, { "search-focus": () => {} });
    window.dispatchEvent(handled);
    window.dispatchEvent(unhandled);
    expect(handled.defaultPrevented).toBe(true);
    expect(unhandled.defaultPrevented).toBe(false);
    action.destroy();
  });

  it("unmatched actions are ignored, not errors", () => {
    const action = shortcuts(document.body, {});
    expect(() =>
      window.dispatchEvent(
        new KeyboardEvent("keydown", { key: "q", ctrlKey: true, bubbles: true }),
      ),
    ).not.toThrow();
    action.destroy();
  });
});

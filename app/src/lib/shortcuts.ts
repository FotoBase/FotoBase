// Keyboard shortcuts (TASK-01-024, PLAN §2M "Full keyboard control").
//
// Two layers:
// - `matchShortcut` is pure: it maps a KeyboardEvent to an action id, or
//   null. Platform-aware (Cmd on macOS, Ctrl elsewhere) and chord-strict
//   (alt/shift never participate) so plain typing never triggers anything.
// - `shortcuts` is a Svelte action: attach it to an element that stays
//   mounted (the app shell), hand it the handlers for the *global* actions.
//   Component-local keys (grid arrows, viewer navigation) live in their own
//   components — a global action must not fire while the viewer already
//   consumes the same key.
//
// The full user-facing list is documented in docs/help/shortcuts.md.

export type ShortcutAction = "search-focus" | "exit" | "tray-clear";

/// The modifier for this platform: Cmd on macOS, Ctrl elsewhere.
function platformModifier(e: ShortcutEvent, mac: boolean): boolean {
  return mac ? e.metaKey : e.ctrlKey;
}

/// Minimal structural subset of KeyboardEvent the matcher needs — keeps the
/// pure function testable with plain objects.
export interface ShortcutEvent {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}

/// Map a keydown to a global shortcut action, or null when the key is not a
/// shortcut (typing, plain navigation — must never be swallowed).
export function matchShortcut(e: ShortcutEvent, mac: boolean): ShortcutAction | null {
  // Chords with extra modifiers mean something else (browser, OS, text
  // editing) — never match them. The one shifted chord is the tray clear.
  if (e.altKey) return null;
  if (!platformModifier(e, mac)) return null;
  if (e.shiftKey) return e.key === "t" || e.key === "T" ? "tray-clear" : null;
  switch (e.key) {
    case "f":
    case "F":
      return "search-focus";
    case "q":
    case "Q":
      return "exit";
    default:
      return null;
  }
}

/// Handlers for the actions this element cares about. Unregistered actions
/// are ignored (another layer owns them).
export type ShortcutHandlers = Partial<Record<ShortcutAction, (e: KeyboardEvent) => void>>;

function isMac(): boolean {
  try {
    return /mac/i.test(navigator.platform);
  } catch {
    return false;
  }
}

/// Svelte action: listen for the global shortcuts on `window` (they must
/// work regardless of which element holds focus) and dispatch to handlers.
/// `update` re-binds when the handlers object changes (e.g. the shell
/// re-renders); returns a destroy function, per the Svelte action contract.
export function shortcuts(node: HTMLElement, handlers: ShortcutHandlers): {
  update(handlers: ShortcutHandlers): void;
  destroy(): void;
} {
  const mac = isMac();
  let current = handlers;
  const onKeydown = (e: KeyboardEvent): void => {
    const action = matchShortcut(e, mac);
    if (action === null) return;
    const handler = current[action];
    if (!handler) return;
    e.preventDefault();
    handler(e);
  };
  window.addEventListener("keydown", onKeydown);
  return {
    update(handlers: ShortcutHandlers) {
      current = handlers;
    },
    destroy() {
      window.removeEventListener("keydown", onKeydown);
    },
  };
}

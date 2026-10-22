// Theme resolution (TASK-01-024, PLAN §2 additions "Dark mode / HiDPI:
// native theming"). The decision logic is pure so the E2E-relevant behavior
// — toggling the OS theme swaps the CSS variables — reduces to a data-theme
// attribute the stylesheet keys off.

import { describe, expect, it } from "vitest";

import {
  applyTheme,
  resolveTheme,
  systemPrefersDark,
  watchSystemTheme,
  type ThemeSetting,
} from "./theme";

describe("resolveTheme", () => {
  it("'system' follows the OS setting", () => {
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("system", false)).toBe("light");
  });

  it("explicit light/dark overrides the OS", () => {
    expect(resolveTheme("light", true)).toBe("light");
    expect(resolveTheme("dark", false)).toBe("dark");
  });

  it("unknown settings degrade to system, never crash", () => {
    expect(resolveTheme("nonsense" as ThemeSetting, true)).toBe("dark");
    expect(resolveTheme("nonsense" as ThemeSetting, false)).toBe("light");
  });
});

describe("applyTheme", () => {
  it("writes data-theme on the document root (the CSS hook)", () => {
    applyTheme("light");
    expect(document.documentElement.dataset.theme).toBe("light");
    applyTheme("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
  });
});

describe("systemPrefersDark", () => {
  it("returns a boolean without throwing in any environment", () => {
    expect(typeof systemPrefersDark()).toBe("boolean");
  });
});

describe("watchSystemTheme", () => {
  it("returns an unsubscribe function", () => {
    const stop = watchSystemTheme(() => {});
    expect(typeof stop).toBe("function");
    stop();
  });
});

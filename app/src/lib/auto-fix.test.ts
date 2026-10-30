// TASK-04-007 tests: op emission for the one-click auto fixes. I'm Feeling
// Lucky is the combined auto color + contrast pass; Auto Contrast and Auto
// Color are each their own single op. Default state emits no ops.

import { describe, expect, it } from "vitest";

import { opsFromState, type AutoFixState } from "./auto-fix";

describe("opsFromState", () => {
  it("default state emits no ops (untouched photo)", () => {
    expect(opsFromState({ autoColor: false, autoContrast: false })).toEqual([]);
  });

  it("auto color alone emits a single auto_color op", () => {
    const ops = opsFromState({ autoColor: true, autoContrast: false });
    expect(ops).toHaveLength(1);
    expect(ops[0].type).toBe("auto_color");
  });

  it("auto contrast alone emits a single auto_contrast op", () => {
    const ops = opsFromState({ autoColor: false, autoContrast: true });
    expect(ops).toHaveLength(1);
    expect(ops[0].type).toBe("auto_contrast");
  });

  it("I'm Feeling Lucky (both on) emits color pass then contrast pass", () => {
    const ops = opsFromState({ autoColor: true, autoContrast: true });
    expect(ops.map((o) => o.type)).toEqual(["auto_color", "auto_contrast"]);
  });
});

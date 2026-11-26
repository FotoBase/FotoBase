import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import ProPanel from "./ProPanel.svelte";

afterEach(cleanup);

describe("ProPanel", () => {
  it("edits a curve point and emits the changed recipe state", async () => {
    const onchange = vi.fn();
    render(ProPanel, { previewSrc: "preview://abc", onchange, ops: [], onreorder: vi.fn(), onduplicate: vi.fn(), onremove: vi.fn(), detectLens: vi.fn() });
    await waitFor(() => expect(onchange).toHaveBeenCalled());
    await fireEvent.click(screen.getByTestId("pro-curves"));
    await fireEvent.input(screen.getByTestId("pro-curve-y-1"), {
      target: { value: "0.7" },
    });
    expect(onchange).toHaveBeenLastCalledWith(expect.objectContaining({
      curves: { enabled: true, points: [[0, 0], [0.5, 0.7], [1, 1]] },
    }));
  });

  it("requests an operation move from the visible recipe stack", async () => {
    const onreorder = vi.fn();
    render(ProPanel, {
      previewSrc: "preview://abc",
      onchange: vi.fn(),
      ops: [{ type: "exposure", params: { ev: 1 } }, { type: "bw", params: {} }],
      onreorder,
      onduplicate: vi.fn(),
      onremove: vi.fn(),
      detectLens: vi.fn(),
    });
    await fireEvent.click(screen.getByTestId("pro-move-up-1"));
    expect(onreorder).toHaveBeenCalledWith(1, 0);
  });

  it("requests a copy of an operation", async () => {
    const onduplicate = vi.fn();
    render(ProPanel, {
      previewSrc: "preview://abc",
      onchange: vi.fn(),
      ops: [{ type: "exposure", params: { ev: 1 } }],
      onreorder: vi.fn(),
      onduplicate,
      onremove: vi.fn(),
      detectLens: vi.fn(),
    });
    await fireEvent.click(screen.getByTestId("pro-duplicate-0"));
    expect(onduplicate).toHaveBeenCalledWith(0);
  });

  it("requests removal of a selected operation", async () => {
    const onremove = vi.fn();
    render(ProPanel, {
      previewSrc: "preview://abc",
      onchange: vi.fn(),
      ops: [{ type: "exposure", params: { ev: 1 } }],
      onreorder: vi.fn(),
      onduplicate: vi.fn(),
      onremove,
      detectLens: vi.fn(),
    });
    await fireEvent.click(screen.getByTestId("pro-remove-0"));
    expect(onremove).toHaveBeenCalledWith(0);
  });

  it("detects the lens from EXIF and toggles each Lensfun correction", async () => {
    const profile = {
      profile: "<lensdatabase/>", lens: "Canon EF 50mm f/1.8 II", crop: 1, focal: 50,
      aperture: 2.8, distortion: true, vignetting: true, tca: true,
    };
    const onchange = vi.fn();
    render(ProPanel, {
      previewSrc: "preview://abc", onchange, ops: [], onreorder: vi.fn(),
      onduplicate: vi.fn(), onremove: vi.fn(), detectLens: vi.fn().mockResolvedValue(profile),
    });
    await fireEvent.click(screen.getByTestId("pro-lens-detect"));
    await waitFor(() => expect(onchange).toHaveBeenLastCalledWith(
      expect.objectContaining({ lensCorrection: profile })));
    expect(screen.getByTestId("pro-lens-name").textContent).toContain("Canon EF 50mm f/1.8 II");

    await fireEvent.click(screen.getByTestId("pro-lens-vignetting"));
    expect(onchange).toHaveBeenLastCalledWith(expect.objectContaining({
      lensCorrection: { ...profile, vignetting: false } }));

    await fireEvent.click(screen.getByTestId("pro-lens-clear"));
    expect(onchange).toHaveBeenLastCalledWith(expect.objectContaining({ lensCorrection: null }));
  });

  it("shows why no lens profile was found instead of guessing one", async () => {
    const onchange = vi.fn();
    render(ProPanel, {
      previewSrc: "preview://abc", onchange, ops: [], onreorder: vi.fn(), onduplicate: vi.fn(),
      onremove: vi.fn(),
      detectLens: vi.fn().mockRejectedValue("lens \"Foo 13mm\" is not in the Lensfun database"),
    });
    await fireEvent.click(screen.getByTestId("pro-lens-detect"));
    await waitFor(() => expect(screen.getByTestId("pro-lens-error").textContent)
      .toContain("Foo 13mm"));
    expect(onchange).not.toHaveBeenLastCalledWith(
      expect.objectContaining({ lensCorrection: expect.anything() }));
  });
});

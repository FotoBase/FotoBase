// TASK-08-004 tests: the print dialog — layouts incl. contact-sheet
// images-per-page, shrink/crop fit, border/text options, copies per photo,
// a live paper preview drawn from the backend page plan. TASK-08-005 adds
// the destination: installed printers (with the paper-size mismatch
// warning), export to PDF, and progress/cancel feedback. IPC is mocked at
// the Tauri core and event modules.

import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import PrintDialog from "./PrintDialog.svelte";
import { setLocale } from "./i18n.svelte.js";
import type { CellPlan, PrintPlan } from "./print";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...a: unknown[]) => invoke(...a),
}));

let progressCb: ((e: { payload: { done: number } }) => void) | null = null;
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (_name: string, cb: (e: { payload: { done: number } }) => void) => {
    progressCb = cb;
    return () => {
      progressCb = null;
    };
  },
}));

const PRINTERS = [
  { name: "Office", is_default: false, papers: ["a4"], default_paper: "a4", mismatch: ["letter"] },
  { name: "Lab", is_default: true, papers: ["letter", "a4"], default_paper: "letter", mismatch: [] },
];

function cell(file_id: number, x: number): CellPlan {
  const box = { x, y: 2.5, w: 4, h: 6 };
  return {
    file_id,
    cell: box,
    image_box: box,
    fit: { src: { x: 0, y: 0, w: 1, h: 1 }, dst: box },
    rotated: false,
    border: null,
    text: null,
  };
}

// 2 photos × 2 copies on 4×6: two cells per sheet, two sheets.
const PLAN: PrintPlan = {
  pages: [
    { width: 8.5, height: 11, cells: [cell(1, 0.25), cell(1, 4.25)] },
    { width: 8.5, height: 11, cells: [cell(2, 0.25), cell(2, 4.25)] },
  ],
  photos: [
    { file_id: 1, filename: "a.jpg", recipe: "" },
    { file_id: 2, filename: "b.jpg", recipe: '{"spec":"fotobase.recipe"}' },
  ],
};

function lastRequest(command: string): { ids: number[]; request: { copies: number; options: Record<string, any> } } {
  const calls = invoke.mock.calls.filter((c) => c[0] === command);
  return calls[calls.length - 1][1];
}

beforeEach(() => {
  setLocale("en");
  invoke.mockReset();
  invoke.mockImplementation(async (cmd: string, args: any) => {
    if (cmd === "print_plan") return PLAN;
    if (cmd === "render_edit_preview") return `preview://hash${args.fileId}`;
    if (cmd === "print_pages") {
      return { files: ["/out/p-001.png", "/out/p-002.png"], pages: 2, job_id: null, failed: [] };
    }
    return undefined;
  });
});

/** Mock with installed printers and the PDF/printer commands answering. */
function withPrinters(): void {
  const base = invoke.getMockImplementation()!;
  invoke.mockImplementation(async (cmd: string, args: any) => {
    if (cmd === "list_printers") return PRINTERS;
    if (cmd === "print_to_printer") return { files: [], pages: 2, job_id: "Lab-7", failed: [] };
    if (cmd === "export_print_pdf") {
      return { files: ["/out/sheet.pdf"], pages: 2, job_id: null, failed: [] };
    }
    return base(cmd, args);
  });
}
afterEach(cleanup);

describe("PrintDialog", () => {
  it("renders nothing when closed", () => {
    render(PrintDialog, { open: false, ids: [1] });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(invoke).not.toHaveBeenCalled();
  });

  it("requests the plan with 4×6 / shrink defaults and previews every cell", async () => {
    render(PrintDialog, { open: true, ids: [1, 2] });
    await waitFor(() => expect(screen.getAllByTestId("print-cell")).toHaveLength(4));

    const req = lastRequest("print_plan");
    expect(req.ids).toEqual([1, 2]);
    expect(req.request.copies).toBe(1);
    expect(req.request.options.layout).toBe("4x6");
    expect(req.request.options.fit).toBe("shrink");
    expect(screen.getAllByTestId("print-page")).toHaveLength(2);

    // Each photo's edited preview is fetched once, with its recipe.
    await waitFor(() => expect(screen.getAllByRole("img")).toHaveLength(4));
    const previews = invoke.mock.calls.filter((c) => c[0] === "render_edit_preview");
    expect(previews).toHaveLength(2);
    expect(previews[1][1]).toEqual({ fileId: 2, recipe: '{"spec":"fotobase.recipe"}' });
    expect((screen.getAllByRole("img")[0] as HTMLImageElement).src).toContain("preview://hash1");
  });

  it("offers every layout and images-per-page only for the contact sheet", async () => {
    render(PrintDialog, { open: true, ids: [1] });
    const select = screen.getByTestId("print-layout") as HTMLSelectElement;
    expect(Array.from(select.options).map((o) => o.value)).toEqual([
      "wallet", "3.5x5", "4x6", "5x7", "8x10", "full_page", "contact_sheet",
    ]);
    expect(screen.queryByTestId("print-per-page")).toBeNull();

    await fireEvent.change(select, { target: { value: "contact_sheet" } });
    const perPage = await screen.findByTestId("print-per-page");
    await fireEvent.input(perPage, { target: { value: "30" } });
    await waitFor(() => {
      const o = lastRequest("print_plan").request.options;
      expect(o.layout).toBe("contact_sheet");
      expect(o.images_per_page).toBe(30);
    });

    await fireEvent.input(perPage, { target: { value: "500" } });
    await waitFor(() => expect(lastRequest("print_plan").request.options.images_per_page).toBe(100));
  });

  it("sends the fit mode and copies", async () => {
    render(PrintDialog, { open: true, ids: [1, 2] });
    await fireEvent.click(screen.getByTestId("print-fit-crop"));
    await fireEvent.input(screen.getByTestId("print-copies"), { target: { value: "2" } });
    await waitFor(() => {
      const r = lastRequest("print_plan").request;
      expect(r.options.fit).toBe("crop");
      expect(r.copies).toBe(2);
    });
  });

  it("shows the text options only when a text source is chosen", async () => {
    render(PrintDialog, { open: true, ids: [1] });
    expect(screen.queryByTestId("print-text-position")).toBeNull();

    await fireEvent.change(screen.getByTestId("print-text-source"), { target: { value: "exif" } });
    await fireEvent.change(await screen.findByTestId("print-text-position"), {
      target: { value: "above" },
    });
    await fireEvent.click(screen.getByTestId("print-text-wrap"));
    await fireEvent.input(screen.getByTestId("print-border-width"), { target: { value: "0.1" } });
    await waitFor(() => {
      const o = lastRequest("print_plan").request.options;
      expect(o.text.source).toBe("exif");
      expect(o.text.position).toBe("above");
      expect(o.text.wrap).toBe(false);
      expect(o.border.width_in).toBe(0.1);
    });
  });

  it("draws planned text bands and borders in the preview", async () => {
    const withText = structuredClone(PLAN);
    withText.pages[0].cells[0].text = {
      rect: { x: 0.25, y: 8.3, w: 4, h: 0.2 },
      text: "Beach day",
    };
    withText.pages[0].cells[0].border = { x: 0.25, y: 2.5, w: 4, h: 6 };
    invoke.mockImplementation(async (cmd: string) =>
      cmd === "print_plan" ? withText : cmd === "render_edit_preview" ? "preview://x" : undefined,
    );
    render(PrintDialog, { open: true, ids: [1, 2] });
    expect(await screen.findByText("Beach day")).toBeTruthy();
    expect(screen.getAllByTestId("print-border")).toHaveLength(1);
  });

  it("writes the pages into the chosen folder", async () => {
    render(PrintDialog, { open: true, ids: [1, 2] });
    await waitFor(() => expect(screen.getAllByTestId("print-cell")).toHaveLength(4));
    const run = screen.getByTestId("print-run") as HTMLButtonElement;
    expect(run.disabled).toBe(true);

    await fireEvent.input(screen.getByTestId("print-folder"), { target: { value: "/out" } });
    await fireEvent.click(run);
    await waitFor(() => expect(screen.getByTestId("print-done")).toBeTruthy());
    const req = lastRequest("print_pages") as unknown as { dest: string; request: { copies: number } };
    expect(req.dest).toBe("/out");
    expect(screen.getByTestId("print-done").textContent).toContain("2");
  });

  it("reports a failed plan", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "print_plan") throw new Error("boom");
      return undefined;
    });
    render(PrintDialog, { open: true, ids: [1] });
    expect((await screen.findByTestId("print-error")).textContent).toContain("boom");
  });

  describe("destination (TASK-08-005)", () => {
    const destination = () => screen.getByTestId("print-destination") as HTMLSelectElement;

    it("lists the installed printers, the PDF export and the PNG pages, defaulting to the default printer", async () => {
      withPrinters();
      render(PrintDialog, { open: true, ids: [1, 2] });
      await waitFor(() => expect(destination().options.length).toBe(4));
      expect(Array.from(destination().options).map((o) => o.value)).toEqual([
        "printer:Office",
        "printer:Lab",
        "pdf",
        "pages",
      ]);
      expect(destination().value).toBe("printer:Lab");
      // A printer needs no folder or file path.
      expect(screen.queryByTestId("print-folder")).toBeNull();
      expect(screen.queryByTestId("print-pdf-path")).toBeNull();
    });

    it("prints on the chosen printer with the configured layout", async () => {
      withPrinters();
      render(PrintDialog, { open: true, ids: [1, 2] });
      await waitFor(() => expect(destination().value).toBe("printer:Lab"));
      await waitFor(() => expect(screen.getAllByTestId("print-cell")).toHaveLength(4));
      await fireEvent.click(screen.getByTestId("print-fit-crop"));
      await fireEvent.input(screen.getByTestId("print-copies"), { target: { value: "2" } });
      await fireEvent.click(screen.getByTestId("print-run"));
      await waitFor(() => expect(screen.getByTestId("print-done")).toBeTruthy());

      const req = lastRequest("print_to_printer") as unknown as {
        printer: string;
        request: { copies: number; options: Record<string, any> };
      };
      expect(req.printer).toBe("Lab");
      expect(req.request.copies).toBe(2);
      expect(req.request.options.fit).toBe("crop");
      expect(screen.getByTestId("print-done").textContent).toContain("Lab-7");
      expect(invoke.mock.calls.some((c) => c[0] === "print_pages")).toBe(false);
    });

    it("exports a PDF to the typed path with the same options", async () => {
      withPrinters();
      render(PrintDialog, { open: true, ids: [1, 2] });
      await waitFor(() => expect(destination().options.length).toBe(4));
      await fireEvent.change(destination(), { target: { value: "pdf" } });
      const run = screen.getByTestId("print-run") as HTMLButtonElement;
      expect(run.disabled).toBe(true);

      await fireEvent.input(screen.getByTestId("print-pdf-path"), {
        target: { value: "/out/sheet.pdf" },
      });
      await fireEvent.click(run);
      await waitFor(() => expect(screen.getByTestId("print-done")).toBeTruthy());
      const req = lastRequest("export_print_pdf") as unknown as {
        dest: string;
        request: { options: Record<string, any> };
      };
      expect(req.dest).toBe("/out/sheet.pdf");
      expect(req.request.options.layout).toBe("4x6");
      expect(screen.getByTestId("print-done").textContent).toContain("/out/sheet.pdf");
    });

    it("warns about a paper-size mismatch before printing", async () => {
      withPrinters();
      render(PrintDialog, { open: true, ids: [1] });
      await waitFor(() => expect(destination().value).toBe("printer:Lab"));
      expect(screen.queryByTestId("print-paper-warning")).toBeNull();

      // Letter (the default paper) on the A4-only printer.
      await fireEvent.change(destination(), { target: { value: "printer:Office" } });
      const warning = await screen.findByTestId("print-paper-warning");
      expect(warning.textContent).toContain("Office");
      expect(invoke.mock.calls.some((c) => c[0] === "print_to_printer")).toBe(false);

      // Switching the paper to A4 clears it.
      await fireEvent.change(screen.getByTestId("print-paper"), { target: { value: "a4" } });
      await waitFor(() => expect(screen.queryByTestId("print-paper-warning")).toBeNull());
    });

    it("shows sheet progress and cancels the running job", async () => {
      withPrinters();
      const base = invoke.getMockImplementation()!;
      let release: (v: unknown) => void = () => {};
      invoke.mockImplementation(async (cmd: string, args: any) => {
        if (cmd === "print_to_printer") return await new Promise((r) => (release = r));
        return base(cmd, args);
      });
      render(PrintDialog, { open: true, ids: [1, 2] });
      await waitFor(() => expect(destination().value).toBe("printer:Lab"));
      await waitFor(() => expect(screen.getAllByTestId("print-page")).toHaveLength(2));
      await fireEvent.click(screen.getByTestId("print-run"));
      await waitFor(() => expect(progressCb).not.toBeNull());

      progressCb?.({ payload: { done: 1 } });
      await waitFor(() =>
        expect(screen.getByTestId("print-progress").textContent).toContain("1"),
      );
      await fireEvent.click(screen.getByTestId("print-cancel"));
      await waitFor(() =>
        expect(invoke.mock.calls.some((c) => c[0] === "cancel_print")).toBe(true),
      );

      release(Promise.reject(new Error("print cancelled")));
      expect((await screen.findByTestId("print-error")).textContent).toContain("Cancelled");
    });

    it("falls back to the PNG pages when no printer is installed", async () => {
      render(PrintDialog, { open: true, ids: [1] });
      await waitFor(() => expect(destination().value).toBe("pages"));
      expect(screen.getByTestId("print-folder")).toBeTruthy();
    });
  });
});

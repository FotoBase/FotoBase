// TASK-03-020 tests: info-pane metadata editing — date, GPS, artist,
// copyright. Pure model (draft ↔ edit mapping) plus the editor component.
// The backend validates and writes the sidecar; IPC is mocked at the
// wrapper module.

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import MetadataEditor from "./MetadataEditor.svelte";
import { locales, setLocale, t } from "./i18n.svelte.js";
import type { PhotoInfo } from "./info-pane";
import {
  buildEdit,
  draftFromInfo,
  editPhotoMetadata,
  formatCoordinate,
  type EditResult,
} from "./metadata-edit";

vi.mock("./metadata-edit", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./metadata-edit")>()),
  editPhotoMetadata: vi.fn(),
}));

const INFO: PhotoInfo = {
  id: 7,
  path: "/photos/a.jpg",
  filename: "a.jpg",
  size: 10,
  mtime: 0,
  width: null,
  height: null,
  media_type: "image",
  camera: null,
  lens: null,
  exposure: null,
  f_number: null,
  iso: null,
  focal_length: null,
  taken_at: "2024-06-01T12:00:00.25+02:00",
  latitude: 52.5333,
  longitude: -13.405,
  artist: "Ada",
  copyright: "© Ada",
};

const BARE: PhotoInfo = {
  ...INFO,
  taken_at: null,
  latitude: null,
  longitude: null,
  artist: null,
  copyright: null,
};

// --- Pure model -------------------------------------------------------------

describe("metadata edit model", () => {
  it("formats coordinates as decimal degrees, six places, trailing zeros kept short", () => {
    expect(formatCoordinate(52.5333)).toBe("52.5333");
    expect(formatCoordinate(-13.405)).toBe("-13.405");
    expect(formatCoordinate(0)).toBe("0");
    expect(formatCoordinate(52.123456789)).toBe("52.123457");
    expect(formatCoordinate(null)).toBe("");
  });

  it("drafts the wall clock without fraction or offset", () => {
    expect(draftFromInfo(INFO)).toEqual({
      takenAt: "2024-06-01 12:00:00",
      latitude: "52.5333",
      longitude: "-13.405",
      artist: "Ada",
      copyright: "© Ada",
    });
    expect(draftFromInfo(BARE)).toEqual({
      takenAt: "",
      latitude: "",
      longitude: "",
      artist: "",
      copyright: "",
    });
  });

  it("an unreadable stored date drafts as typed text, so nothing is lost", () => {
    expect(draftFromInfo({ ...INFO, taken_at: "sometime" }).takenAt).toBe("sometime");
  });

  it("an unchanged draft produces no edit", () => {
    expect(buildEdit(INFO, draftFromInfo(INFO))).toEqual({});
  });

  it("only changed fields are sent", () => {
    const draft = { ...draftFromInfo(INFO), artist: "Grace", takenAt: "2024-06-02 08:00:00" };
    expect(buildEdit(INFO, draft)).toEqual({
      taken_at: "2024-06-02 08:00:00",
      artist: "Grace",
    });
  });

  it("changing either coordinate sends both as typed", () => {
    const draft = { ...draftFromInfo(INFO), latitude: `52°31'59.9"N` };
    expect(buildEdit(INFO, draft)).toEqual({
      gps: { kind: "set", latitude: `52°31'59.9"N`, longitude: "-13.405" },
    });
  });

  it("emptying both coordinates clears GPS; emptying them on a photo without GPS does nothing", () => {
    const cleared = { ...draftFromInfo(INFO), latitude: "  ", longitude: "" };
    expect(buildEdit(INFO, cleared)).toEqual({ gps: { kind: "clear" } });
    expect(buildEdit(BARE, draftFromInfo(BARE))).toEqual({});
  });

  it("one coordinate alone is still sent, for the backend to refuse inline", () => {
    const draft = { ...draftFromInfo(BARE), latitude: "52" };
    expect(buildEdit(BARE, draft)).toEqual({
      gps: { kind: "set", latitude: "52", longitude: "" },
    });
  });

  it("emptying artist or copyright sends a blank to clear it", () => {
    const draft = { ...draftFromInfo(INFO), artist: "", copyright: "" };
    expect(buildEdit(INFO, draft)).toEqual({ artist: "", copyright: "" });
  });
});

// --- Component --------------------------------------------------------------

const ok = (info: PhotoInfo): EditResult => ({ errors: [], info });

beforeEach(() => {
  setLocale("en");
  vi.mocked(editPhotoMetadata).mockReset();
});

afterEach(cleanup);

function renderEditor(info: PhotoInfo | null = INFO, onsaved = vi.fn()) {
  render(MetadataEditor, { info, onsaved });
  return onsaved;
}

const field = (id: string): HTMLInputElement => screen.getByTestId(id) as HTMLInputElement;

async function startEditing() {
  await fireEvent.click(screen.getByTestId("meta-edit"));
}

describe("MetadataEditor", () => {
  it("shows the four values read-only until editing starts", () => {
    renderEditor();
    expect(screen.getByTestId("meta-gps").textContent?.trim()).toBe("52.5333, -13.405");
    expect(screen.getByTestId("meta-artist").textContent?.trim()).toBe("Ada");
    expect(screen.getByTestId("meta-copyright").textContent?.trim()).toBe("© Ada");
    expect(screen.queryByTestId("meta-input-artist")).toBeNull();
  });

  it("shows dashes for missing values", () => {
    renderEditor(BARE);
    expect(screen.getByTestId("meta-gps").textContent?.trim()).toBe("—");
    expect(screen.getByTestId("meta-artist").textContent?.trim()).toBe("—");
  });

  it("cannot edit without a photo", () => {
    renderEditor(null);
    expect((screen.getByTestId("meta-edit") as HTMLButtonElement).disabled).toBe(true);
  });

  it("edit mode prefills the fields from the photo", async () => {
    renderEditor();
    await startEditing();
    expect(field("meta-input-date").value).toBe("2024-06-01 12:00:00");
    expect(field("meta-input-lat").value).toBe("52.5333");
    expect(field("meta-input-lon").value).toBe("-13.405");
    expect(field("meta-input-artist").value).toBe("Ada");
    expect(field("meta-input-copyright").value).toBe("© Ada");
  });

  it("save sends only what changed and shows what the backend stored", async () => {
    const stored = { ...INFO, artist: "Grace" };
    vi.mocked(editPhotoMetadata).mockResolvedValue(ok(stored));
    const onsaved = renderEditor();
    await startEditing();

    await fireEvent.input(field("meta-input-artist"), { target: { value: "Grace" } });
    await fireEvent.click(screen.getByTestId("meta-save"));

    await waitFor(() => expect(onsaved).toHaveBeenCalledWith(stored));
    expect(editPhotoMetadata).toHaveBeenCalledWith(7, { artist: "Grace" });
    expect(screen.getByTestId("meta-artist").textContent?.trim()).toBe("Grace");
    expect(screen.queryByTestId("meta-input-artist")).toBeNull();
  });

  it("saving without changes closes the editor and writes nothing", async () => {
    renderEditor();
    await startEditing();
    await fireEvent.click(screen.getByTestId("meta-save"));
    expect(editPhotoMetadata).not.toHaveBeenCalled();
    expect(screen.queryByTestId("meta-input-artist")).toBeNull();
  });

  it("rejected input stays in edit mode with the error beside its field", async () => {
    vi.mocked(editPhotoMetadata).mockResolvedValue({
      errors: [
        { field: "latitude", message: "out of range: at most ±90°" },
        { field: "taken_at", message: "Enter a real date and time" },
      ],
      info: INFO,
    });
    const onsaved = renderEditor();
    await startEditing();
    await fireEvent.input(field("meta-input-lat"), { target: { value: "95" } });
    await fireEvent.click(screen.getByTestId("meta-save"));

    await waitFor(() =>
      expect(screen.getByTestId("meta-error-lat").textContent).toContain("at most ±90°"),
    );
    expect(screen.getByTestId("meta-error-date").textContent).toContain("real date");
    expect(screen.queryByTestId("meta-error-lon")).toBeNull();
    expect(field("meta-input-lat").value).toBe("95");
    expect(field("meta-input-lat").getAttribute("aria-invalid")).toBe("true");
    expect(onsaved).not.toHaveBeenCalled();
  });

  it("editing a field clears its error", async () => {
    vi.mocked(editPhotoMetadata).mockResolvedValue({
      errors: [{ field: "latitude", message: "bad" }],
      info: INFO,
    });
    renderEditor();
    await startEditing();
    await fireEvent.input(field("meta-input-lat"), { target: { value: "95" } });
    await fireEvent.click(screen.getByTestId("meta-save"));
    await waitFor(() => screen.getByTestId("meta-error-lat"));

    await fireEvent.input(field("meta-input-lat"), { target: { value: "45" } });
    expect(screen.queryByTestId("meta-error-lat")).toBeNull();
  });

  it("cancel discards the draft", async () => {
    renderEditor();
    await startEditing();
    await fireEvent.input(field("meta-input-artist"), { target: { value: "Grace" } });
    await fireEvent.click(screen.getByTestId("meta-cancel"));

    expect(editPhotoMetadata).not.toHaveBeenCalled();
    expect(screen.getByTestId("meta-artist").textContent?.trim()).toBe("Ada");
    await startEditing();
    expect(field("meta-input-artist").value).toBe("Ada");
  });

  it("Enter in a field saves and Escape cancels", async () => {
    vi.mocked(editPhotoMetadata).mockResolvedValue(ok({ ...INFO, artist: "Grace" }));
    renderEditor();
    await startEditing();
    await fireEvent.input(field("meta-input-artist"), { target: { value: "Grace" } });
    await fireEvent.keyDown(field("meta-input-artist"), { key: "Enter" });
    await waitFor(() => expect(editPhotoMetadata).toHaveBeenCalledTimes(1));

    await startEditing();
    await fireEvent.keyDown(field("meta-input-artist"), { key: "Escape" });
    expect(screen.queryByTestId("meta-input-artist")).toBeNull();
  });

  it("a backend failure is shown and the draft is kept", async () => {
    vi.mocked(editPhotoMetadata).mockRejectedValue("disk full");
    renderEditor();
    await startEditing();
    await fireEvent.input(field("meta-input-artist"), { target: { value: "Grace" } });
    await fireEvent.click(screen.getByTestId("meta-save"));

    await waitFor(() => expect(screen.getByTestId("meta-failure").textContent).toContain("disk full"));
    expect(field("meta-input-artist").value).toBe("Grace");
  });

  it("switching photos leaves edit mode", async () => {
    const { rerender } = render(MetadataEditor, { info: INFO, onsaved: vi.fn() });
    await startEditing();
    await rerender({ info: { ...INFO, id: 8, artist: "Bob" }, onsaved: vi.fn() });
    expect(screen.queryByTestId("meta-input-artist")).toBeNull();
    expect(screen.getByTestId("meta-artist").textContent?.trim()).toBe("Bob");
  });

  it("has every label in every locale", () => {
    for (const locale of locales) {
      setLocale(locale);
      for (const key of [
        "meta.title",
        "meta.edit",
        "meta.save",
        "meta.cancel",
        "meta.date",
        "meta.dateHint",
        "meta.gps",
        "meta.latitude",
        "meta.longitude",
        "meta.gpsHint",
        "meta.artist",
        "meta.copyright",
        "meta.failed",
      ]) {
        expect(t(key, { error: "x" }), `${locale}:${key}`).not.toBe(key);
      }
    }
    setLocale("en");
  });
});

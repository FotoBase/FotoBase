// TASK-06-005 drag-to-geotag payload tests: only Fotobase drags carry the
// custom MIME type, ids round-trip through the payload, and the geotag
// call reaches the backend with decimal degrees.

import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  PHOTO_DRAG_MIME,
  acceptsPhotoDrag,
  geotagPhotos,
  readPhotoDrop,
  setPhotoDrag,
} from "./geotag-drag";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

function fakeTransfer(): DataTransfer {
  const data = new Map<string, string>();
  return {
    setData: (type: string, value: string) => void data.set(type, value),
    getData: (type: string) => data.get(type) ?? "",
    get types() {
      return [...data.keys()];
    },
    effectAllowed: "uninitialized",
  } as unknown as DataTransfer;
}

const PHOTOS = [
  { id: 1, filename: "a.jpg", hash: "h1" },
  { id: 2, filename: "b.jpg", hash: "h2" },
];

describe("geotag-drag payload", () => {
  it("round-trips photo ids through the payload", () => {
    const transfer = fakeTransfer();
    setPhotoDrag(transfer, PHOTOS);
    expect(readPhotoDrop(transfer)).toEqual([1, 2]);
  });

  it("carries the ids as plain text for debugging", () => {
    const transfer = fakeTransfer();
    setPhotoDrag(transfer, PHOTOS);
    expect(transfer.getData("text/plain")).toBe("1,2");
  });

  it("rejects foreign drags", () => {
    const transfer = fakeTransfer();
    expect(acceptsPhotoDrag(transfer)).toBe(false);
    setPhotoDrag(transfer, PHOTOS);
    expect(acceptsPhotoDrag(transfer)).toBe(true);
  });

  it("returns null for malformed payloads", () => {
    const transfer = fakeTransfer();
    transfer.setData(PHOTO_DRAG_MIME, "not json");
    expect(readPhotoDrop(transfer)).toBeNull();
    const bad = fakeTransfer();
    bad.setData(PHOTO_DRAG_MIME, '["1"]');
    expect(readPhotoDrop(bad)).toBeNull();
  });
});

describe("geotagPhotos", () => {
  beforeEach(() => {
    (invoke as ReturnType<typeof vi.fn>).mockReset();
  });

  it("invokes the backend with the dropped ids and coordinates", async () => {
    (invoke as ReturnType<typeof vi.fn>).mockResolvedValue(undefined);
    await geotagPhotos([1, 2], 52.52, -13.405);
    expect(invoke).toHaveBeenCalledWith("geotag_photos", {
      fileIds: [1, 2],
      latitude: 52.52,
      longitude: -13.405,
    });
  });

  it("does nothing for an empty drop", async () => {
    await geotagPhotos([], 52.52, 13.405);
    expect(invoke).not.toHaveBeenCalled();
  });
});

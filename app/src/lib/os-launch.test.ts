import { describe, expect, it } from "vitest";
import { mergeUploadRequest, resolveUploadIds } from "./os-launch";

describe("resolveUploadIds (TASK-08-020)", () => {
  it("maps cataloged paths to ids and skips unknown, failing and repeated ones", async () => {
    const catalog: Record<string, number> = { "/p/a.jpg": 1, "/p/b.jpg": 2 };
    const find = async (path: string) => {
      if (path === "/p/boom.jpg") throw new Error("backend down");
      return path in catalog ? { id: catalog[path] } : null;
    };
    const ids = await resolveUploadIds(
      ["/p/a.jpg", "/p/unknown.jpg", "/p/boom.jpg", "/p/b.jpg", "/p/a.jpg"],
      find,
    );
    expect(ids).toEqual([1, 2]);
  });
});

describe("mergeUploadRequest (TASK-08-020)", () => {
  it("opens the dialog preloaded with the photo", () => {
    expect(mergeUploadRequest({ open: false, ids: [] }, [7])).toEqual({
      open: true,
      ids: [7],
    });
  });

  it("adds to an open dialog without duplicates", () => {
    expect(mergeUploadRequest({ open: true, ids: [7, 8] }, [8, 9])).toEqual({
      open: true,
      ids: [7, 8, 9],
    });
  });

  it("leaves the state alone when nothing resolved", () => {
    const current = { open: false, ids: [] };
    expect(mergeUploadRequest(current, [])).toBe(current);
  });
});

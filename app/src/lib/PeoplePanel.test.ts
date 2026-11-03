// TASK-05-005 tests: the unnamed people queue panel. Queue rendering with
// representative face boxes, bulk confirm (name required), bulk reject with
// its confirmation step, and the queue emptying after an action. The IPC
// wrapper module is mocked; the wrappers themselves are covered by the
// Rust integration tests.

import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import PeoplePanel from "./PeoplePanel.svelte";
import {
  canConfirm,
  confirmCluster,
  faceBox,
  faceThumbUri,
  peopleQueue,
  rejectCluster,
  type QueueCluster,
} from "./people";
import { setLocale } from "./i18n.svelte.js";

vi.mock("./people", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./people")>()),
  peopleQueue: vi.fn(),
  confirmCluster: vi.fn(),
  rejectCluster: vi.fn(),
}));

import { confirmCluster as confirmMock, peopleQueue as queueMock, rejectCluster as rejectMock } from "./people";

const A: QueueCluster = {
  cluster_id: 1,
  face_count: 7,
  face_id: 101,
  file_id: 11,
  x: 0.1,
  y: 0.2,
  width: 0.3,
  height: 0.25,
  hash: "a".repeat(16),
};
const B: QueueCluster = { ...A, cluster_id: 2, face_id: 202, file_id: 12, face_count: 3, hash: null };

beforeEach(() => {
  setLocale("en");
  vi.mocked(queueMock).mockReset().mockResolvedValue([A, B]);
  vi.mocked(confirmMock).mockReset().mockResolvedValue(A.face_count);
  vi.mocked(rejectMock).mockReset().mockResolvedValue(B.face_count);
});
afterEach(cleanup);

describe("queue rules", () => {
  it("requires a non-blank name to confirm", () => {
    expect(canConfirm("Ada")).toBe(true);
    expect(canConfirm("  Ada  ")).toBe(true);
    expect(canConfirm("")).toBe(false);
    expect(canConfirm("   ")).toBe(false);
  });

  it("builds the thumb URI and face box", () => {
    expect(faceThumbUri(A)).toBe(`thumb://${A.hash}?v=101`);
    expect(faceThumbUri(B)).toBe("");
    expect(faceBox(A)).toEqual({
      left: "10%",
      top: "20%",
      width: "30%",
      height: "25%",
    });
  });
});

describe("PeoplePanel", () => {
  it("lists the unnamed clusters with their face counts", async () => {
    render(PeoplePanel, { open: true, onchanged: () => {} });
    const items = await screen.findAllByTestId("people-cluster");
    expect(items).toHaveLength(2);
    expect(items[0].dataset.clusterId).toBe("1");
    expect(screen.getByText("7 faces")).toBeTruthy();
  });

  it("shows the empty state when the queue is empty", async () => {
    vi.mocked(queueMock).mockResolvedValue([]);
    render(PeoplePanel, { open: true, onchanged: () => {} });
    await waitFor(() => expect(screen.getByTestId("people-empty")).toBeTruthy());
  });

  it("bulk-confirms a cluster with the typed name", async () => {
    const onchanged = vi.fn();
    render(PeoplePanel, { open: true, onchanged });
    await waitFor(() => expect(screen.getAllByTestId("people-cluster")).toHaveLength(2));

    const confirmButtons = screen.getAllByTestId("people-confirm");
    expect(confirmButtons[0]).toHaveProperty("disabled", true);
    await fireEvent.input(screen.getAllByTestId("people-name")[0], { target: { value: "Ada" } });
    expect(confirmButtons[0]).toHaveProperty("disabled", false);
    await fireEvent.click(confirmButtons[0]);

    await waitFor(() => expect(confirmMock).toHaveBeenCalledWith(1, "Ada"));
    expect(screen.getByTestId("people-status").textContent).toContain("Ada");
    expect(onchanged).toHaveBeenCalled();
    expect(screen.getAllByTestId("people-cluster")).toHaveLength(1);
  });

  it("bulk-rejects a cluster after its confirmation step", async () => {
    render(PeoplePanel, { open: true, onchanged: () => {} });
    await waitFor(() => expect(screen.getAllByTestId("people-cluster")).toHaveLength(2));

    await fireEvent.click(screen.getAllByTestId("people-reject")[1]);
    await fireEvent.click(screen.getByTestId("people-reject-yes"));
    await waitFor(() => expect(rejectMock).toHaveBeenCalledWith(2));
    expect(screen.getAllByTestId("people-cluster")).toHaveLength(1);
  });

  it("cancelling the reject confirmation rejects nothing", async () => {
    render(PeoplePanel, { open: true, onchanged: () => {} });
    await waitFor(() => expect(screen.getAllByTestId("people-cluster")).toHaveLength(2));

    await fireEvent.click(screen.getAllByTestId("people-reject")[0]);
    await fireEvent.click(screen.getByTestId("people-reject-no"));
    expect(rejectMock).not.toHaveBeenCalled();
    expect(screen.getAllByTestId("people-cluster")).toHaveLength(2);
  });
});

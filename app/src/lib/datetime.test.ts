// TASK-03-019 tests: date/time adjustment model + IPC wrappers. The backend
// owns the arithmetic and the sidecar writes; this module turns dialog input
// into the adjustment the backend takes and formats what it returns.

import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  adjustDateTime,
  formatCaptureTime,
  formatOffset,
  mergeShiftSeconds,
  onDateTimeProgress,
  parseOffset,
  previewDateTime,
  shiftSeconds,
} from "./datetime";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

import * as tauriCore from "@tauri-apps/api/core";
import * as tauriEvent from "@tauri-apps/api/event";

const invokeMock = vi.mocked(tauriCore.invoke);
const listenMock = vi.mocked(tauriEvent.listen);

beforeEach(() => {
  invokeMock.mockReset();
  listenMock.mockReset();
});

describe("shiftSeconds", () => {
  it("adds days, hours, minutes and seconds", () => {
    expect(shiftSeconds({ direction: "later", days: 1, hours: 2, minutes: 3, seconds: 4 })).toBe(
      86_400 + 7200 + 180 + 4,
    );
  });

  it("is negative for 'earlier'", () => {
    expect(shiftSeconds({ direction: "earlier", days: 0, hours: 2, minutes: 0, seconds: 0 })).toBe(
      -7200,
    );
  });

  it("treats blank / negative / fractional fields as whole non-negative numbers", () => {
    expect(
      shiftSeconds({
        direction: "later",
        days: Number.NaN,
        hours: -3,
        minutes: 1.9,
        seconds: 0,
      }),
    ).toBe(60);
  });

  it("is 0 for an empty form, never -0", () => {
    expect(Object.is(shiftSeconds({ direction: "earlier", days: 0, hours: 0, minutes: 0, seconds: 0 }), 0)).toBe(
      true,
    );
  });
});

describe("offsets", () => {
  it("parses +HH:MM, -HHMM, Z and bare hours", () => {
    expect(parseOffset("+02:00")).toBe(120);
    expect(parseOffset("-05:30")).toBe(-330);
    expect(parseOffset("+0530")).toBe(330);
    expect(parseOffset("Z")).toBe(0);
    expect(parseOffset("utc")).toBe(0);
    expect(parseOffset(" +2 ")).toBe(120);
    expect(parseOffset("-12")).toBe(-720);
  });

  it("rejects what the backend would reject", () => {
    for (const bad of ["", "abc", "+15:00", "-12:30", "+02:60", "02:00", "+2:5"]) {
      expect(parseOffset(bad), bad).toBeNull();
    }
    expect(parseOffset("+14:00")).toBe(840);
  });

  it("formats minutes as ±HH:MM", () => {
    expect(formatOffset(120)).toBe("+02:00");
    expect(formatOffset(-330)).toBe("-05:30");
    expect(formatOffset(0)).toBe("+00:00");
  });
});

describe("mergeShiftSeconds", () => {
  it("is the right camera's reading minus this camera's, for one moment", () => {
    // Right camera 10:00:00, this one 08:59:30 → +1 h 0 min 30 s.
    expect(mergeShiftSeconds("2024-06-01T10:00:00", "2024-06-01T08:59:30")).toBe(3630);
  });

  it("is negative when this camera runs ahead and works across midnight", () => {
    expect(mergeShiftSeconds("2024-06-01T23:59:00", "2024-06-02T00:01:00")).toBe(-120);
  });

  it("accepts datetime-local values without seconds", () => {
    expect(mergeShiftSeconds("2024-06-01T10:05", "2024-06-01T10:00")).toBe(300);
  });

  it("is null for an incomplete or invalid entry", () => {
    expect(mergeShiftSeconds("", "2024-06-01T10:00:00")).toBeNull();
    expect(mergeShiftSeconds("2024-06-01T10:00:00", "nope")).toBeNull();
    expect(mergeShiftSeconds("2024-02-30T10:00:00", "2024-06-01T10:00:00")).toBeNull();
  });
});

describe("formatCaptureTime", () => {
  it("shows date, time and offset", () => {
    expect(formatCaptureTime("2024-06-01T12:00:00+02:00")).toBe("2024-06-01 12:00:00 +02:00");
    expect(formatCaptureTime("2024-06-01T12:00:00")).toBe("2024-06-01 12:00:00");
    expect(formatCaptureTime("2024-06-01T12:00:00.25-03:30")).toBe("2024-06-01 12:00:00.25 -03:30");
  });

  it("passes EXIF-style or unknown text through and shows a dash for none", () => {
    expect(formatCaptureTime("2024:06:01 12:00:00")).toBe("2024:06:01 12:00:00");
    expect(formatCaptureTime(null)).toBe("—");
  });
});

describe("ipc wrappers", () => {
  it("forward ids and the tagged adjustment", async () => {
    invokeMock.mockResolvedValue([]);
    await previewDateTime([1, 2], { kind: "shift", seconds: -7200 });
    expect(invokeMock).toHaveBeenLastCalledWith("preview_datetime_adjustment", {
      ids: [1, 2],
      adjustment: { kind: "shift", seconds: -7200 },
    });

    invokeMock.mockResolvedValue({ adjusted: 2, failed: [], cancelled: false });
    await adjustDateTime([1, 2], { kind: "convert_offset", offset_minutes: 120 });
    expect(invokeMock).toHaveBeenLastCalledWith("adjust_datetime", {
      ids: [1, 2],
      adjustment: { kind: "convert_offset", offset_minutes: 120 },
    });
  });

  it("subscribes to datetime-progress and hands back the unsubscribe", async () => {
    const unlisten = vi.fn();
    listenMock.mockResolvedValue(unlisten);
    const cb = vi.fn();
    const off = await onDateTimeProgress(cb);
    expect(listenMock).toHaveBeenCalledWith("datetime-progress", expect.any(Function));
    listenMock.mock.calls[0][1]({ payload: { done: 1, total: 3 } } as never);
    expect(cb).toHaveBeenCalledWith({ done: 1, total: 3 });
    expect(off).toBe(unlisten);
  });
});

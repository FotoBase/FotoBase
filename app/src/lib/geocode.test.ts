// TASK-06-008: reverse-geocoding IPC wrapper. IPC is mocked here — this
// module's job is the argument mapping and event subscription, the
// backend's is the GeoNames lookup and the catalog/FTS mirror.

import { beforeEach, describe, expect, it, vi } from "vitest";

import { invoke } from "@tauri-apps/api/core";
import { onGeocodeProgress, reverseGeocodeAll } from "./geocode";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

beforeEach(() => {
  vi.mocked(invoke).mockReset().mockResolvedValue(0);
});

describe("reverseGeocodeAll", () => {
  it("calls the backend command with no arguments", async () => {
    vi.mocked(invoke).mockResolvedValue(42);
    const geocoded = await reverseGeocodeAll();
    expect(invoke).toHaveBeenCalledWith("reverse_geocode_all", {});
    expect(geocoded).toBe(42);
  });
});

describe("onGeocodeProgress", () => {
  it("subscribes to the geocode-progress event", async () => {
    const listen = vi.fn().mockResolvedValue(() => {});
    vi.doMock("@tauri-apps/api/event", () => ({ listen }));
    vi.resetModules();
    const mod = await import("./geocode");
    const unlisten = await mod.onGeocodeProgress(() => {});
    expect(listen).toHaveBeenCalledWith(
      "geocode-progress",
      expect.any(Function),
    );
    expect(typeof unlisten).toBe("function");
  });
});

describe("event name contract", () => {
  it("matches the backend's geocode.rs GEOCODE_EVENT", async () => {
    // The backend constant is asserted in the Rust suite; here we pin the
    // frontend spelling so a rename on either side fails a test.
    const { GEOCODE_EVENT } = await import("./geocode");
    expect(GEOCODE_EVENT).toBe("geocode-progress");
  });
});

describe("unused import guard", () => {
  it("keeps the module surface minimal", () => {
    expect(onGeocodeProgress).toBeTypeOf("function");
    expect(reverseGeocodeAll).toBeTypeOf("function");
  });
});

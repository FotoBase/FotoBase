// SearchBar tests (TASK-01-018, PLAN §2C "Live search bar filters while
// typing"): typing feeds the shared grid store's `searchQuery` after a
// short debounce (tens of ms), clearing empties it immediately. The store
// is the single source of truth, so this component never talks to IPC.

import { flushSync } from "svelte";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import SearchBar from "./SearchBar.svelte";
import { gridState, resetGridState } from "./grid-state.svelte.js";

beforeEach(() => {
  vi.useFakeTimers();
  resetGridState();
});

afterEach(() => {
  vi.useRealTimers();
  cleanup();
});

const DEBOUNCE_MS = 80;

describe("SearchBar", () => {
  it("renders a labelled search input", () => {
    render(SearchBar);
    const input = screen.getByTestId("search-input") as HTMLInputElement;
    expect(input.placeholder).not.toBe("");
  });

  it("debounces typing into the shared store's searchQuery", async () => {
    render(SearchBar);
    const input = screen.getByTestId("search-input");

    fireEvent.input(input, { target: { value: "beach" } });
    // Tens of ms debounce: nothing is queried while the user is still typing.
    expect(gridState.searchQuery).toBe("");
    vi.advanceTimersByTime(DEBOUNCE_MS - 1);
    expect(gridState.searchQuery).toBe("");
    vi.advanceTimersByTime(1);
    expect(gridState.searchQuery).toBe("beach");
  });

  it("collapses a typing burst into the last value", async () => {
    render(SearchBar);
    const input = screen.getByTestId("search-input");

    fireEvent.input(input, { target: { value: "be" } });
    fireEvent.input(input, { target: { value: "beach" } });
    fireEvent.input(input, { target: { value: "beach sun" } });
    vi.advanceTimersByTime(DEBOUNCE_MS);
    expect(gridState.searchQuery).toBe("beach sun");
  });

  it("trims whitespace and stores '' for whitespace-only input", async () => {
    render(SearchBar);
    const input = screen.getByTestId("search-input");

    fireEvent.input(input, { target: { value: "  beach  " } });
    vi.advanceTimersByTime(DEBOUNCE_MS);
    expect(gridState.searchQuery).toBe("beach");

    fireEvent.input(input, { target: { value: "   " } });
    vi.advanceTimersByTime(DEBOUNCE_MS);
    expect(gridState.searchQuery).toBe("");
  });

  it("clear button empties input and store immediately (no pending timer)", async () => {
    render(SearchBar);
    const input = screen.getByTestId("search-input");

    fireEvent.input(input, { target: { value: "beach" } });
    vi.advanceTimersByTime(DEBOUNCE_MS);
    expect(gridState.searchQuery).toBe("beach");
    flushSync(); // the clear button renders only while there is text

    fireEvent.click(screen.getByTestId("search-clear"));
    flushSync();
    expect((input as HTMLInputElement).value).toBe("");
    expect(gridState.searchQuery).toBe("");

    // The debounce timer from any earlier keystroke must not resurrect the
    // old query after the clear.
    fireEvent.input(input, { target: { value: "beach" } });
    flushSync(); // the clear button reappears with the new text
    fireEvent.click(screen.getByTestId("search-clear"));
    vi.advanceTimersByTime(DEBOUNCE_MS);
    expect(gridState.searchQuery).toBe("");
  });
});

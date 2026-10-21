// Vitest setup: jsdom has no layout engine, so give the grid's measuring
// paths something deterministic to read.
//
// - TanStack Virtual reads the scroll element's `clientWidth`/`clientHeight`
//   (virtual-core `getRect`) and observes resize via `ResizeObserver`.
//   jsdom reports 0×0 and has no ResizeObserver, which would collapse the
//   viewport to zero height and mount nothing (or everything).
//
// Elements can override the fake dimensions per test by setting the
// `__clientWidth` / `__clientHeight` properties on the element itself.

type ResizeCallback = () => void;

const observers: { cb: ResizeCallback; observed: Set<Element> }[] = [];

class FakeResizeObserver {
  private readonly cb: ResizeCallback;
  constructor(cb: ResizeCallback) {
    this.cb = cb;
    observers.push({ cb, observed: new Set() });
  }
  observe(el: Element): void {
    for (const o of observers) if (o.cb === this.cb) o.observed.add(el);
    // Real browsers fire an initial observation on observe().
    this.cb();
  }
  unobserve(el: Element): void {
    for (const o of observers) o.observed.delete(el);
  }
  disconnect(): void {
    for (const o of observers) if (o.cb === this.cb) o.observed.clear();
  }
}

// Fire every live ResizeObserver callback (what a real window resize does).
export function triggerResize(): void {
  for (const o of observers) for (const _el of o.observed) o.cb();
}

window.ResizeObserver = FakeResizeObserver as unknown as typeof ResizeObserver;

for (const prop of ["clientWidth", "clientHeight"] as const) {
  const fallback = prop === "clientWidth" ? 800 : 600;
  Object.defineProperty(Element.prototype, prop, {
    configurable: true,
    get(this: Element & Record<string, number | undefined>) {
      return this[`__${prop}`] ?? fallback;
    },
  });
}

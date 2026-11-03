// Click dispatch for overlay controls (TASK-05-013). Under xvfb + software GL,
// WebKitWebDriver's native click on the edit view's controls intermittently
// reports "element not interactable" after 25–55 s per attempt, or hangs until
// connectionRetryTimeout — and a retry loop around it outlives the mocha
// timeout, keeps clicking into the next tests, and masks the real error.
// So: ONE DOM dispatch through `browser.execute` (the element's own click
// handler — what these specs exercise), then wait for the effect. Never a
// second click: the edit controls are toggles and undo pops history, so a
// "fallback" click reverses the first one.

import { $, browser } from "@wdio/globals";

/// Reject with `what` when `p` does not settle within `ms`: a wedged webview
/// otherwise blocks a WebDriver command for minutes with no useful message.
export async function withDeadline<T>(p: Promise<T>, ms: number, what: string): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const deadline = new Promise<never>((_, reject) => {
    timer = setTimeout(
      () => reject(new Error(`${what}: no answer from the webview within ${ms}ms`)),
      ms,
    );
  });
  try {
    return await Promise.race([p, deadline]);
  } finally {
    clearTimeout(timer);
  }
}

/// Fire the element's click handler once. Throws naming the phase and why.
export async function dispatchClick(selector: string): Promise<void> {
  const why = await withDeadline(
    browser.execute((sel: string) => {
      const el = document.querySelector(sel) as HTMLElement | null;
      if (!el) return "not found";
      if ((el as HTMLButtonElement).disabled) return "disabled";
      el.click();
      return null;
    }, selector),
    30_000,
    `dispatch ${selector}`,
  );
  if (why !== null) throw new Error(`dispatch ${selector}: ${why}`);
}

/// Click `data-testid=testid` once and wait until `verify` holds.
export async function activate(
  testid: string,
  verify: () => Promise<boolean> | boolean,
  timeout = 15_000,
): Promise<void> {
  const sel = `[data-testid="${testid}"]`;
  await $(sel).waitForExist({ timeoutMsg: `find ${sel}: never rendered` });
  await dispatchClick(sel);
  await browser.waitUntil(verify, {
    timeout,
    timeoutMsg: `verify ${sel}: clicked once, effect not observed within ${timeout}ms`,
  });
}

/// Page vitals for wedge hunting (TASK-05-013): counts of resource loads per
/// URI scheme since `installVitals()`, read without touching the app.
export async function installVitals(): Promise<void> {
  await browser.execute(() => {
    const w = window as unknown as { __fbVitals?: Record<string, number> };
    if (w.__fbVitals) return;
    const counts: Record<string, number> = {};
    w.__fbVitals = counts;
    new PerformanceObserver((list) => {
      for (const e of list.getEntries()) {
        const scheme = e.name.split(":")[0];
        counts[scheme] = (counts[scheme] ?? 0) + 1;
      }
    }).observe({ type: "resource", buffered: true });
  });
}

/// One log line: elapsed ms, how long the page took to answer, and the
/// per-scheme load counts.
export async function logVitals(step: string, t0: number): Promise<void> {
  const asked = Date.now();
  const counts = await withDeadline(
    browser.execute(
      () => JSON.stringify((window as unknown as { __fbVitals?: unknown }).__fbVitals ?? {}),
    ),
    20_000,
    `vitals ${step}`,
  ).catch((e: Error) => `unanswered (${e.message})`);
  // eslint-disable-next-line no-console
  console.log(
    `[e2e] vitals +${asked - t0}ms ${step}: answered in ${Date.now() - asked}ms loads=${counts}`,
  );
}

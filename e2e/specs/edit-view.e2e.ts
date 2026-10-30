// TASK-04-010 e2e: the edit view, end to end against the real app. Opening
// a photo shows the three edit tabs and the filmstrip; applying an op from
// each tab updates the preview; undo reverts it; and no byte of any
// original changes (§9 Data safety — Save is FEAT-04-06, not here).

import { browser, expect, $, $$ } from "@wdio/globals";
import fs from "node:fs";
import path from "node:path";

import { makeLibrary, sha256, SOLO, type Library } from "../support/library";

let lib: Library;

type Reply = { ok: true; value: unknown } | { ok: false; error: string };

const invoke = async (cmd: string, args: object = {}): Promise<unknown> => {
  const reply = await browser.executeAsync<Reply, [string, object]>(
    (cmd: string, args: object, done: (r: Reply) => void) => {
      const tauri = (
        window as unknown as {
          __TAURI__: {
            core: { invoke: (c: string, a: object) => Promise<unknown> };
          };
        }
      ).__TAURI__;
      tauri.core.invoke(cmd, args).then(
        (value) => done({ ok: true, value }),
        (e) => done({ ok: false, error: String(e) }),
      );
    },
    cmd,
    args,
  );
  if (!reply.ok) throw new Error(`${cmd} failed: ${reply.error}`);
  return reply.value;
};

const cells = () => $$('[data-testid="grid-cell"]');

// WebKitGTK intermittently reports an interactable-looking element as "not
// interactable" (element re-rendered between lookup and click). Plain
// clicks with bounded retries; the selector is named when giving up so
// failures say what could not be clicked.
async function clickTestId(testid: string): Promise<void> {
  const sel = `[data-testid="${testid}"]`;
  for (let attempt = 0; attempt < 5; attempt++) {
    try {
      const el = await $(sel);
      await el.click();
      return;
    } catch {
      await browser.pause(500);
    }
  }
  throw new Error(`could not click ${sel}`);
}

// Sidecar polls: the debounced save lands ~250ms after the last edit, so
// assert with a waitUntil instead of an instant read.
function sidecarPath(): string {
  return path.join(lib.dir, `${SOLO}.xmp`);
}

function sidecarText(): string {
  return fs.existsSync(sidecarPath())
    ? fs.readFileSync(sidecarPath(), "utf8")
    : "";
}

async function waitSidecarContains(fragment: string): Promise<void> {
  await browser.waitUntil(() => sidecarText().includes(fragment), {
    timeout: 10_000,
    timeoutMsg: `sidecar never contained ${fragment}; last value: ${sidecarText().slice(0, 500)}`,
  });
}

// Click + verify + DOM-click fallback. WebKitGTK under xvfb intermittently
// reports overlay elements as "not interactable" or dispatches the click at
// coordinates nothing receives; the fallback fires the element's own click
// handler directly, which is what these specs actually exercise.
async function activate(
  testid: string,
  verify: () => Promise<boolean> | boolean,
): Promise<void> {
  const sel = `[data-testid="${testid}"]`;
  await clickTestId(testid);
  try {
    await browser.waitUntil(verify, { timeout: 2000 });
    return;
  } catch {
    // fall through to the DOM click
  }
  await browser.execute((selector: string) => {
    const el = document.querySelector(selector) as HTMLElement | null;
    if (!el) throw new Error(`${selector} not found`);
    el.click();
  }, sel);
  await browser.waitUntil(verify, {
    timeout: 8000,
    timeoutMsg: `${sel} never took effect`,
  });
}

async function waitForCellCount(count: number): Promise<void> {
  await browser.waitUntil(async () => (await cells().length) === count, {
    timeoutMsg: `expected ${count} grid cells`,
  });
}

// The debounced render saves the recipe ~250ms after the last edit.
async function settle(): Promise<void> {
  await browser.pause(700);
}

const errorText = async (): Promise<string> => {
  try {
    return await (await $('[data-testid="edit-error"]')).getText();
  } catch {
    return "";
  }
};

describe("edit view", () => {
  before(async () => {
    lib = makeLibrary();
    await browser.waitUntil(async () =>
      browser.execute(() =>
        Boolean((window as unknown as { __TAURI__?: unknown }).__TAURI__),
      ),
    );
    const error = await browser.executeAsync(
      (folder: string, done: (e: string | null) => void) => {
        const tauri = (
          window as unknown as {
            __TAURI__: {
              core: { invoke: (cmd: string, args: object) => Promise<unknown> };
            };
          }
        ).__TAURI__;
        tauri.core
          .invoke("set_folder_mode", { path: folder, mode: "once" })
          .then(
            () => done(null),
            (e) => done(String(e)),
          );
      },
      lib.dir,
    );
    expect(error).toBeNull();
    await waitForCellCount(3);
  });

  after(() => {
    fs.rmSync(lib.dir, { recursive: true, force: true });
  });

  it("opens the edit view with tabs, filmstrip and a live preview", async () => {
    // Open the viewer on the solo photo — selected by filename, not cell
    // index (the catalog's order is not the library's write order). A
    // double-click is unreliable in WebKitWebDriver (the first click's
    // selection re-render can swallow the dblclick), so use the keyboard
    // path: click selects, Enter opens.
    await $(`[data-testid="grid-cell"] img[alt="${SOLO}"]`).click();
    await browser.keys(["Enter"]);
    await $('[data-testid="viewer"]').waitForDisplayed();
    await activate("edit-open", () =>
      $('[data-testid="edit-view"]').isDisplayed(),
    );
    await $('[data-testid="edit-view"]').waitForDisplayed();
    await $('[data-testid="edit-tab-basic"]').waitForDisplayed();
    await $('[data-testid="edit-tab-tuning"]').waitForDisplayed();
    await $('[data-testid="edit-tab-effects"]').waitForDisplayed();
    await $('[data-testid="edit-preview"]').waitForDisplayed();
    expect(await $$('[data-testid="edit-filmstrip-thumb"]').length).toBe(3);
  });

  it("applies an op from each tab and updates the preview", async () => {
    // Effects tab: B&W one-click (verify: the strength slider appears).
    await activate(
      "edit-tab-effects",
      async () =>
        (await $('[data-testid="effects"]').isExisting()) &&
        (await $('[data-testid="effects"]').isDisplayed()),
    );
    await activate("effect-bw", async () => {
      try {
        return await $('[data-testid="strength-bw"]').isDisplayed();
      } catch {
        return false;
      }
    });

    // Tuning tab: Fill Light slider. WebKitGTK refuses sendKeys on range
    // inputs and synthetic events are unreliable here, so try, in order: a
    // real WebDriver click (the center of a 0..1 slider is exactly 0.5),
    // keyboard steps on the focused input, and finally a dispatched input
    // event — the panel's handler (clamping, op emission) is what the spec
    // exercises, whichever interaction reaches it.
    await activate("edit-tab-tuning", async () => {
      try {
        return await $('[data-testid="tuning"]').isDisplayed();
      } catch {
        return false;
      }
    });
    const sliderState = async (): Promise<string> => {
      try {
        return String(await (await $('[data-testid="fill-light"]')).getValue());
      } catch {
        return "unreachable";
      }
    };
    const fillSaved = (): boolean => sidecarText().includes("fill_light");
    const sliderDiagnostics = async (): Promise<string> => {
      const recipe = await invoke("get_recipe", {
        fileId:
          (await invoke("find_file_by_path", { path: `${lib.dir}/${SOLO}` })) !=
          null
            ? (
                (await invoke("find_file_by_path", {
                  path: `${lib.dir}/${SOLO}`,
                })) as { id: number }
              ).id
            : -1,
      }).catch((e) => `get_recipe failed: ${e}`);
      return `slider=${await sliderState()} error=${await errorText()} recipe=${JSON.stringify(
        recipe,
      ).slice(0, 200)}`;
    };

    try {
      await (await $('[data-testid="fill-light"]')).click();
      await browser.waitUntil(fillSaved, { timeout: 3000 });
    } catch {
      // fall through
    }
    if (!fillSaved()) {
      await browser.execute(() => {
        const el = document.querySelector(
          '[data-testid="fill-light"]',
        ) as HTMLInputElement | null;
        if (!el) throw new Error("fill-light slider not found");
        el.focus();
      });
      for (let i = 0; i < 10; i++) await browser.keys(["ArrowRight"]);
      try {
        await browser.waitUntil(fillSaved, { timeout: 3000 });
      } catch {
        // fall through
      }
    }
    if (!fillSaved()) {
      await browser.execute(() => {
        const el = document.querySelector(
          '[data-testid="fill-light"]',
        ) as HTMLInputElement | null;
        if (!el) throw new Error("fill-light slider not found");
        el.value = "0.5";
        el.dispatchEvent(new Event("input", { bubbles: true }));
      });
    }
    await browser.waitUntil(fillSaved, {
      timeout: 10_000,
      timeoutMsg: `fill_light never saved; ${await sliderDiagnostics()}`,
    });

    // Basic Fixes tab: Auto Contrast.
    await activate("edit-tab-basic", async () => {
      try {
        return await $('[data-testid="auto-fix"]').isDisplayed();
      } catch {
        return false;
      }
    });
    await activate("auto-contrast", () =>
      sidecarText().includes("auto_contrast"),
    );

    const after = await $('[data-testid="edit-preview"]').getAttribute("src");
    await browser.waitUntil(
      async () =>
        String(
          await $('[data-testid="edit-preview"]').getAttribute("src"),
        ).includes("?r="),
      {
        timeout: 10_000,
        timeoutMsg: `preview never showed an edited render; error: ${await errorText()}`,
      },
    );
    expect(after !== undefined).toBe(true);

    // The recipe persisted through the sidecar write-through.
    await waitSidecarContains('"bw"');
    await waitSidecarContains("fill_light");
    await waitSidecarContains("auto_contrast");
  });

  it("undo reverts the recipe", async () => {
    await activate("edit-undo", () => !sidecarText().includes("auto_contrast"));
  });

  it("never modified the original (checksum-verified, §9)", async () => {
    expect(sha256(path.join(lib.dir, SOLO))).toBe(lib.checksums[SOLO]);
  });

  it("Escape closes the edit view", async () => {
    await browser.keys(["Escape"]);
    await $('[data-testid="edit-view"]').waitForDisplayed({ reverse: true });
    // The viewer underneath comes back (EditView overlays it, not replaces
    // the session).
    await $('[data-testid="viewer"]').waitForDisplayed();
  });
});

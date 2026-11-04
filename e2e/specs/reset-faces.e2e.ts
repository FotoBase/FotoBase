// TASK-05-009 e2e: "Reset faces" from the picture context menu, end to end
// against the real app. One fixture photo carries a sidecar with a named
// MWG face region (no AI models needed — the sync ingests sidecar-named
// faces directly); the context menu clears it: sidecar regions, catalog
// face rows, person album.

import fs from "node:fs";
import path from "node:path";
import { browser, expect } from "@wdio/globals";

import { withDeadline } from "../support/interact";
import { makeLibrary, SOLO, type Library } from "../support/library";

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
  if (!reply.ok) throw new Error(reply.error);
  return reply.value;
};

// A minimal sidecar with one named face region (the shape the MWG writer
// produces, TASK-05-008). The scanner syncs it into `faces` + a person row.
const SIDECAR = `<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:mwg-rs="http://www.metadataworkinggroup.com/schemas/regions/"
    xmlns:stArea="http://ns.adobe.com/xmp/sType/Area#">
   <mwg-rs:Regions rdf:parseType="Resource">
    <mwg-rs:RegionList>
     <rdf:Bag>
      <rdf:li>
       <rdf:Description mwg-rs:Name="Anna" mwg-rs:Type="Face">
        <mwg-rs:Area stArea:x="0.5" stArea:y="0.5" stArea:w="0.2" stArea:h="0.2" stArea:unit="normalized"/>
       </rdf:Description>
      </rdf:li>
     </rdf:Bag>
    </mwg-rs:RegionList>
   </mwg-rs:Regions>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
`;

interface PersonEntry {
  person_id: number | null;
  cluster_id: number | null;
  name: string | null;
  face_count: number;
}

const peopleCount = async (): Promise<number> =>
  ((await invoke("people_manager_list")) as PersonEntry[]).length;

// The e2e driver cannot answer the native confirm() dialog; it is part of
// the flow under test, so stub it instead of bypassing the gate in app code.
// The context menu opens on the cell's `contextmenu` event; under xvfb +
// WebKitWebDriver a native right-click hangs, so the whole flow — find the
// solo cell (waiting for its thumbnail to render), dispatch the event, wait
// for the menu item, click it — runs as ONE page task. Splitting it into
// separate WebDriver calls let the menu close between them (native
// context-menu grab) and raced the lazy thumbnail.
const openMenuOnSolo = async (answer: boolean): Promise<void> => {
  const why = await withDeadline(
    browser.executeAsync(
      (
        alt: string,
        answer: boolean,
        done: (r: "ok" | "no-cell" | "no-menu") => void,
      ): void => {
        // Stub confirm here, right before the click: a native confirm()
        // dialog would block the flow forever, and a stub set in an earlier
        // WebDriver call is lost when the webview reloads between tests.
        (window as unknown as { confirm: (m?: string) => boolean }).confirm =
          () => answer;
        let tries = 0;
        const attempt = (): void => {
          const img = document.querySelector(`img[alt="${alt}"]`);
          const cell =
            img?.closest('[data-testid="grid-cell"]') ??
            document.querySelector('[data-testid="grid-cell"]');
          if (!cell) {
            if (++tries > 100) return done("no-cell");
            setTimeout(attempt, 100);
            return;
          }
          cell.dispatchEvent(
            new MouseEvent("contextmenu", {
              bubbles: true,
              cancelable: true,
              clientX: 40,
              clientY: 40,
            }),
          );
          // The item renders synchronously once the menu is open; poll
          // briefly in case a refresh re-renders the grid first.
          let waits = 0;
          const wait = (): void => {
            const item = document.querySelector(
              '[data-testid="menu-item-reset-faces"]',
            ) as HTMLElement | null;
            if (item) {
              item.click();
              return done("ok");
            }
            if (++waits > 50) return done("no-menu");
            setTimeout(wait, 100);
            return;
          };
          wait();
        };
        attempt();
      },
      SOLO,
      answer,
    ),
    30_000,
    "open menu on solo",
  );
  if (why !== "ok") throw new Error(`open menu on solo: ${why}`);
};

describe("reset faces", () => {
  before(async () => {
    lib = makeLibrary();
    fs.writeFileSync(path.join(lib.dir, `${SOLO}.xmp`), SIDECAR);
    await browser.waitUntil(async () =>
      browser.execute(() =>
        Boolean((window as unknown as { __TAURI__?: unknown }).__TAURI__),
      ),
    );
    await invoke("set_folder_mode", { path: lib.dir, mode: "once" });
    await browser.waitUntil(async () => (await peopleCount()) === 1);
  });

  after(() => {
    fs.rmSync(lib.dir, { recursive: true, force: true });
  });

  it("declining the confirmation leaves everything in place", async () => {
    await openMenuOnSolo(false);
    expect(await peopleCount()).toBe(1);
    const raw = fs.readFileSync(path.join(lib.dir, `${SOLO}.xmp`), "utf8");
    expect(raw).toContain('mwg-rs:Name="Anna"');
  });

  it("confirming clears the person, the face row and the sidecar region", async () => {
    await openMenuOnSolo(true);
    // The person row itself survives (a person may span several photos);
    // what resets is Anna's face count, the sidecar region and the filter.
    await browser.waitUntil(async () => {
      const entries = (await invoke("people_manager_list")) as PersonEntry[];
      return entries.every((e) => e.name !== "Anna" || e.face_count === 0);
    });
    const raw = fs.readFileSync(path.join(lib.dir, `${SOLO}.xmp`), "utf8");
    expect(raw).not.toContain('mwg-rs:Name="Anna"');
    // The photo also leaves the "has faces" filter.
    const hits = (await invoke("query_files", {
      has_faces: true,
    })) as unknown[];
    expect(hits.length).toBe(0);
  });
});

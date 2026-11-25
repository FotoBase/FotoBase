// Static web-gallery viewer (TASK-10-01-01): the generated site's vanilla-JS
// viewer navigates photos from the embedded JSON alone — no backend, no
// network. The script under test is the one the Rust generator ships.

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

const VIEWER = readFileSync(
  resolve(__dirname, "../../../crates/export/src/gallery/templates/shared/viewer.js"),
  "utf8",
);

const PHOTOS = [
  { title: "A", caption: "first", image: { src: "images/a.jpg" } },
  { title: "B", caption: "second", image: { src: "images/b.jpg" } },
  { title: "C", caption: "", image: { src: "images/c.jpg" } },
];

function mountSite(): void {
  document.body.innerHTML =
    PHOTOS.map(
      (p, i) => `<a class="fb-item" href="${p.image.src}" data-fb-index="${i}"><img alt=""></a>`,
    ).join("") +
    `<script type="application/json" id="fb-data">${JSON.stringify({ photos: PHOTOS })}</script>`;
  // Indirect eval runs the script as a page script, like <script src>.
  (0, eval)(VIEWER);
}

const box = () => document.querySelector(".fb-lightbox") as HTMLElement;
const shown = () => box().querySelector("img")?.getAttribute("src");
const key = (k: string) => document.dispatchEvent(new KeyboardEvent("keydown", { key: k }));

describe("gallery viewer", () => {
  beforeEach(mountSite);
  afterEach(() => {
    document.body.innerHTML = "";
  });

  it("opens the clicked photo with its title and caption", () => {
    (document.querySelectorAll(".fb-item")[1] as HTMLElement).click();
    expect(box().classList.contains("fb-open")).toBe(true);
    expect(shown()).toBe("images/b.jpg");
    expect(box().querySelector(".fb-lb-title")?.textContent).toBe("B");
    expect(box().querySelector(".fb-lb-caption")?.textContent).toBe("second");
  });

  it("navigates with the arrow keys and buttons, wrapping at both ends", () => {
    (document.querySelectorAll(".fb-item")[0] as HTMLElement).click();
    key("ArrowLeft");
    expect(shown()).toBe("images/c.jpg");
    key("ArrowRight");
    expect(shown()).toBe("images/a.jpg");
    (box().querySelector(".fb-lb-next") as HTMLElement).click();
    expect(shown()).toBe("images/b.jpg");
    (box().querySelector(".fb-lb-prev") as HTMLElement).click();
    expect(shown()).toBe("images/a.jpg");
  });

  it("closes on Escape and ignores keys while closed", () => {
    key("ArrowRight");
    expect(box().classList.contains("fb-open")).toBe(false);
    (document.querySelectorAll(".fb-item")[2] as HTMLElement).click();
    key("Escape");
    expect(box().classList.contains("fb-open")).toBe(false);
    expect(box().querySelector("img")?.hasAttribute("src")).toBe(false);
  });
});

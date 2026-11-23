// Fotobase gallery viewer: lightbox with keyboard, touch and slideshow
// support. Reads the embedded #fb-data JSON; makes no network requests.
(function () {
  "use strict";
  var dataEl = document.getElementById("fb-data");
  if (!dataEl) return;
  var photos = JSON.parse(dataEl.textContent).photos || [];
  var SLIDE_MS = 4000;
  var current = -1;
  var timer = null;
  var touchX = null;

  function el(tag, cls, text) {
    var node = document.createElement(tag);
    if (cls) node.className = cls;
    if (text) node.textContent = text;
    return node;
  }

  var box = el("div", "fb-lightbox");
  box.setAttribute("role", "dialog");
  box.setAttribute("aria-modal", "true");
  var img = el("img");
  var text = el("div", "fb-lb-text");
  var title = el("div", "fb-lb-title");
  var caption = el("div", "fb-lb-caption");
  var close = el("button", "fb-lb-close", "\u00d7");
  var prev = el("button", "fb-lb-prev", "\u2039");
  var next = el("button", "fb-lb-next", "\u203a");
  var play = el("button", "fb-lb-play", "\u25b6");
  close.setAttribute("aria-label", "Close");
  prev.setAttribute("aria-label", "Previous photo");
  next.setAttribute("aria-label", "Next photo");
  play.setAttribute("aria-label", "Start slideshow");
  text.appendChild(title);
  text.appendChild(caption);
  [img, text, close, prev, next, play].forEach(function (n) {
    box.appendChild(n);
  });
  document.body.appendChild(box);

  function show(index) {
    if (!photos.length) return;
    current = (index + photos.length) % photos.length;
    var p = photos[current];
    img.src = p.image.src;
    img.alt = p.title || p.caption || "";
    title.textContent = p.title || "";
    caption.textContent = p.caption || "";
    box.classList.add("fb-open");
    var ahead = photos[(current + 1) % photos.length];
    new Image().src = ahead.image.src;
  }

  function stop() {
    if (timer !== null) clearInterval(timer);
    timer = null;
    play.textContent = "\u25b6";
    play.setAttribute("aria-label", "Start slideshow");
  }

  function hide() {
    stop();
    box.classList.remove("fb-open");
    img.removeAttribute("src");
    current = -1;
  }

  function step(delta) {
    show(current + delta);
  }

  play.addEventListener("click", function () {
    if (timer !== null) return stop();
    play.textContent = "\u23f8";
    play.setAttribute("aria-label", "Stop slideshow");
    timer = setInterval(function () {
      step(1);
    }, SLIDE_MS);
  });
  close.addEventListener("click", hide);
  prev.addEventListener("click", function () {
    step(-1);
  });
  next.addEventListener("click", function () {
    step(1);
  });
  box.addEventListener("click", function (e) {
    if (e.target === box) hide();
  });
  box.addEventListener("touchstart", function (e) {
    touchX = e.changedTouches[0].clientX;
  });
  box.addEventListener("touchend", function (e) {
    if (touchX === null) return;
    var dx = e.changedTouches[0].clientX - touchX;
    touchX = null;
    if (Math.abs(dx) > 40) step(dx < 0 ? 1 : -1);
  });
  document.addEventListener("keydown", function (e) {
    if (current < 0) return;
    if (e.key === "Escape") hide();
    else if (e.key === "ArrowRight") step(1);
    else if (e.key === "ArrowLeft") step(-1);
  });

  document.addEventListener("click", function (e) {
    var item = e.target.closest ? e.target.closest("[data-fb-index]") : null;
    if (!item) return;
    e.preventDefault();
    show(parseInt(item.getAttribute("data-fb-index"), 10));
  });
})();

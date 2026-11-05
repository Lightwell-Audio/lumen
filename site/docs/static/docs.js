// The Lumen documentation's behaviour (docs/USER_DOCS.md §6.3, §7, §9).
// No framework and no tracking: the theme and platform the reader picks
// are kept in this browser only.
(function () {
  "use strict";
  var root = document.documentElement;
  var reduced = window.matchMedia("(prefers-reduced-motion: reduce)");
  var systemLight = window.matchMedia("(prefers-color-scheme: light)");

  function remember(key, value) {
    try { localStorage.setItem(key, value); } catch (e) { /* private mode */ }
  }

  function theme() {
    return root.dataset.theme || (systemLight.matches ? "light" : "dark");
  }

  // ---- Theme: the switch, and every picture following it ----
  function swapMedia() {
    var t = theme();
    document.querySelectorAll("video[data-dark]").forEach(function (v) {
      var src = v.dataset[t];
      var poster = v.dataset[t + "Poster"];
      if (poster && v.getAttribute("poster") !== poster) v.setAttribute("poster", poster);
      if (v.dataset.loaded && v.getAttribute("src") !== src) {
        var playing = !v.paused;
        v.setAttribute("src", src);
        if (playing) v.play().catch(function () {});
      }
    });
    document.querySelectorAll(".shot img[data-dark]").forEach(function (img) {
      img.src = img.dataset[t];
      var source = img.parentElement.querySelector("source[data-theme-src]");
      if (source) source.remove();
    });
  }

  document.querySelectorAll("[data-theme-toggle]").forEach(function (button) {
    button.addEventListener("click", function () {
      root.dataset.theme = theme() === "dark" ? "light" : "dark";
      remember("lumen-docs-theme", root.dataset.theme);
      swapMedia();
    });
  });
  systemLight.addEventListener("change", swapMedia);

  // ---- Platform: Windows or macOS keycaps ----
  document.querySelectorAll("[data-platform-set]").forEach(function (button) {
    button.setAttribute("aria-pressed", String(root.dataset.platform === button.dataset.platformSet));
    button.addEventListener("click", function () {
      root.dataset.platform = button.dataset.platformSet;
      remember("lumen-docs-platform", root.dataset.platform);
      document.querySelectorAll("[data-platform-set]").forEach(function (b) {
        b.setAttribute("aria-pressed", String(b === button || b.dataset.platformSet === button.dataset.platformSet));
      });
    });
  });

  // ---- Clips: play while in view, pause button, reduced motion ----
  function load(video) {
    if (video.dataset.loaded) return;
    video.dataset.loaded = "1";
    var src = video.dataset[theme()] || video.getAttribute("src");
    if (src) video.setAttribute("src", src);
  }

  var clips = document.querySelectorAll("[data-clip]");
  var watcher = "IntersectionObserver" in window ? new IntersectionObserver(function (entries) {
    entries.forEach(function (entry) {
      var figure = entry.target;
      var video = figure.querySelector("video");
      if (entry.isIntersecting && !figure.classList.contains("is-paused") && !reduced.matches) {
        load(video);
        video.play().catch(function () {});
      } else {
        video.pause();
      }
    });
  }, { threshold: 0.4 }) : null;

  clips.forEach(function (figure) {
    var video = figure.querySelector("video");
    var toggle = figure.querySelector("[data-clip-toggle]");
    if (reduced.matches) figure.classList.add("is-paused");
    toggle.setAttribute("aria-label", figure.classList.contains("is-paused") ? "Play" : "Pause");
    toggle.addEventListener("click", function () {
      var pausing = !figure.classList.contains("is-paused");
      figure.classList.toggle("is-paused", pausing);
      toggle.setAttribute("aria-label", pausing ? "Play" : "Pause");
      if (pausing) {
        video.pause();
      } else {
        load(video);
        video.play().catch(function () {});
      }
    });
    if (watcher) watcher.observe(figure);
  });

  // Cards on the home page play on hover and focus.
  document.querySelectorAll("[data-hover-play]").forEach(function (media) {
    var video = media.querySelector("video");
    var card = media.closest(".goal-link") || media;
    function start() {
      if (reduced.matches) return;
      load(video);
      video.play().catch(function () {});
    }
    function stop() { video.pause(); }
    card.addEventListener("mouseenter", start);
    card.addEventListener("focus", start);
    card.addEventListener("mouseleave", stop);
    card.addEventListener("blur", stop);
  });

  // "Show me" loads its clip when opened.
  document.querySelectorAll(".tryit-answer").forEach(function (details) {
    details.addEventListener("toggle", function () {
      var video = details.querySelector("video");
      if (!video) return;
      if (details.open && !reduced.matches) {
        load(video);
        video.play().catch(function () {});
      } else {
        video.pause();
      }
    });
  });

  swapMedia();

  // ---- Contents on phones ----
  var side = document.getElementById("side");
  document.querySelectorAll("[data-nav-toggle]").forEach(function (button) {
    button.addEventListener("click", function () {
      var open = !side.classList.contains("is-open");
      side.classList.toggle("is-open", open);
      button.setAttribute("aria-expanded", String(open));
    });
  });

  // ---- "On this page" follows the reading position ----
  var tocLinks = document.querySelectorAll(".toc a");
  if (tocLinks.length && "IntersectionObserver" in window) {
    var byId = {};
    tocLinks.forEach(function (a) { byId[decodeURIComponent(a.hash.slice(1))] = a; });
    var headings = Object.keys(byId).map(function (id) { return document.getElementById(id); }).filter(Boolean);
    var spy = new IntersectionObserver(function (entries) {
      entries.forEach(function (entry) {
        if (entry.isIntersecting) {
          tocLinks.forEach(function (a) { a.classList.remove("is-here"); });
          byId[entry.target.id].classList.add("is-here");
        }
      });
    }, { rootMargin: "0px 0px -70% 0px" });
    headings.forEach(function (h) { spy.observe(h); });
  }

  // ---- Search: Ctrl/Cmd+K or "/" ----
  var layer = document.querySelector("[data-search-layer]");
  var searchReady = false;
  var lastFocus = null;
  function openSearch() {
    if (!layer) return;
    lastFocus = document.activeElement;
    if (!searchReady && window.PagefindUI) {
      new window.PagefindUI({
        element: "#search",
        showSubResults: true,
        showImages: false,
        resetStyles: false,
        translations: {
          placeholder: "Search the docs: split, punch-in, export stems…",
          zero_results: "Nothing for [SEARCH_TERM] yet. Try another word, or browse the guides."
        }
      });
      searchReady = true;
    }
    layer.hidden = false;
    setTimeout(function () {
      var input = layer.querySelector("input");
      if (input) input.focus();
    }, 30);
  }
  function closeSearch() {
    if (!layer || layer.hidden) return;
    layer.hidden = true;
    if (lastFocus) lastFocus.focus();
  }
  document.querySelectorAll("[data-search-open]").forEach(function (button) {
    button.addEventListener("click", openSearch);
  });
  if (layer) {
    layer.addEventListener("click", function (event) {
      if (event.target === layer) closeSearch();
    });
  }
  document.addEventListener("keydown", function (event) {
    var typing = /INPUT|TEXTAREA|SELECT/.test(document.activeElement.tagName);
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
      event.preventDefault();
      openSearch();
    } else if (event.key === "/" && !typing) {
      event.preventDefault();
      openSearch();
    } else if (event.key === "Escape") {
      closeSearch();
      if (side && side.classList.contains("is-open")) side.classList.remove("is-open");
    }
  });
})();

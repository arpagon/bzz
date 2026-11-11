// bzz site — theme switcher (mirrors bzz built-in palettes), tabs, copy.
(() => {
  "use strict";

  // ---- terminal-compatible logo ----
  // Exact SVG geometry for Nerd Fonts v3 nf-md-hexagon_outline (F02D9)
  // and nf-md-hexagon_slice_1…6 (F0AC3…F0AC8), sourced from
  // Pictogrammers Material Design Icons under Apache-2.0.
  const logo = document.querySelector("[data-logo-animation]");
  if (logo) {
    const frame = logo.querySelector("[data-logo-frame]");
    const finalMark = logo.querySelector("[data-logo-final]");
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
    const frames = [
      "M21,16.5C21,16.88 20.79,17.21 20.47,17.38L12.57,21.82C12.41,21.94 12.21,22 12,22C11.79,22 11.59,21.94 11.43,21.82L3.53,17.38C3.21,17.21 3,16.88 3,16.5V7.5C3,7.12 3.21,6.79 3.53,6.62L11.43,2.18C11.59,2.06 11.79,2 12,2C12.21,2 12.41,2.06 12.57,2.18L20.47,6.62C20.79,6.79 21,7.12 21,7.5V16.5M12,4.15L5,8.09V15.91L12,19.85L19,15.91V8.09L12,4.15Z",
      "M21,16.5C21,16.88 20.79,17.21 20.47,17.38L12.57,21.82C12.41,21.94 12.21,22 12,22C11.79,22 11.59,21.94 11.43,21.82L3.53,17.38C3.21,17.21 3,16.88 3,16.5V7.5C3,7.12 3.21,6.79 3.53,6.62L11.43,2.18C11.59,2.06 11.79,2 12,2C12.21,2 12.41,2.06 12.57,2.18L20.47,6.62C20.79,6.79 21,7.12 21,7.5V16.5M12,4.15L5,8.09V15.91L12,19.85L19,15.91V8.09L12,4.15M12,12V5.32L17.94,8.66L12,12Z",
      "M18,15.38L12,12V5.32L18,8.69V15.38M21,16.5C21,16.88 20.79,17.21 20.47,17.38L12.57,21.82C12.41,21.94 12.21,22 12,22C11.79,22 11.59,21.94 11.43,21.82L3.53,17.38C3.21,17.21 3,16.88 3,16.5V7.5C3,7.12 3.21,6.79 3.53,6.62L11.43,2.18C11.59,2.06 11.79,2 12,2C12.21,2 12.41,2.06 12.57,2.18L20.47,6.62C20.79,6.79 21,7.12 21,7.5V16.5M12,4.15L5,8.09V15.91L12,19.85L19,15.91V8.09L12,4.15Z",
      "M12,5.32L18,8.69V15.31L12,18.68V5.32M21,16.5C21,16.88 20.79,17.21 20.47,17.38L12.57,21.82C12.41,21.94 12.21,22 12,22C11.79,22 11.59,21.94 11.43,21.82L3.53,17.38C3.21,17.21 3,16.88 3,16.5V7.5C3,7.12 3.21,6.79 3.53,6.62L11.43,2.18C11.59,2.06 11.79,2 12,2C12.21,2 12.41,2.06 12.57,2.18L20.47,6.62C20.79,6.79 21,7.12 21,7.5V16.5M12,4.15L5,8.09V15.91L12,19.85L19,15.91V8.09L12,4.15Z",
      "M12,5.32L18,8.69V15.31L12,18.68L6.06,15.34L12,12V5.32M21,16.5C21,16.88 20.79,17.21 20.47,17.38L12.57,21.82C12.41,21.94 12.21,22 12,22C11.79,22 11.59,21.94 11.43,21.82L3.53,17.38C3.21,17.21 3,16.88 3,16.5V7.5C3,7.12 3.21,6.79 3.53,6.62L11.43,2.18C11.59,2.06 11.79,2 12,2C12.21,2 12.41,2.06 12.57,2.18L20.47,6.62C20.79,6.79 21,7.12 21,7.5V16.5M12,4.15L5,8.09V15.91L12,19.85L19,15.91V8.09L12,4.15Z",
      "M12,5.32L18,8.69V15.31L12,18.68L6,15.31V8.69L12,12V5.32M21,16.5C21,16.88 20.79,17.21 20.47,17.38L12.57,21.82C12.41,21.94 12.21,22 12,22C11.79,22 11.59,21.94 11.43,21.82L3.53,17.38C3.21,17.21 3,16.88 3,16.5V7.5C3,7.12 3.21,6.79 3.53,6.62L11.43,2.18C11.59,2.06 11.79,2 12,2C12.21,2 12.41,2.06 12.57,2.18L20.47,6.62C20.79,6.79 21,7.12 21,7.5V16.5M12,4.15L5,8.09V15.91L12,19.85L19,15.91V8.09L12,4.15Z",
      "M12,5.32L18,8.69V15.31L12,18.68L6,15.31V8.69L12,5.32M21,16.5C21,16.88 20.79,17.21 20.47,17.38L12.57,21.82C12.41,21.94 12.21,22 12,22C11.79,22 11.59,21.94 11.43,21.82L3.53,17.38C3.21,17.21 3,16.88 3,16.5V7.5C3,7.12 3.21,6.79 3.53,6.62L11.43,2.18C11.59,2.06 11.79,2 12,2C12.21,2 12.41,2.06 12.57,2.18L20.47,6.62C20.79,6.79 21,7.12 21,7.5V16.5M12,4.15L5,8.09V15.91L12,19.85L19,15.91V8.09L12,4.15Z",
    ];
    const frameMs = 170;
    const finalHoldMs = 5000;
    let timer;

    const showFinalMark = () => {
      frame.setAttribute("hidden", "");
      finalMark.removeAttribute("hidden");
    };

    const playFrame = (index = 0) => {
      window.clearTimeout(timer);
      if (reducedMotion.matches || document.hidden) {
        showFinalMark();
        return;
      }
      if (index < frames.length) {
        finalMark.setAttribute("hidden", "");
        frame.removeAttribute("hidden");
        frame.setAttribute("d", frames[index]);
        timer = window.setTimeout(() => playFrame(index + 1), frameMs);
        return;
      }
      showFinalMark();
      timer = window.setTimeout(() => playFrame(0), finalHoldMs);
    };

    const restartLogo = () => {
      window.clearTimeout(timer);
      showFinalMark();
      if (!reducedMotion.matches && !document.hidden) {
        window.requestAnimationFrame(() => playFrame(0));
      }
    };

    if (typeof reducedMotion.addEventListener === "function") {
      reducedMotion.addEventListener("change", restartLogo);
    } else {
      reducedMotion.addListener(restartLogo);
    }
    document.addEventListener("visibilitychange", restartLogo);
    restartLogo();
  }

  // ---- theme switcher ----
  const root = document.documentElement;
  const themeButtons = document.querySelectorAll("[data-set-theme]");
  const saved = localStorage.getItem("bzz-theme");
  if (saved) setTheme(saved);

  function setTheme(id) {
    root.dataset.theme = id;
    localStorage.setItem("bzz-theme", id);
    themeButtons.forEach((b) =>
      b.setAttribute("aria-pressed", String(b.dataset.setTheme === id)),
    );
  }

  themeButtons.forEach((b) =>
    b.addEventListener("click", () => setTheme(b.dataset.setTheme)),
  );

  // ---- install tabs ----
  const tabs = document.querySelectorAll("[data-tab]");
  const panels = document.querySelectorAll("[data-panel]");
  tabs.forEach((t) =>
    t.addEventListener("click", () => {
      tabs.forEach((x) =>
        x.setAttribute("aria-selected", String(x === t)),
      );
      panels.forEach((p) => {
        p.hidden = p.dataset.panel !== t.dataset.tab;
      });
    }),
  );

  // ---- copy button ----
  document.querySelectorAll("[data-copy]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const el = document.querySelector(btn.dataset.copy);
      if (!el) return;
      try {
        await navigator.clipboard.writeText(el.textContent.trim());
        const old = btn.textContent;
        btn.textContent = "copied";
        setTimeout(() => (btn.textContent = old), 1200);
      } catch {
        /* clipboard unavailable; ignore */
      }
    });
  });
})();

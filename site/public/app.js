// bzz site — theme switcher (mirrors bzz built-in palettes), tabs, copy.
(() => {
  "use strict";

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

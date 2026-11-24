// Replays a record-tape.py tape inside a HyperFrames composition.
//
// The tape holds bzz's real output bytes. A real terminal emulator (xterm.js
// with its image addon for Sixel) draws them at the film time HyperFrames
// seeks to, so every render is frame-identical and text is re-rasterized at
// the render resolution (sharp under push-ins and at 4K).
//
// Pinned: @xterm/xterm 6.0.0, @xterm/addon-image 0.9.0, @xterm/addon-unicode11 0.9.0. `_renderRows` is a
// private API: the film clock is paused during capture, so xterm's
// requestAnimationFrame refresh never runs and rows must be drawn directly.
// Re-verify it when upgrading xterm.
//
// Usage (one call per terminal shot):
//   mountTape({ id: "team", el: "#term", url: "assets/tapes/team.jsonl",
//               map: [[8.0, 7.8], [13.3, 13.1]], width: 1440, height: 752 });
//
// `map` is a list of [filmSecond, tapeSecond] keyframes, linear between
// them and held outside: an offset, a speed-up (typing), a hold, or a jump
// cut (two keyframes at the same film second). `filmStart`/`tapeStart` are
// the one-keyframe shorthand. `align: "center"` centres a small terminal.
(function () {
  function decode(base64) {
    return Uint8Array.from(atob(base64), (c) => c.charCodeAt(0));
  }

  window.mountTape = function mountTape(options) {
    const { id, el, url, width, height } = options;
    const map = options.map || [[options.filmStart, options.tapeStart]];
    const tapeAt = (film) => {
      if (film <= map[0][0]) return map[0][1];
      for (let i = map.length - 1; i >= 0; i--) {
        const [f0, t0] = map[i];
        if (film < f0) continue;
        const next = map[i + 1];
        if (!next) return map.length === 1 ? t0 + (film - f0) : t0;
        return t0 + ((film - f0) * (next[1] - t0)) / Math.max(1e-9, next[0] - f0);
      }
      return map[0][1];
    };
    const fontSize = options.fontSize || 20;
    const theme = options.theme || { background: "#14110d" };
    let term, events, next = 0, shownUntil = -1;

    const ready = (async () => {
      await document.fonts.load(`${fontSize}px "JetBrains Mono"`);
      await document.fonts.load(`700 ${fontSize}px "JetBrains Mono"`);
      const text = await (await fetch(url)).text();
      const lines = text.split("\n").filter((l) => l.trim()).map((l) => JSON.parse(l));
      const header = lines[0];
      if (header.version !== 1) throw new Error(`unsupported tape version in ${url}`);
      events = lines.slice(1).filter((e) => e.o).map((e) => ({ t: e.t, bytes: decode(e.o) }));
      term = new Terminal({
        cols: header.cols, rows: header.rows, fontFamily: '"JetBrains Mono"', fontSize,
        lineHeight: 1.0, allowProposedApi: true, cursorBlink: false, scrollback: 0, theme,
      });
      term.loadAddon(new ImageAddon.ImageAddon({ sixelSupport: true }));
      // Emoji are two cells wide, as in bzz's own width tables.
      term.loadAddon(new Unicode11Addon.Unicode11Addon());
      term.unicode.activeVersion = "11";
      const host = document.querySelector(el);
      term.open(host);
      const cell = term._core._renderService.dimensions.css.cell;
      const scale = Math.min(width / (cell.width * header.cols), height / (cell.height * header.rows));
      host.style.transformOrigin = "0 0";
      const dy = options.align === "center" ? (height - scale * cell.height * header.rows) / 2 : 0;
      const dx = options.align === "center" ? (width - scale * cell.width * header.cols) / 2 : 0;
      host.style.transform = `translate(${dx}px, ${dy}px) scale(${scale})`;
    })();
    window.__hf = window.__hf || {};
    window.__hf.buildReady = window.__hf.buildReady || {};
    window.__hf.buildReady[`tape-${id}`] = ready;

    async function seek(filmTime) {
      await ready;
      const tapeTime = tapeAt(filmTime);
      if (tapeTime < shownUntil) {
        term.reset();
        next = 0;
      }
      const chunks = [];
      while (next < events.length && events[next].t <= tapeTime) chunks.push(events[next++].bytes);
      shownUntil = tapeTime;
      if (chunks.length) {
        const all = new Uint8Array(chunks.reduce((n, c) => n + c.length, 0));
        let at = 0;
        for (const c of chunks) {
          all.set(c, at);
          at += c.length;
        }
        await new Promise((resolve) => term.write(all, resolve));
      }
      term._core._renderService._renderRows(0, term.rows - 1);
    }
    window.addEventListener("hf-seek", (e) => e.detail.waitUntil(seek(e.detail.time)));
  };
})();

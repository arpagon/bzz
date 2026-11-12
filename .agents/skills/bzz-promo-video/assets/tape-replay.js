// Replays a record-tape.py tape inside a HyperFrames composition.
//
// The tape holds bzz's real output bytes. A real terminal emulator (xterm.js
// with its image addon for Sixel) draws them at the film time HyperFrames
// seeks to, so every render is frame-identical and text is re-rasterized at
// the render resolution (sharp under push-ins and at 4K).
//
// Pinned: @xterm/xterm 6.0.0, @xterm/addon-image 0.9.0. `_renderRows` is a
// private API: the film clock is paused during capture, so xterm's
// requestAnimationFrame refresh never runs and rows must be drawn directly.
// Re-verify it when upgrading xterm.
//
// Usage (one call per terminal shot):
//   mountTape({ id: "team", el: "#term", url: "assets/tapes/team.json",
//               filmStart: 1.6, tapeStart: 2.6, width: 1600, height: 856 });
(function () {
  function decode(base64) {
    return Uint8Array.from(atob(base64), (c) => c.charCodeAt(0));
  }

  window.mountTape = function mountTape(options) {
    const { id, el, url, filmStart, tapeStart, width, height } = options;
    const fontSize = options.fontSize || 20;
    const theme = options.theme || { background: "#14110d" };
    let term, events, next = 0, shownUntil = -1;

    const ready = (async () => {
      await document.fonts.load(`${fontSize}px "JetBrains Mono"`);
      await document.fonts.load(`700 ${fontSize}px "JetBrains Mono"`);
      const lines = await (await fetch(url)).json();
      const header = lines[0];
      if (header.version !== 1) throw new Error(`unsupported tape version in ${url}`);
      events = lines.slice(1).filter((e) => e.o).map((e) => ({ t: e.t, bytes: decode(e.o) }));
      term = new Terminal({
        cols: header.cols, rows: header.rows, fontFamily: '"JetBrains Mono"', fontSize,
        lineHeight: 1.0, allowProposedApi: true, cursorBlink: false, scrollback: 0, theme,
      });
      term.loadAddon(new ImageAddon.ImageAddon({ sixelSupport: true }));
      const host = document.querySelector(el);
      term.open(host);
      const cell = term._core._renderService.dimensions.css.cell;
      const scale = Math.min(width / (cell.width * header.cols), height / (cell.height * header.rows));
      host.style.transformOrigin = "0 0";
      host.style.transform = `scale(${scale})`;
    })();
    window.__hf = window.__hf || {};
    window.__hf.buildReady = window.__hf.buildReady || {};
    window.__hf.buildReady[`tape-${id}`] = ready;

    async function seek(filmTime) {
      await ready;
      const tapeTime = Math.max(0, filmTime - filmStart) + tapeStart;
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

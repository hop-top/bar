// ANSI text (SGR colours, bold, OSC 8 links) to HTML, for the preview.
// Covers what bar emits: 16 and 256 colours, 24-bit colours, bold, faint,
// italic, underline, inverse, and OSC 8 links ended by ST or BEL.
"use strict";

const Ansi = (() => {
  // xterm defaults for the 16 base colours
  const BASE = [
    "#000000", "#cd0000", "#00cd00", "#cdcd00", "#0000ee", "#cd00cd", "#00cdcd", "#e5e5e5",
    "#7f7f7f", "#ff0000", "#00ff00", "#ffff00", "#5c5cff", "#ff00ff", "#00ffff", "#ffffff",
  ];

  const hex = (n) => n.toString(16).padStart(2, "0");

  function color256(n) {
    if (n < 16) return BASE[n];
    if (n < 232) {
      const i = n - 16;
      const step = (v) => (v === 0 ? 0 : 55 + v * 40);
      return "#" + hex(step(Math.floor(i / 36))) + hex(step(Math.floor(i / 6) % 6)) + hex(step(i % 6));
    }
    const g = 8 + (n - 232) * 10;
    return "#" + hex(g) + hex(g) + hex(g);
  }

  const escapeHtml = (s) =>
    s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

  function fresh() {
    return { fg: null, bg: null, bold: false, faint: false, italic: false, underline: false, inverse: false };
  }

  // Applies one SGR parameter list to `st`.
  function sgr(st, params) {
    const p = params.length ? params : [0];
    for (let i = 0; i < p.length; i++) {
      const n = p[i];
      if (n === 0) Object.assign(st, fresh());
      else if (n === 1) st.bold = true;
      else if (n === 2) st.faint = true;
      else if (n === 3) st.italic = true;
      else if (n === 4) st.underline = true;
      else if (n === 7) st.inverse = true;
      else if (n === 22) st.bold = st.faint = false;
      else if (n === 23) st.italic = false;
      else if (n === 24) st.underline = false;
      else if (n === 27) st.inverse = false;
      else if (n >= 30 && n <= 37) st.fg = BASE[n - 30];
      else if (n >= 90 && n <= 97) st.fg = BASE[n - 82];
      else if (n === 39) st.fg = null;
      else if (n >= 40 && n <= 47) st.bg = BASE[n - 40];
      else if (n >= 100 && n <= 107) st.bg = BASE[n - 92];
      else if (n === 49) st.bg = null;
      else if (n === 38 || n === 48) {
        let c = null;
        if (p[i + 1] === 5 && i + 2 < p.length) {
          c = color256(p[i + 2] & 255);
          i += 2;
        } else if (p[i + 1] === 2 && i + 4 < p.length) {
          c = "#" + hex(p[i + 2] & 255) + hex(p[i + 3] & 255) + hex(p[i + 4] & 255);
          i += 4;
        }
        if (n === 38) st.fg = c;
        else st.bg = c;
      }
    }
  }

  function style(st) {
    let fg = st.fg;
    let bg = st.bg;
    if (st.inverse) [fg, bg] = [bg || "var(--term-bg)", fg || "var(--term-fg)"];
    const css = [];
    if (fg) css.push("color:" + fg);
    if (bg) css.push("background:" + bg);
    if (st.bold) css.push("font-weight:700");
    if (st.faint) css.push("opacity:.6");
    if (st.italic) css.push("font-style:italic");
    if (st.underline) css.push("text-decoration:underline");
    return css.join(";");
  }

  // One row of ANSI text to HTML; `state` carries SGR and link across rows.
  function row(text, state) {
    let out = "";
    let buf = "";
    const flush = () => {
      if (!buf) return;
      const css = style(state.sgr);
      const body = escapeHtml(buf);
      out += css ? `<span style="${css}">${body}</span>` : body;
      buf = "";
    };
    const openLink = (url) => {
      const safe = escapeHtml(url);
      return /^https?:\/\//i.test(url)
        ? `<a href="${safe}" title="${safe}" target="_blank" rel="noopener noreferrer">`
        : `<a class="nolink" title="${safe}">`;
    };
    if (state.link) out += openLink(state.link);
    let i = 0;
    while (i < text.length) {
      const ch = text[i];
      if (ch !== "\x1b") {
        buf += ch;
        i++;
        continue;
      }
      const next = text[i + 1];
      if (next === "[") {
        const m = /^\x1b\[([0-9;:]*)([@-~])/.exec(text.slice(i));
        if (!m) break;
        if (m[2] === "m") {
          flush();
          sgr(state.sgr, m[1] === "" ? [] : m[1].split(/[;:]/).map((x) => parseInt(x || "0", 10)));
        }
        i += m[0].length;
      } else if (next === "]") {
        // OSC: up to ST (ESC \) or BEL
        let end = i + 2;
        let stop = -1;
        let len = 0;
        while (end < text.length) {
          if (text[end] === "\x07") {
            stop = end;
            len = 1;
            break;
          }
          if (text[end] === "\x1b" && text[end + 1] === "\\") {
            stop = end;
            len = 2;
            break;
          }
          end++;
        }
        if (stop < 0) break;
        const body = text.slice(i + 2, stop);
        const osc8 = /^8;[^;]*;(.*)$/s.exec(body);
        if (osc8) {
          flush();
          if (state.link) out += "</a>";
          state.link = osc8[1] || null;
          if (state.link) out += openLink(state.link);
        }
        i = stop + len;
      } else {
        i += 2;
      }
    }
    flush();
    if (state.link) out += "</a>";
    return out;
  }

  // ANSI text to an array of HTML rows.
  function toHtml(text) {
    const state = { sgr: fresh(), link: null };
    return text.split("\n").map((r) => row(r, state));
  }

  return { toHtml, color256 };
})();

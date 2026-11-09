// Layout composer: one status line at a time, its left, center and right
// groups as chips (placeholders) and text pieces, each placeholder with
// its options as `bar check` knows them.
"use strict";

const Compose = (() => {
  const SIDES = ["left", "center", "right"];
  const LINES = 4;
  let ui = null;
  let line = 1;
  let open = null; // {side, i}: the chip whose options are shown
  let groups = null; // side -> tokens, for the shown line
  let lines = null; // inspect().lines

  const el = (tag, props = {}, ...kids) => {
    const n = Object.assign(document.createElement(tag), props);
    for (const k of kids) if (k != null) n.append(k);
    return n;
  };

  function init(opts) {
    ui = opts;
  }

  // Redraws from a fresh `inspect` result (null when the config has errors).
  function update(inspect) {
    lines = inspect ? inspect.lines : null;
    ui.off.hidden = !!lines;
    ui.groups.hidden = !lines;
    drawTabs();
    if (!lines) return;
    groups = {};
    SIDES.forEach((side, s) => {
      const t = ui.tokens(lines[line - 1][s]);
      groups[side] = t.ok ? t.tokens : [];
    });
    drawGroups();
  }

  function drawTabs() {
    ui.tabs.replaceChildren();
    for (let n = 1; n <= LINES; n++) {
      const used = lines && lines[n - 1].some((g) => g !== "");
      const tab = el("button", {
        className: "tab" + (used ? "" : " empty"),
        textContent: `line ${n}`,
        type: "button",
      });
      tab.setAttribute("role", "tab");
      tab.setAttribute("aria-selected", String(n === line));
      tab.disabled = !lines;
      tab.onclick = () => {
        line = n;
        open = null;
        update({ lines });
      };
      ui.tabs.append(tab);
    }
  }

  function commit(side) {
    ui.onGroup(side, line, Conf.serialize(groups[side]));
  }

  function drawGroups() {
    ui.groups.replaceChildren(...SIDES.map(drawGroup));
  }

  function drawGroup(side) {
    const toks = groups[side];
    const items = el("div", { className: "items" });
    toks.forEach((t, i) => items.append("text" in t ? textChip(side, i) : placeholderChip(side, i)));
    const box = el("div", { className: "group" }, el("h3", { textContent: side }), items);
    if (open && open.side === side && toks[open.i] && !("text" in toks[open.i])) {
      box.append(optionsEditor(side, open.i));
    }
    box.append(adders(side));
    return box;
  }

  function move(side, i, d) {
    const toks = groups[side];
    const j = i + d;
    if (j < 0 || j >= toks.length) return;
    [toks[i], toks[j]] = [toks[j], toks[i]];
    if (open && open.side === side && open.i === i) open.i = j;
    commit(side);
  }

  function remove(side, i) {
    groups[side].splice(i, 1);
    if (open && open.side === side) open = null;
    commit(side);
  }

  function chipButtons(side, i) {
    const b = (label, title, fn, cls = "") =>
      el("button", { type: "button", textContent: label, title, className: cls, onclick: fn });
    return [
      b("‹", "move left", () => move(side, i, -1)),
      b("›", "move right", () => move(side, i, 1)),
      b("×", "remove", () => remove(side, i), "x"),
    ];
  }

  function textChip(side, i) {
    const t = groups[side][i];
    const input = el("input", { value: t.text, title: "text", spellcheck: false });
    input.style.width = Math.max(2, [...t.text].length + 1) + "ch";
    input.onchange = () => {
      t.text = input.value;
      if (t.text === "") groups[side].splice(i, 1);
      commit(side);
    };
    return el("span", { className: "chip text" }, input, ...chipButtons(side, i));
  }

  function placeholderChip(side, i) {
    const t = groups[side][i];
    const isOpen = open && open.side === side && open.i === i;
    const summary = t.options.map(([k, v]) => (v === "" ? k : `${k}=${v}`)).join(",");
    const name = el("button", {
      type: "button",
      className: "name",
      textContent: summary ? `${t.name}[${summary}]` : t.name,
      title: "options",
      onclick: () => {
        open = isOpen ? null : { side, i };
        drawGroups();
      },
    });
    name.setAttribute("aria-expanded", String(isOpen));
    return el("span", { className: "chip" + (isOpen ? " open" : "") }, name, ...chipButtons(side, i));
  }

  function optionsEditor(side, i) {
    const t = groups[side][i];
    const spec = ui.options(t.name);
    const get = (k) => (t.options.find(([o]) => o === k) || [null, ""])[1];
    const put = (k, v) => {
      const at = t.options.findIndex(([o]) => o === k);
      if (v === "" && at >= 0) t.options.splice(at, 1);
      else if (v !== "" && at >= 0) t.options[at][1] = v;
      else if (v !== "") t.options.push([k, v]);
      commit(side);
    };
    const field = (o) => {
      const cur = get(o.key);
      const name = o.label || o.key;
      const tip = (n) => {
        if (o.help) n.title = o.help;
        return n;
      };
      if (o.kind === "one") {
        const sel = el("select", {}, el("option", { value: "", textContent: "(default)" }));
        for (const v of o.values) sel.append(el("option", { value: v, textContent: v, selected: v === cur }));
        sel.onchange = () => put(o.key, sel.value);
        return tip(el("label", {}, name, sel));
      }
      if (o.kind === "list") {
        const on = new Set(cur.split(",").filter(Boolean));
        const box = el("span", { className: "list" });
        for (const v of o.values) {
          const cb = el("input", { type: "checkbox", checked: on.has(v) });
          cb.onchange = () => {
            const next = o.values.filter((x) => (x === v ? cb.checked : on.has(x)));
            put(o.key, next.join(","));
          };
          box.append(el("label", {}, cb, v));
        }
        return tip(el("label", {}, name, box));
      }
      const num = o.kind === "int" || o.kind === "pos";
      const input = el("input", { type: num ? "number" : "text", value: cur, min: o.kind === "pos" ? 1 : 0 });
      if (o.placeholder) input.placeholder = o.placeholder;
      // suggested values for free-form options
      if (o.values && o.values.length) {
        const list = el("datalist", { id: `opt-${t.name}-${o.key}` });
        for (const v of o.values) list.append(el("option", { value: v }));
        input.setAttribute("list", list.id);
        input.onchange = () => put(o.key, input.value.trim());
        return tip(el("label", {}, name, input, list));
      }
      input.onchange = () => put(o.key, input.value.trim());
      return tip(el("label", {}, name, input));
    };
    const known = new Set(spec.map((o) => o.key).concat(["icon", "bg"]));
    const extra = t.options.filter(([k]) => !known.has(k)).map(([k]) => ({ key: k, kind: "any", values: [] }));
    const all = spec.concat([{ key: "icon", kind: "any", values: [] }, { key: "bg", kind: "int", values: [] }], extra);
    return el("div", { className: "opts" }, ...all.map(field));
  }

  function adders(side) {
    const sel = el("select", { title: "add a placeholder" }, el("option", { value: "", textContent: "+ placeholder" }));
    const core = el("optgroup", { label: "core" });
    for (const p of ui.schema.placeholders) core.append(el("option", { value: p.name, textContent: p.name }));
    sel.append(core);
    const plugins = el("optgroup", { label: "plugins" });
    for (const p of ui.plugins()) {
      const label = p.category ? `${p.name} (${p.category})` : p.name;
      plugins.append(el("option", { value: p.name, textContent: label, title: p.description || "" }));
    }
    plugins.append(el("option", { value: "\u0000plugin", textContent: "other plugin…" }));
    sel.append(plugins);
    sel.onchange = () => {
      let name = sel.value;
      if (name === "\u0000plugin") name = (prompt("Plugin name (bar-<name> on PATH):") || "").trim();
      if (!/^[A-Za-z0-9_.-]+$/.test(name)) return drawGroups();
      groups[side].push({ name, options: [] });
      open = { side, i: groups[side].length - 1 };
      commit(side);
    };
    const sep = el("button", {
      type: "button",
      className: "btn small",
      textContent: "+ separator",
      title: "split segments here",
      onclick: () => {
        groups[side].push({ text: ` ${ui.separator()} ` });
        commit(side);
      },
    });
    const text = el("button", {
      type: "button",
      className: "btn small",
      textContent: "+ text",
      onclick: () => {
        const s = prompt("Text:", ":");
        if (!s) return;
        groups[side].push({ text: s });
        commit(side);
      },
    });
    return el("div", { className: "adders" }, sel, sep, text);
  }

  return { init, update };
})();

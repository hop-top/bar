// Layout composer: one status line at a time, its left, center and right
// groups as chips (placeholders and text), each placeholder with its
// options as `bar check` knows them.
//
// Keyboard: one tab stop per chip list; arrows move between chips, Alt+
// arrows reorder, Delete removes, Enter opens the chip's editor, Escape
// closes it. Line tabs follow the tabs pattern (arrows, Home, End).
"use strict";

const Compose = (() => {
  const SIDES = ["left", "center", "right"];
  const MIN_TABS = 4; // tabs shown at least; a spare empty line follows the last used one
  let maxLines = 64; // inspect().max_lines
  let ui = null;
  let line = 1;
  let open = null; // {side, i}: the chip whose editor is shown
  let groups = null; // side -> tokens, for the shown line
  let lines = null; // inspect().lines
  const active = { left: 0, center: 0, right: 0 }; // roving tab stop per group
  let focusNext = null; // data-focus key to focus after the next draw
  let picker = null;

  const el = (tag, props = {}, ...kids) => {
    const n = Object.assign(document.createElement(tag), props);
    for (const k of kids) if (k != null) n.append(k);
    return n;
  };
  const attrs = (n, a) => {
    for (const [k, v] of Object.entries(a)) n.setAttribute(k, v);
    return n;
  };

  function init(opts) {
    ui = opts;
  }

  // The data-focus key of the focused element inside the composer.
  function focusedKey() {
    const a = document.activeElement;
    return a && (ui.groups.contains(a) || ui.tabs.contains(a)) ? a.dataset.focus || null : null;
  }

  function restoreFocus(key) {
    if (!key) return;
    const k = CSS.escape(key);
    const n = document.querySelector(`[data-focus="${k}"]`) || document.querySelector(`[data-focus^="${k}:"]`);
    if (n && !n.disabled) n.focus();
  }

  // Redraws from a fresh `inspect` result (null when the config has errors).
  function update(inspect) {
    const key = focusNext || focusedKey();
    focusNext = null;
    lines = inspect ? inspect.lines : null;
    if (inspect && inspect.max_lines) maxLines = inspect.max_lines;
    ui.off.hidden = !!lines;
    ui.groups.hidden = !lines;
    drawTabs();
    if (!lines) {
      if (picker) picker.close(false);
      return;
    }
    groups = {};
    SIDES.forEach((side, s) => {
      const t = ui.tokens(lines[line - 1] ? lines[line - 1][s] : "");
      groups[side] = t.ok ? t.tokens : [];
    });
    if (open && !(groups[open.side] && groups[open.side][open.i])) open = null;
    drawGroups();
    restoreFocus(key);
  }

  // Tabs for every line up to the last one in use, one spare line to grow
  // into, and the shown line; at least MIN_TABS, at most the line ceiling.
  function tabCount() {
    let last = 0;
    (lines || []).forEach((l, i) => {
      if (l.some((g) => g !== "")) last = i + 1;
    });
    return Math.min(maxLines, Math.max(MIN_TABS, last + 1, line));
  }

  function drawTabs() {
    ui.tabs.replaceChildren();
    const count = tabCount();
    for (let n = 1; n <= count; n++) {
      const used = lines && lines[n - 1] && lines[n - 1].some((g) => g !== "");
      const tab = el("button", {
        className: "tab" + (used ? "" : " empty"),
        textContent: `line ${n}`,
        type: "button",
        id: `line-tab-${n}`,
        tabIndex: n === line ? 0 : -1,
        disabled: !lines,
      });
      tab.dataset.focus = `tab:${n}`;
      attrs(tab, { role: "tab", "aria-selected": String(n === line), "aria-controls": ui.groups.id });
      if (!used) tab.title = "empty line";
      tab.onclick = () => select(n);
      tab.onkeydown = (e) => {
        const to = { ArrowRight: n + 1, ArrowLeft: n - 1, Home: 1, End: count }[e.key];
        if (to === undefined) return;
        e.preventDefault();
        select(((to - 1 + count) % count) + 1);
      };
      ui.tabs.append(tab);
    }
    ui.groups.setAttribute("aria-labelledby", `line-tab-${line}`);
  }

  function select(n) {
    line = n;
    open = null;
    focusNext = `tab:${n}`;
    update({ lines });
  }

  // Writes group `side` back; `focus` is the element to focus afterwards.
  function commit(side, focus = null) {
    focusNext = focus;
    ui.onGroup(side, line, Conf.serialize(groups[side]));
  }

  function drawGroups() {
    ui.groups.replaceChildren(...SIDES.map(drawGroup));
  }

  const label = (t) => ("text" in t ? textLabel(t).spoken : t.name);

  function drawGroup(side) {
    const toks = groups[side];
    if (active[side] >= toks.length) active[side] = Math.max(0, toks.length - 1);
    const head = el("h3", { id: `group-${side}`, textContent: side });
    const items = attrs(el("div", { className: "items" }), {
      role: "group",
      "aria-labelledby": `group-${side}`,
      "aria-describedby": "chip-keys",
    });
    toks.forEach((t, i) => items.append(chip(side, i)));
    if (!toks.length) items.append(el("span", { className: "items-empty", textContent: "empty" }));
    const box = el("div", { className: "group" }, head, items);
    if (open && open.side === side && toks[open.i]) box.append(editor(side, open.i));
    box.append(adders(side));
    return box;
  }

  function move(side, i, d) {
    const toks = groups[side];
    const j = i + d;
    if (j < 0 || j >= toks.length) return;
    [toks[i], toks[j]] = [toks[j], toks[i]];
    if (open && open.side === side && open.i === i) open.i = j;
    active[side] = j;
    ui.announce(`${label(toks[j])} moved to position ${j + 1} of ${toks.length}`);
    commit(side, `chip:${side}:${j}`);
  }

  function remove(side, i) {
    const [gone] = groups[side].splice(i, 1);
    if (open && open.side === side) open = null;
    const left = groups[side].length;
    active[side] = Math.min(i, left - 1);
    ui.announce(`${label(gone)} removed`);
    commit(side, left ? `chip:${side}:${active[side]}` : `add:${side}`);
  }

  function focusChip(side, i) {
    const n = groups[side].length;
    if (!n) return;
    active[side] = Math.max(0, Math.min(n - 1, i));
    for (const b of ui.groups.querySelectorAll(`[data-chip-side="${side}"]`)) b.tabIndex = -1;
    const b = ui.groups.querySelector(`[data-focus="chip:${side}:${active[side]}"]`);
    if (b) {
      b.tabIndex = 0;
      b.focus();
    }
  }

  function toggle(side, i) {
    const isOpen = open && open.side === side && open.i === i;
    open = isOpen ? null : { side, i };
    active[side] = i;
    drawGroups();
    restoreFocus(isOpen ? `chip:${side}:${i}` : `edit:${side}:${i}`);
  }

  // A text piece as shown: spaces made visible, and how it is read out.
  function textLabel(t) {
    const s = t.text;
    if (s.trim() === "") return { shown: "␠".repeat([...s].length), spoken: s.length > 1 ? `${s.length} spaces` : "space" };
    const sep = ui.separator();
    if (s.trim() === sep.trim() && sep.trim() !== "") return { shown: sep.trim(), spoken: `separator ${sep.trim()}`, sep: true };
    return { shown: s.replace(/^ +| +$/g, (m) => "␠".repeat(m.length)), spoken: `text "${s}"` };
  }

  function chip(side, i) {
    const t = groups[side][i];
    const isText = "text" in t;
    const isOpen = !!(open && open.side === side && open.i === i);
    const n = groups[side].length;
    let main;
    let name;
    if (isText) {
      const l = textLabel(t);
      name = l.spoken;
      main = el(
        "button",
        { type: "button", className: "name" },
        el("span", { textContent: l.shown }),
        el("span", { className: "badge", textContent: l.sep ? "separator" : "text" }),
      );
    } else {
      const summary = t.options.map(([k, v]) => (v === "" ? k : `${k}=${v}`)).join(",");
      name = t.name;
      main = el("button", { type: "button", className: "name", textContent: summary ? `${t.name}[${summary}]` : t.name });
    }
    main.dataset.focus = `chip:${side}:${i}`;
    main.dataset.chipSide = side;
    main.tabIndex = i === active[side] ? 0 : -1;
    attrs(main, {
      "aria-expanded": String(isOpen),
      "aria-label": `${name}, ${i + 1} of ${n}, ${isText ? "edit text" : "options"}`,
    });
    main.onclick = () => toggle(side, i);
    main.onfocus = () => {
      active[side] = i;
    };
    main.onkeydown = (e) => {
      const k = e.key;
      if ((k === "ArrowLeft" || k === "ArrowRight") && e.altKey) {
        e.preventDefault();
        move(side, i, k === "ArrowLeft" ? -1 : 1);
      } else if (k === "ArrowLeft" || k === "ArrowRight") {
        e.preventDefault();
        focusChip(side, i + (k === "ArrowLeft" ? -1 : 1));
      } else if (k === "Home" || k === "End") {
        e.preventDefault();
        focusChip(side, k === "Home" ? 0 : n - 1);
      } else if (k === "Delete" || k === "Backspace") {
        e.preventDefault();
        remove(side, i);
      } else if (k === "Escape" && isOpen) {
        e.preventDefault();
        toggle(side, i);
      }
    };
    const b = (text, aria, fn, cls = "") => {
      const btn = el("button", { type: "button", textContent: text, className: "tool " + cls, tabIndex: -1, onclick: fn });
      attrs(btn, { "aria-label": aria, title: aria });
      return btn;
    };
    return el(
      "span",
      { className: "chip" + (isText ? " text" : "") + (isOpen ? " open" : "") },
      main,
      b("‹", `move ${name} left`, () => move(side, i, -1)),
      b("›", `move ${name} right`, () => move(side, i, 1)),
      b("×", `remove ${name}`, () => remove(side, i), "x"),
    );
  }

  // The editor under a group for its open chip: text, or options.
  function editor(side, i) {
    const t = groups[side][i];
    const close = el("button", { type: "button", className: "btn small", textContent: "Done" });
    close.onclick = () => toggle(side, i);
    const box = el("div", { className: "opts" });
    box.onkeydown = (e) => {
      if (e.key === "Escape") {
        e.preventDefault();
        toggle(side, i);
      }
    };
    if ("text" in t) {
      const input = el("input", { type: "text", value: t.text, spellcheck: false });
      input.dataset.focus = `edit:${side}:${i}`;
      input.onchange = () => {
        if (input.value === "") return remove(side, i);
        t.text = input.value;
        commit(side, `edit:${side}:${i}`);
      };
      box.append(el("label", { className: "wide" }, "text (spaces count)", input), close);
      return box;
    }
    const spec = ui.options(t.name);
    const get = (k) => (t.options.find(([o]) => o === k) || [null, ""])[1];
    const put = (k, v, key) => {
      const at = t.options.findIndex(([o]) => o === k);
      if (v === "" && at >= 0) t.options.splice(at, 1);
      else if (v !== "" && at >= 0) t.options[at][1] = v;
      else if (v !== "") t.options.push([k, v]);
      commit(side, key);
    };
    const shownDefault = (o) => (o.default === null || o.default === undefined || o.default === "" ? "" : String(o.default));
    let first = true;
    const field = (o) => {
      const cur = get(o.key);
      const key = `opt:${side}:${i}:${o.key}`;
      const def = shownDefault(o);
      const help = o.help ? el("span", { className: "help", textContent: o.help }) : null;
      const mark = (n) => {
        n.dataset.focus = first ? `edit:${side}:${i}` : key;
        if (first) n.dataset.alt = key;
        first = false;
        return n;
      };
      const name = el("span", { className: "opt-name", textContent: o.label || o.key });
      if (o.kind === "one") {
        const sel = mark(el("select", {}, el("option", { value: "", textContent: def ? `default (${def})` : "default" })));
        for (const v of o.values) sel.append(el("option", { value: v, textContent: v, selected: v === cur }));
        sel.onchange = () => put(o.key, sel.value, sel.dataset.focus);
        return el("label", {}, name, sel, help);
      }
      if (o.kind === "list") {
        const on = new Set(cur.split(",").filter(Boolean));
        const box = el("span", { className: "list" });
        for (const v of o.values) {
          const cb = mark(el("input", { type: "checkbox", checked: on.has(v) }));
          cb.dataset.focus += `:${v}`;
          cb.onchange = () => {
            const next = o.values.filter((x) => (x === v ? cb.checked : on.has(x)));
            put(o.key, next.join(","), cb.dataset.focus);
          };
          box.append(el("label", {}, cb, v));
        }
        const legend = def ? `${o.label || o.key} (default ${def})` : o.label || o.key;
        return el("fieldset", { className: "list-field" }, el("legend", { textContent: legend }), box, help);
      }
      const num = o.kind === "int" || o.kind === "pos";
      const input = mark(el("input", { type: num ? "number" : "text", value: cur, min: o.kind === "pos" ? 1 : 0 }));
      input.placeholder = o.placeholder || (def ? `default ${def}` : "");
      input.onchange = () => put(o.key, input.value.trim(), input.dataset.focus);
      const kids = [name, input];
      // suggested values for free-form options
      if (o.values && o.values.length) {
        const list = el("datalist", { id: `opt-${side}-${i}-${o.key}` });
        for (const v of o.values) list.append(el("option", { value: v }));
        input.setAttribute("list", list.id);
        kids.push(list);
      }
      return el("label", {}, ...kids, help);
    };
    const generic = [
      { key: "icon", kind: "any", values: [], help: "a glyph (at most 2 cells), or none to hide it" },
      { key: "bg", kind: "int", values: [], help: "segment background 0-255 under powerline or capsule" },
    ];
    const known = new Set(spec.map((o) => o.key).concat(generic.map((o) => o.key)));
    const extra = t.options.filter(([k]) => !known.has(k)).map(([k]) => ({ key: k, kind: "any", values: [] }));
    const desc = ui.describe(t.name);
    if (desc) box.append(el("p", { className: "opts-desc", textContent: `${t.name}: ${desc}` }));
    for (const o of spec.concat(generic, extra)) box.append(field(o));
    box.append(close);
    return box;
  }

  // Adds placeholder `name` to `side`, after a separator when the group
  // ends in a placeholder, so values never run together.
  function add(side, name) {
    const toks = groups[side];
    if (toks.length && !("text" in toks[toks.length - 1])) toks.push({ text: ` ${ui.separator()} ` });
    toks.push({ name, options: [] });
    const i = toks.length - 1;
    active[side] = i;
    open = ui.options(name).length ? { side, i } : null;
    ui.announce(`${name} added to ${side}`);
    commit(side, `chip:${side}:${i}`);
  }

  function adders(side) {
    const host = el("div", { className: "picker-host" });
    const pick = el("button", { type: "button", className: "btn small", textContent: "+ placeholder" });
    pick.dataset.focus = `add:${side}`;
    attrs(pick, { "aria-haspopup": "listbox", "aria-label": `add a placeholder to ${side}` });
    pick.onclick = () => {
      if (picker) picker.close(false);
      picker = Picker.open(host, {
        label: `Search placeholders to add to ${side}`,
        items: ui.pickerItems,
        sample: ui.sample,
        onPick: (name) => {
          picker = null;
          add(side, name);
        },
        onClose: (refocus) => {
          picker = null;
          if (refocus) pick.focus();
        },
      });
    };
    const sep = el("button", {
      type: "button",
      className: "btn small",
      textContent: "+ separator",
      title: "split segments here",
      onclick: () => {
        groups[side].push({ text: ` ${ui.separator()} ` });
        active[side] = groups[side].length - 1;
        commit(side, `sep:${side}`);
      },
    });
    sep.dataset.focus = `sep:${side}`;
    attrs(sep, { "aria-label": `add a separator to ${side}` });
    const text = el("button", {
      type: "button",
      className: "btn small",
      textContent: "+ text",
      onclick: () => {
        groups[side].push({ text: ":" });
        const i = groups[side].length - 1;
        active[side] = i;
        open = { side, i };
        commit(side, `edit:${side}:${i}`);
      },
    });
    attrs(text, { "aria-label": `add text to ${side}` });
    return el("div", { className: "adders" }, el("div", { className: "row wrap" }, pick, sep, text), host);
  }

  // Option keys placeholder `name` takes, the generic ones included.
  function optionKeys(name) {
    return ui.options(name).map((o) => o.key).concat(["icon", "bg"]);
  }

  return { init, update, optionKeys };
})();

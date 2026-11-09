// Placeholder picker: a search box over core placeholders and plugins,
// each with its one-line description and a sample of its output.
"use strict";

const Picker = (() => {
  const NAME = /^[A-Za-z0-9_.-]+$/;
  let seq = 0;

  const el = (tag, props = {}, ...kids) => {
    const n = Object.assign(document.createElement(tag), props);
    for (const k of kids) if (k != null) n.append(k);
    return n;
  };

  // Ranks `items` for `query`: name prefix, then name, then description.
  function filter(items, query) {
    const q = query.trim().toLowerCase();
    if (!q) return items;
    const scored = [];
    for (const it of items) {
      const name = it.name.toLowerCase();
      const text = `${it.description || ""} ${it.category || ""}`.toLowerCase();
      const score = name.startsWith(q) ? 0 : name.includes(q) ? 1 : text.includes(q) ? 2 : -1;
      if (score >= 0) scored.push([score, it]);
    }
    return scored.sort((a, b) => a[0] - b[0]).map(([, it]) => it);
  }

  // Opens under `host`. opts: {items: () => [{name, description, category,
  // plugin}], sample: (name) => html, onPick(name), onClose(), label}.
  function open(host, opts) {
    const id = `picker-${++seq}`;
    const input = el("input", {
      type: "search",
      className: "picker-search",
      placeholder: "Search placeholders",
      autocomplete: "off",
      spellcheck: false,
    });
    input.setAttribute("role", "combobox");
    input.setAttribute("aria-expanded", "true");
    input.setAttribute("aria-controls", id);
    input.setAttribute("aria-autocomplete", "list");
    input.setAttribute("aria-label", opts.label || "Search placeholders");
    const list = el("ul", { id, className: "picker-list" });
    list.setAttribute("role", "listbox");
    list.setAttribute("aria-label", "Placeholders");
    const hint = el("p", { className: "picker-hint", textContent: "↑↓ to choose, Enter to add, Esc to close" });
    const box = el("div", { className: "picker" }, input, list, hint);
    let shown = [];
    let at = 0;

    function row(it, i) {
      const li = el("li", { id: `${id}-${i}`, className: "picker-item" });
      li.setAttribute("role", "option");
      const head = el("span", { className: "picker-name", textContent: it.name });
      if (it.plugin) head.append(el("span", { className: "badge", textContent: it.category || "plugin" }));
      li.append(head);
      if (it.description) li.append(el("span", { className: "picker-desc", textContent: it.description }));
      if (!it.custom) {
        const sample = el("span", { className: "picker-sample term" });
        sample.innerHTML = opts.sample(it.name) || '<span class="empty-sample">(nothing with the sample data)</span>';
        li.append(sample);
      }
      li.onmousedown = (e) => e.preventDefault();
      li.onclick = () => pick(it);
      return li;
    }

    function draw() {
      const q = input.value.trim();
      shown = filter(opts.items(), q);
      if (q && NAME.test(q) && !shown.some((it) => it.name === q)) {
        shown.push({ name: q, description: `plugin bar-${q} on your PATH`, custom: true, plugin: true });
      }
      at = Math.min(at, Math.max(0, shown.length - 1));
      list.replaceChildren(...shown.map(row));
      if (!shown.length) list.append(el("li", { className: "picker-none", textContent: "No match. Plugin names use letters, digits, . _ -" }));
      mark();
    }

    function mark() {
      [...list.children].forEach((li, i) => li.setAttribute("aria-selected", String(i === at)));
      const cur = list.children[at];
      if (cur && shown.length) {
        input.setAttribute("aria-activedescendant", cur.id);
        cur.scrollIntoView({ block: "nearest" });
      } else input.removeAttribute("aria-activedescendant");
    }

    function close(refocus = true) {
      document.removeEventListener("mousedown", outside, true);
      box.remove();
      opts.onClose(refocus);
    }

    function pick(it) {
      document.removeEventListener("mousedown", outside, true);
      box.remove();
      opts.onPick(it.name);
    }

    function outside(e) {
      if (!box.contains(e.target)) close(false);
    }

    input.oninput = () => {
      at = 0;
      draw();
    };
    input.onkeydown = (e) => {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        if (!shown.length) return;
        at = (at + (e.key === "ArrowDown" ? 1 : -1) + shown.length) % shown.length;
        mark();
      } else if (e.key === "Enter") {
        e.preventDefault();
        if (shown[at]) pick(shown[at]);
      } else if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        close();
      } else if (e.key === "Tab") {
        close(false);
      }
    };
    host.append(box);
    draw();
    input.focus();
    document.addEventListener("mousedown", outside, true);
    return { close };
  }

  return { open, filter };
})();

// Page wiring: bar.conf text is the state; every change re-renders the
// preview with bar's renderer (web/pkg, built by `make web`).
"use strict";

(async () => {
  const $ = (id) => document.getElementById(id);
  if (typeof wasm_bindgen === "undefined" || typeof BAR_WASM_BASE64 === "undefined") {
    $("missing").hidden = false;
    return;
  }
  const bytes = Uint8Array.from(atob(BAR_WASM_BASE64), (c) => c.charCodeAt(0));
  await wasm_bindgen({ module_or_path: bytes });
  const bar = wasm_bindgen;
  const schema = JSON.parse(bar.schema());
  const STORE = "bar-configurator";
  // first visit: icons, a skin, and it fits 120 columns
  const FIRST_PRESET = "powerline";

  // A sample statusLine payload; only fields bar reads, resets relative to now.
  function sampleSession() {
    const now = Math.floor(Date.now() / 1000);
    return {
      session_id: "3b8f2c1e-0000-4000-8000-000000000000",
      cwd: "/home/you/src/bar",
      workspace: { current_dir: "/home/you/src/bar", project_dir: "/home/you/src/bar" },
      model: { id: "claude-opus-4-7", display_name: "Claude Opus 4.7" },
      version: "2.1.0",
      output_style: { name: "Explanatory" },
      effort: { level: "high" },
      thinking: { enabled: true },
      fast_mode: false,
      vim: { mode: "NORMAL" },
      agent: { name: "reviewer" },
      cost: { total_cost_usd: 1.84, total_duration_ms: 1620000, total_lines_added: 120, total_lines_removed: 14 },
      context_window: { used_percentage: 38 },
      rate_limits: {
        five_hour: { used_percentage: 10, resets_at: now + 4 * 3600 + 40 * 60 },
        seven_day: { used_percentage: 12, resets_at: now + 6 * 86400 },
      },
    };
  }

  const ui = {
    conf: $("conf"),
    session: $("session"),
    term: $("term"),
    columns: $("columns"),
    issues: $("issues"),
    status: $("status"),
  };

  let text = "";
  let lastInspect = null;
  // last text with no errors, for "Revert to last valid"
  let lastValid = null;
  let history = History.create();

  // Plugins: the bundled manifest (reference plugins), then the user's
  // plugins.json over it, by name.
  const bundled = typeof BAR_PLUGINS === "object" && BAR_PLUGINS ? BAR_PLUGINS.plugins || [] : [];
  let userPlugins = [];
  let manifest = "";
  let pluginList = [];
  function setPlugins(list) {
    userPlugins = list;
    const byName = new Map();
    for (const p of bundled.concat(userPlugins)) if (p && typeof p.name === "string") byName.set(p.name, p);
    manifest = JSON.stringify({ plugins: [...byName.values()] });
    pluginList = JSON.parse(bar.plugins(manifest));
    $("plugins-status").textContent = `${pluginList.length} plugin${pluginList.length === 1 ? "" : "s"}` +
      (userPlugins.length ? ` (${userPlugins.length} from your plugins.json)` : "");
  }

  function store() {
    try {
      localStorage.setItem(
        STORE,
        JSON.stringify({ text, lastValid, history: history.save(), columns: ui.columns.value, plugins: userPlugins }),
      );
    } catch (_) {
      /* storage unavailable: nothing kept between visits */
    }
  }

  function restore() {
    try {
      return JSON.parse(localStorage.getItem(STORE) || "null");
    } catch (_) {
      return null;
    }
  }

  function renderPreview() {
    const cols = parseInt(ui.columns.value, 10);
    $("columns-out").value = cols;
    const out = bar.render(text, ui.session.value, cols, Date.now() / 1000, manifest);
    const rows = Ansi.toHtml(out).map((h) => `<span class="row">${h || " "}</span>`);
    ui.term.innerHTML = rows.join("") + `<span class="cols" style="width:${cols}ch"></span>`;
  }

  function renderIssues() {
    const report = JSON.parse(bar.check(text, manifest));
    ui.issues.replaceChildren(
      ...report.issues.map((i) => {
        const li = document.createElement("li");
        li.className = i.level;
        li.textContent = `${i.line ? "line " + i.line + ": " : ""}${i.level}: ${i.message}`;
        return li;
      }),
    );
    const errors = report.issues.filter((i) => i.level === "error").length;
    ui.status.textContent = errors ? `${errors} error${errors > 1 ? "s" : ""}` : "valid";
    ui.status.classList.toggle("bad", errors > 0);
    if (!errors) lastValid = text;
    $("revert").hidden = !errors || lastValid === null || lastValid === text;
    return errors;
  }

  function syncControls(inspect) {
    if (!inspect.ok) return;
    for (const s of schema.settings) $("set-" + s.key).value = inspect.set[s.key];
    $("separator").value = inspect.set.separator;
    $("gap").value = inspect.set.gap;
  }

  // `from` is the control that made the change; it is not redrawn. Every
  // change but undo/redo itself is recorded; `group` folds typing runs.
  function setText(next, from, { group = null, record = true } = {}) {
    if (record && next !== text) history.record(text, group);
    text = next;
    if (from !== ui.conf) ui.conf.value = text;
    const inspect = JSON.parse(bar.inspect(text));
    lastInspect = inspect;
    syncControls(inspect);
    Compose.update(inspect.ok ? inspect : null);
    // controls write `set` lines: locked while the text does not parse
    $("look").disabled = !inspect.ok;
    renderIssues();
    renderPreview();
    markPreset();
    $("undo").disabled = !history.canUndo();
    $("redo").disabled = !history.canRedo();
    store();
  }

  function undo() {
    const prev = history.undo(text);
    if (prev !== null) setText(prev, null, { record: false });
  }
  function redo() {
    const next = history.redo(text);
    if (next !== null) setText(next, null, { record: false });
  }
  $("undo").onclick = undo;
  $("redo").onclick = redo;
  // Ctrl/Cmd+Z, Ctrl/Cmd+Shift+Z, Ctrl+Y: the page's history, bar.conf
  // typing included; the session JSON keeps the browser's own.
  document.addEventListener("keydown", (e) => {
    if (!(e.ctrlKey || e.metaKey) || e.altKey || e.target === ui.session) return;
    const k = e.key.toLowerCase();
    if (k === "z" || k === "y") {
      e.preventDefault();
      if (k === "y" || e.shiftKey) redo();
      else undo();
    }
  });
  $("revert").onclick = () => {
    if (lastValid !== null) setText(lastValid);
  };

  // Look controls
  const settingsBox = $("settings");
  for (const s of schema.settings) {
    const sel = document.createElement("select");
    sel.id = "set-" + s.key;
    for (const v of s.values) sel.append(new Option(v, v));
    sel.onchange = () => setText(Conf.set(text, s.key, sel.value));
    const label = document.createElement("label");
    label.append(s.key, sel);
    settingsBox.append(label);
  }
  $("separator").onchange = (e) => setText(Conf.set(text, "separator", e.target.value || null));
  $("gap").onchange = (e) => setText(Conf.set(text, "gap", e.target.value === "" ? null : e.target.value));

  // Presets: one click applies, with an undo in the toast
  const presets = schema.presets.map((name) => ({ name, body: bar.preset(name) }));
  const presetButtons = presets.map((p) => {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "preset";
    b.dataset.focus = "preset:" + p.name;
    const name = document.createElement("span");
    name.className = "preset-name";
    name.textContent = p.name;
    const desc = document.createElement("span");
    desc.className = "preset-desc";
    // the preset's first comment line, without its "name:" prefix
    const first = p.body.split("\n")[0] || "";
    desc.textContent = first.startsWith("#") ? first.replace(/^#\s*([\w-]+:\s*)?/, "") : "";
    const thumb = document.createElement("span");
    thumb.className = "preset-thumb term";
    thumb.setAttribute("aria-hidden", "true");
    b.append(name, desc, thumb);
    b.onclick = () => applyPreset(p);
    $("preset-list").append(b);
    return { p, b, thumb };
  });

  function drawPresetThumbs() {
    for (const { p, thumb } of presetButtons) {
      const out = bar.render(p.body, ui.session.value, 90, Date.now() / 1000, manifest);
      thumb.innerHTML = Ansi.toHtml(out).map((h) => `<span class="row">${h || " "}</span>`).join("");
    }
  }

  function markPreset() {
    const cur = presets.find((p) => p.body === text);
    for (const { p, b } of presetButtons) b.setAttribute("aria-pressed", String(cur === p));
    $("preset-current").textContent = cur ? `current: ${cur.name}` : "current: custom";
  }

  function applyPreset(p) {
    if (p.body === text) return;
    setText(p.body);
    toast(`Applied the ${p.name} preset.`);
  }

  let toastTimer = 0;
  function toast(message) {
    $("toast-text").textContent = message;
    $("toast").hidden = false;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => ($("toast").hidden = true), 8000);
  }
  $("toast-undo").onclick = () => {
    $("toast").hidden = true;
    undo();
  };

  // Feedback beside the header buttons, read out by screen readers
  let sayTimer = 0;
  function say(message) {
    $("say").textContent = message;
    clearTimeout(sayTimer);
    sayTimer = setTimeout(() => ($("say").textContent = ""), 10000);
  }
  function showUseIt() {
    $("use-it").hidden = false;
  }
  $("use-it-close").onclick = () => {
    $("use-it").hidden = true;
    $("download").focus();
  };
  for (const btn of document.querySelectorAll(".copy-cmd")) {
    btn.onclick = async () => {
      const code = $(btn.dataset.copy).textContent;
      try {
        await navigator.clipboard.writeText(code);
        btn.textContent = "Copied";
      } catch (_) {
        getSelection().selectAllChildren($(btn.dataset.copy));
        btn.textContent = "Press Ctrl+C";
      }
      setTimeout(() => (btn.textContent = "Copy"), 1500);
    };
  }

  Compose.init({
    schema,
    groups: $("groups"),
    tabs: $("line-tabs"),
    off: $("compose-off"),
    tokens: (src) => JSON.parse(bar.tokens(src)),
    plugins: () => pluginList,
    options: (name) => {
      const core = schema.placeholders.find((p) => p.name === name);
      if (core) return core.options;
      const plugin = pluginList.find((p) => p.name === name);
      return plugin ? plugin.options : [];
    },
    separator: () => (lastInspect && lastInspect.ok ? lastInspect.set.separator : "│"),
    describe: (name) => {
      const core = schema.placeholders.find((p) => p.name === name);
      if (core) return core.description;
      const plugin = pluginList.find((p) => p.name === name);
      return plugin ? plugin.description : "";
    },
    pickerItems: () =>
      schema.placeholders
        .map((p) => ({ name: p.name, description: p.description }))
        .concat(pluginList.map((p) => ({ name: p.name, description: p.description, category: p.category, plugin: true }))),
    sample,
    announce: (msg) => {
      $("announce").textContent = msg;
    },
    onGroup: (side, line, src) => setText(Conf.setGroup(text, side, line, src)),
  });

  // One placeholder alone, as the picker's sample: HTML, or "" when it
  // draws nothing with the sample data. Cached until the inputs change.
  let samples = new Map();
  let samplesKey = "";
  function sample(name) {
    const icons = lastInspect && lastInspect.ok ? lastInspect.set.icons : "none";
    const key = `${icons}\u0000${ui.session.value}\u0000${manifest}`;
    if (key !== samplesKey) {
      samples = new Map();
      samplesKey = key;
    }
    if (!samples.has(name)) {
      const conf = `set icons ${icons}\nset left "#{${name}}"`;
      const out = bar.render(conf, ui.session.value, 200, Date.now() / 1000, manifest).split("\n")[0];
      const plain = out.replace(/\x1b\[[0-9;:]*[@-~]/g, "").replace(/\x1b\][^\x07\x1b]*(\x07|\x1b\\)/g, "");
      samples.set(name, plain.trim() ? Ansi.toHtml(out)[0] : "");
    }
    return samples.get(name);
  }

  // Source, session, columns
  ui.conf.oninput = () => setText(ui.conf.value, ui.conf, { group: "typing" });
  ui.conf.onblur = () => history.seal();
  ui.session.oninput = () => {
    renderPreview();
    drawPresetThumbs();
  };
  $("session-reset").onclick = () => {
    ui.session.value = JSON.stringify(sampleSession(), null, 2);
    renderPreview();
    drawPresetThumbs();
  };
  ui.columns.oninput = () => {
    renderPreview();
    store();
  };

  // The user's plugins.json (`bar web plugins -o plugins.json`)
  $("load-plugins").onclick = () => $("load-plugins-file").click();
  $("load-plugins-file").onchange = async (e) => {
    const f = e.target.files[0];
    e.target.value = "";
    if (!f) return;
    try {
      const v = JSON.parse(await f.text());
      const list = Array.isArray(v) ? v : v.plugins;
      if (!Array.isArray(list)) throw new Error("no plugins list");
      setPlugins(list);
      setText(text);
      drawPresetThumbs();
    } catch (err) {
      alert(`Not a plugins.json: ${err.message}`);
    }
  };

  // Load, copy, download
  $("load").onclick = () => $("load-file").click();
  $("load-file").onchange = async (e) => {
    const f = e.target.files[0];
    if (f) setText(await f.text());
    e.target.value = "";
  };
  $("copy").onclick = async () => {
    try {
      await navigator.clipboard.writeText(text);
      say("Copied bar.conf. Paste it into ~/.config/bar/bar.conf; steps below.");
    } catch (_) {
      ui.conf.select();
      say("Copy blocked by the browser: bar.conf is selected, press Ctrl+C or ⌘C.");
    }
    showUseIt();
  };
  $("download").onclick = () => {
    const url = URL.createObjectURL(new Blob([text], { type: "text/plain" }));
    const a = Object.assign(document.createElement("a"), { href: url, download: "bar.conf" });
    a.click();
    // revoked later: a revoke right after click() can cancel the download
    setTimeout(() => URL.revokeObjectURL(url), 30000);
    say("Downloaded bar.conf. Save it to ~/.config/bar/bar.conf; steps below.");
    showUseIt();
  };

  // Start: last session's config, else the full preset
  const saved = restore();
  setPlugins(saved && Array.isArray(saved.plugins) ? saved.plugins : []);
  if (saved && saved.history) history = History.create(saved.history);
  if (saved && typeof saved.lastValid === "string") lastValid = saved.lastValid;
  if (saved && saved.columns) ui.columns.value = saved.columns;
  ui.session.value = JSON.stringify(sampleSession(), null, 2);
  $("app").hidden = false;
  drawPresetThumbs();
  setText(saved && typeof saved.text === "string" ? saved.text : bar.preset(FIRST_PRESET), null, { record: false });
  // countdowns move: redraw once a minute
  setInterval(renderPreview, 60000);
})();

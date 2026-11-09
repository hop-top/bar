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

  const presetSel = $("preset");
  for (const p of schema.presets) presetSel.append(new Option(p, p));
  presetSel.value = "full";
  $("preset-apply").onclick = () => {
    const body = bar.preset(presetSel.value);
    if (body && (text.trim() === "" || confirm(`Replace bar.conf with the ${presetSel.value} preset?`))) setText(body);
  };

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
    onGroup: (side, line, src) => setText(Conf.setGroup(text, side, line, src)),
  });

  // Source, session, columns
  ui.conf.oninput = () => setText(ui.conf.value, ui.conf, { group: "typing" });
  ui.conf.onblur = () => history.seal();
  ui.session.oninput = renderPreview;
  $("session-reset").onclick = () => {
    ui.session.value = JSON.stringify(sampleSession(), null, 2);
    renderPreview();
  };
  ui.columns.oninput = () => {
    renderPreview();
    store();
  };

  // The user's plugins.json (`bar web plugins -o plugins.json`)
  $("load-plugins").onchange = async (e) => {
    const f = e.target.files[0];
    e.target.value = "";
    if (!f) return;
    try {
      const v = JSON.parse(await f.text());
      const list = Array.isArray(v) ? v : v.plugins;
      if (!Array.isArray(list)) throw new Error("no plugins list");
      setPlugins(list);
      setText(text);
    } catch (err) {
      alert(`Not a plugins.json: ${err.message}`);
    }
  };

  // Load, copy, download
  $("load").onchange = async (e) => {
    const f = e.target.files[0];
    if (f) setText(await f.text());
    e.target.value = "";
  };
  $("copy").onclick = async () => {
    try {
      await navigator.clipboard.writeText(text);
      $("copy").textContent = "Copied";
    } catch (_) {
      ui.conf.select();
      $("copy").textContent = "Press ⌘C / Ctrl+C";
    }
    setTimeout(() => ($("copy").textContent = "Copy"), 1500);
  };
  $("download").onclick = () => {
    const url = URL.createObjectURL(new Blob([text], { type: "text/plain" }));
    const a = Object.assign(document.createElement("a"), { href: url, download: "bar.conf" });
    a.click();
    // revoked later: a revoke right after click() can cancel the download
    setTimeout(() => URL.revokeObjectURL(url), 30000);
  };

  // Start: last session's config, else the full preset
  const saved = restore();
  setPlugins(saved && Array.isArray(saved.plugins) ? saved.plugins : []);
  if (saved && saved.history) history = History.create(saved.history);
  if (saved && typeof saved.lastValid === "string") lastValid = saved.lastValid;
  if (saved && saved.columns) ui.columns.value = saved.columns;
  ui.session.value = JSON.stringify(sampleSession(), null, 2);
  $("app").hidden = false;
  setText(saved && typeof saved.text === "string" ? saved.text : bar.preset("full"), null, { record: false });
  // countdowns move: redraw once a minute
  setInterval(renderPreview, 60000);
})();

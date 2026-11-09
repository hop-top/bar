// bar.conf text edits: the file stays the source of truth, controls only
// rewrite the `set` lines they own, so comments, rules, accounts, defaults
// and plugin lines survive every edit.
"use strict";

const Conf = (() => {
  const SET = /^\s*set\s+(\S+)/;

  // `set <key>` of a line, or null (comments, other directives).
  function setKey(line) {
    const t = line.trimStart();
    if (t.startsWith("#")) return null;
    const m = SET.exec(line);
    return m ? m[1] : null;
  }

  // bar.conf has no escape for `"`: values never carry one.
  const clean = (v) => String(v).replace(/"/g, "");

  function quote(v) {
    const s = clean(v);
    return /^[^\s#"]+$/.test(s) ? s : `"${s}"`;
  }

  // Sets `key` to `value` (null removes it). The first line setting any
  // of `keys` is rewritten, later ones dropped; with none, the line goes
  // after the last `set` line, else at the end.
  function set(text, key, value, keys = [key]) {
    const lines = text.split("\n");
    const out = [];
    let placed = false;
    let lastSet = -1;
    for (const line of lines) {
      const k = setKey(line);
      if (k !== null && keys.includes(k)) {
        if (!placed && value !== null) out.push(`set ${key} ${quote(value)}`);
        placed = true;
        continue;
      }
      if (k !== null) lastSet = out.length;
      out.push(line);
    }
    if (!placed && value !== null) {
      const line = `set ${key} ${quote(value)}`;
      if (lastSet >= 0) out.splice(lastSet + 1, 0, line);
      else {
        while (out.length && out[out.length - 1] === "") out.pop();
        out.push(line, "");
      }
    }
    return out.join("\n");
  }

  // The key of group `side` (left/center/right) on 1-based `line`, and
  // every spelling of it.
  function groupKeys(side, line) {
    return line === 1 ? [side, `${side}@1`] : [`${side}@${line}`];
  }

  function setGroup(text, side, line, src) {
    const keys = groupKeys(side, line);
    return set(text, keys[0], src === "" ? null : src, keys);
  }

  // Tokens (as `bar_web.tokens` returns them) back to layout source.
  function serialize(tokens) {
    return tokens
      .map((t) => {
        if ("text" in t) return clean(t.text);
        const opts = t.options
          .filter(([k]) => k !== "")
          .map(([k, v]) => (v === "" ? k : `${k}=${v}`))
          .join(",");
        return opts ? `#{${t.name}[${opts}]}` : `#{${t.name}}`;
      })
      .join("");
  }

  return { set, setGroup, serialize, quote };
})();

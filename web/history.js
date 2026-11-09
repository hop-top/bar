// Undo and redo over bar.conf texts. Each entry is a whole text; typing
// in a row (same `group`, within `MERGE_MS`) folds into one entry.
"use strict";

const History = (() => {
  const LIMIT = 100;
  const MERGE_MS = 1200;

  function create(initial = { past: [], future: [] }) {
    let past = Array.isArray(initial.past) ? initial.past.slice(-LIMIT) : [];
    let future = Array.isArray(initial.future) ? initial.future.slice(0, LIMIT) : [];
    let lastGroup = null;
    let lastAt = 0;

    return {
      // Records `before`, the text a change replaced. Same-group changes
      // close together keep the first `before` only.
      record(before, group = null) {
        const now = Date.now();
        const merge = group !== null && group === lastGroup && now - lastAt < MERGE_MS;
        lastGroup = group;
        lastAt = now;
        future = [];
        if (merge) return;
        if (past[past.length - 1] === before) return;
        past.push(before);
        if (past.length > LIMIT) past.shift();
      },
      // The text to go back to from `current`, or null.
      undo(current) {
        lastGroup = null;
        if (!past.length) return null;
        future.unshift(current);
        return past.pop();
      },
      redo(current) {
        lastGroup = null;
        if (!future.length) return null;
        past.push(current);
        return future.shift();
      },
      canUndo: () => past.length > 0,
      canRedo: () => future.length > 0,
      // Ends the current typing run: the next change is its own entry.
      seal() {
        lastGroup = null;
      },
      save: () => ({ past: past.slice(-30), future: future.slice(0, 30) }),
    };
  }

  return { create };
})();

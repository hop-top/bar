#!/usr/bin/env node
// Synthetic Claude Code transcript (JSONL) for status-line benchmarks.
// Usage: node gen-transcript.mjs <out.jsonl> [target_bytes=5242880] [end_epoch_s=now-120] [cwd] [session_id]
// Deterministic for a given end time (seeded PRNG). No real conversation data.
import { createWriteStream } from 'node:fs';

const out = process.argv[2];
if (!out) { console.error('usage: gen-transcript.mjs <out.jsonl> [bytes] [end_epoch_s] [cwd] [session_id]'); process.exit(2); }
const target = Number(process.argv[3] || 5 * 1024 * 1024);
const end = Number(process.argv[4] || Math.floor(Date.now() / 1000) - 120) * 1000;
const cwd = process.argv[5] || '/opt/slbench/work/repo';
const sid = process.argv[6] || '5b0c9a4e-2d1f-4c3a-9e8b-7f6a5d4c3b2a';
const model = 'claude-opus-5-5';

let seed = 0x9e3779b9;
const rnd = () => { seed ^= seed << 13; seed >>>= 0; seed ^= seed >>> 17; seed ^= seed << 5; seed >>>= 0; return seed / 4294967296; };
const int = (a, b) => a + Math.floor(rnd() * (b - a + 1));
const hex = (n) => { let s = ''; for (let i = 0; i < n; i++) s += int(0, 15).toString(16); return s; };
const uuid = () => `${hex(8)}-${hex(4)}-4${hex(3)}-a${hex(3)}-${hex(12)}`;
const words = 'the status line reads session data from stdin and renders model context git branch cost usage tokens while the agent edits files runs tests and reports results to the user with a short summary of changes'.split(' ');
const text = (n) => { const w = []; for (let i = 0; i < n; i++) w.push(words[int(0, words.length - 1)]); return w.join(' '); };

// Lay out ~N turns over the last 62 minutes ending at `end`.
const avgTurnBytes = 4250;
const turns = Math.max(1, Math.ceil(target / avgTurnBytes));
const span = 62 * 60 * 1000;
const start = end - span;
const ws = createWriteStream(out);
let bytes = 0, parent = null, ctx = 12000;
const write = (o) => { const l = JSON.stringify(o) + '\n'; bytes += Buffer.byteLength(l); ws.write(l); };
const base = (t) => ({ parentUuid: parent, isSidechain: false, userType: 'external', cwd, sessionId: sid, version: '2.1.319', gitBranch: 'main', timestamp: new Date(t).toISOString() });

for (let i = 0; i < turns && bytes < target; i++) {
  let t = start + Math.floor((span * i) / turns);
  const u = uuid();
  if (i % 4 === 0) {
    write({ ...base(t), type: 'user', message: { role: 'user', content: text(int(10, 60)) }, uuid: u });
  } else {
    write({ ...base(t), type: 'user', message: { role: 'user', content: [{ type: 'tool_result', tool_use_id: `toolu_${hex(24)}`, content: text(int(200, 500)) }] }, uuid: u });
  }
  parent = u;
  t += int(800, 4000);
  ctx = Math.min(180000, ctx + int(200, 900));
  const usage = {
    input_tokens: int(1, 12),
    cache_creation_input_tokens: int(200, 2500),
    cache_read_input_tokens: ctx,
    output_tokens: int(40, 1400),
    service_tier: 'standard',
  };
  const id = `msg_${hex(24)}`, req = `req_${hex(24)}`;
  // One API response written as 1-3 lines (thinking/text/tool_use blocks), same id + usage: tools must dedupe.
  const blocks = [];
  if (rnd() < 0.5) blocks.push({ type: 'thinking', thinking: text(int(30, 120)), signature: hex(64) });
  blocks.push({ type: 'text', text: text(int(20, 160)) });
  if (rnd() < 0.6) blocks.push({ type: 'tool_use', id: `toolu_${hex(24)}`, name: 'Bash', input: { command: 'cargo test --quiet', description: 'Run tests' } });
  for (const b of blocks) {
    const a = uuid();
    write({ ...base(t), type: 'assistant', requestId: req, uuid: a, message: { id, type: 'message', role: 'assistant', model, content: [b], stop_reason: b.type === 'tool_use' ? 'tool_use' : null, stop_sequence: null, usage } });
    parent = a;
  }
}
ws.end(() => console.error(`wrote ${bytes} bytes to ${out}`));

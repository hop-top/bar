#!/usr/bin/env node
// Runs the prebuilt bar binary from the platform package npm installed
// with this one (an optional dependency selected by os and cpu).
"use strict";

const { spawnSync } = require("child_process");
const path = require("path");

const platforms = {
  "darwin-arm64": "@hop-top/bar-darwin-arm64",
  "darwin-x64": "@hop-top/bar-darwin-x64",
  "linux-arm64": "@hop-top/bar-linux-arm64",
  "linux-x64": "@hop-top/bar-linux-x64",
  "win32-arm64": "@hop-top/bar-win32-arm64",
  "win32-x64": "@hop-top/bar-win32-x64",
};

function binary() {
  const key = `${process.platform}-${process.arch}`;
  const pkg = platforms[key];
  if (!pkg) {
    fail(
      `bar: no prebuilt binary for ${key}; supported: ${Object.keys(platforms).join(", ")}.\n` +
        "Build from source: https://github.com/hop-top/bar#install",
    );
  }
  let dir;
  try {
    dir = path.dirname(require.resolve(`${pkg}/package.json`));
  } catch {
    fail(
      `bar: ${pkg} is not installed. It is an optional dependency of @hop-top/bar;\n` +
        "reinstall without --omit=optional (or --no-optional) so npm adds it.",
    );
  }
  return path.join(dir, "bin", process.platform === "win32" ? "bar.exe" : "bar");
}

function fail(msg) {
  process.stderr.write(`${msg}\n`);
  process.exit(1);
}

const run = spawnSync(binary(), process.argv.slice(2), { stdio: "inherit", windowsHide: true });
if (run.error) {
  fail(`bar: ${run.error.message}`);
}
if (run.signal) {
  process.kill(process.pid, run.signal);
} else {
  process.exit(run.status === null ? 1 : run.status);
}

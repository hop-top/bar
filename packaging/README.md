# packaging

Package manager files for releases. The release workflow builds the archives; these turn them into packages. Details of each: [Releasing](../docs/technical.md#releasing).

- [`homebrew/bar.rb`](homebrew/bar.rb): Homebrew formula template for macOS and Linux, x86_64 and aarch64. It installs `bar`, `bar-cliproxyapi` and `bar-routellm`, plus `bar-tray` on macOS. `version` and the `SHA256_<TARGET>` values are placeholders, filled in after a release before the file is copied into a tap's `Formula/`.
- [`npm/`](npm/): the npm packages. [`npm/bar/`](npm/bar/) is `@hop-top/bar`, a Node launcher ([`bin/bar.js`](npm/bar/bin/bar.js)) whose [README](npm/bar/README.md) is the package page on npmjs.com; `npm/bar-<os>-<arch>/` are the six platform packages it lists as optional dependencies, which carry the binaries. The platform packages hold only a `package.json` here: the binaries are unpacked into each one's `bin/` at release time by [`scripts/npm-stage`](../scripts/npm-stage).

Versions in `npm/*/package.json` are set by release-please; do not edit them by hand. `make npm-check` (part of `make check`) stages the packages from fake archives, packs them and runs the launcher, offline; it skips without `node` and `npm`.

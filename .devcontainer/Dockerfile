# Dev container for bar: build, test and try bar against Claude Code.
# linux/arm64 and linux/amd64.
#
# Pins: base image by index digest; Debian packages from snapshot.debian.org
# at a fixed date; downloads by sha256; Rust by version through rustup.
# Every pinned release is at least 14 days older than the pin
# (docs/dependencies.md#dev-container). Claude Code is the exception: the
# latest release at build time, unless CLAUDE_CODE_VERSION names one.
FROM docker.io/library/python:3.14.8-slim-trixie@sha256:fbecd461fc72d21b2143c6eaba25143738079154a6d0ea3da3aec724f50eed4a

ARG TARGETARCH
ARG DEBIAN_SNAPSHOT=20261020T000000Z
ARG RUSTUP_VERSION=1.29.2
ARG RUSTUP_SHA256_ARM64=a9c168dbea81a84671c6b7a5c1795cb0eb699020110d107ddfca427bd797e697
ARG RUSTUP_SHA256_AMD64=5be191e8c70a5007074254cb74d78dbfdd25cd5a61c27de7b8e5410122ec24f4
ARG RUST_VERSION=1.99.1
ARG WASM_BINDGEN_VERSION=0.2.121
ARG WASM_BINDGEN_SHA256_ARM64=7253b3aa6e19980b15ea57876e0ed3a9996e192a8ba9fc4e2877318d5f0830e1
ARG WASM_BINDGEN_SHA256_AMD64=3039f38f65fe237b640cf06a140c919ca8d717ec5012146d145d3f27bb4d6b28
ARG GH_VERSION=2.103.2
ARG GH_SHA256_ARM64=4c3e99a50bd8b181a3fe9d69b25dd978bac768148d88f01507c6b4d1ef61a443
ARG GH_SHA256_AMD64=62ccf747115ddc61237efffdde66a3c8f70ba249433dc64f70801089b59c59c8
ARG JJ_VERSION=0.46.0
ARG JJ_SHA256_ARM64=80e28f7501c11e50e0ca06cd6a1fa1d982cff9cc59bfe4411a9d36aa501d867e
ARG JJ_SHA256_AMD64=fce0271158e665ceb82dc66c5eb95b1728649094e9133074f3b5baece345fc08
ARG USERNAME=dev
ARG USER_UID=1000

SHELL ["/bin/bash", "-euo", "pipefail", "-c"]

# Debian packages: C toolchain for cargo, git (reftable support needs 2.45 or
# later), binaryen (wasm-opt for `make web`), tools for the shell.
RUN rm -f /etc/apt/sources.list.d/* \
 && echo "deb [check-valid-until=no] http://snapshot.debian.org/archive/debian/${DEBIAN_SNAPSHOT} trixie main" > /etc/apt/sources.list \
 && echo "deb [check-valid-until=no] http://snapshot.debian.org/archive/debian-security/${DEBIAN_SNAPSHOT} trixie-security main" >> /etc/apt/sources.list \
 && apt-get -o Acquire::Retries=5 update \
 && apt-get install -y --no-install-recommends \
      ca-certificates curl xz-utils gcc libc6-dev make git binaryen jq less procps \
 && rm -rf /var/lib/apt/lists/*

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    CARGO_TARGET_DIR=/home/${USERNAME}/target \
    PATH=/home/${USERNAME}/.local/bin:/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

WORKDIR /tmp/dl

# Rust: the toolchain the bench image pins, with clippy, rustfmt and the
# wasm32 target web-check builds for; rustup checks each component's hash.
RUN case "$TARGETARCH" in \
      arm64) r=aarch64; sum=$RUSTUP_SHA256_ARM64;; \
      amd64) r=x86_64; sum=$RUSTUP_SHA256_AMD64;; \
      *) echo "unsupported architecture: $TARGETARCH" >&2; exit 1;; \
    esac \
 && curl -fsSLo rustup-init "https://static.rust-lang.org/rustup/archive/${RUSTUP_VERSION}/${r}-unknown-linux-gnu/rustup-init" \
 && echo "${sum}  rustup-init" | sha256sum -c - \
 && chmod +x rustup-init \
 && ./rustup-init -y -q --no-modify-path --profile minimal --default-toolchain "$RUST_VERSION" \
      --component clippy,rustfmt,rust-src --target wasm32-unknown-unknown \
 && rustc --version && rm -rf /tmp/dl/*

# wasm-bindgen-cli at the version web/wasm pins; gh; jj.
RUN case "$TARGETARCH" in \
      arm64) r=aarch64; g=arm64; wb=$WASM_BINDGEN_SHA256_ARM64; gs=$GH_SHA256_ARM64; js=$JJ_SHA256_ARM64;; \
      amd64) r=x86_64; g=amd64; wb=$WASM_BINDGEN_SHA256_AMD64; gs=$GH_SHA256_AMD64; js=$JJ_SHA256_AMD64;; \
    esac \
 && f="wasm-bindgen-${WASM_BINDGEN_VERSION}-${r}-unknown-linux-musl" \
 && curl -fsSLO "https://github.com/wasm-bindgen/wasm-bindgen/releases/download/${WASM_BINDGEN_VERSION}/${f}.tar.gz" \
 && echo "${wb}  ${f}.tar.gz" | sha256sum -c - \
 && tar -xzf "${f}.tar.gz" && install -m 0755 "${f}/wasm-bindgen" /usr/local/bin/wasm-bindgen \
 && f="gh_${GH_VERSION}_linux_${g}" \
 && curl -fsSLO "https://github.com/cli/cli/releases/download/v${GH_VERSION}/${f}.tar.gz" \
 && echo "${gs}  ${f}.tar.gz" | sha256sum -c - \
 && tar -xzf "${f}.tar.gz" && install -m 0755 "${f}/bin/gh" /usr/local/bin/gh \
 && f="jj-v${JJ_VERSION}-${r}-unknown-linux-musl.tar.gz" \
 && curl -fsSLO "https://github.com/jj-vcs/jj/releases/download/v${JJ_VERSION}/${f}" \
 && echo "${js}  ${f}" | sha256sum -c - \
 && mkdir jj && tar -xzf "$f" -C jj && install -m 0755 jj/jj /usr/local/bin/jj \
 && rm -rf /tmp/dl/*

# Non-root user. The dirs that named volumes mount over exist and belong to
# the user, so a fresh volume starts with that owner.
RUN useradd --create-home --uid "$USER_UID" --shell /bin/bash "$USERNAME" \
 && install -d -o "$USERNAME" -g "$USERNAME" \
      "$CARGO_HOME/registry" "$CARGO_HOME/git" "$CARGO_TARGET_DIR" \
      "/home/$USERNAME/.claude" "/home/$USERNAME/.config/bar" \
      "/home/$USERNAME/.local/state/bar" "/home/$USERNAME/.local/share/bar" \
      "/home/$USERNAME/.local/bin" \
 && chown -R "$USERNAME:$USERNAME" "$CARGO_HOME" "$RUSTUP_HOME" "/home/$USERNAME"

# Claude Code, last so the layers above stay cached: the native installer,
# into ~/.local/bin and ~/.local/share/claude. "latest" (default) or "stable"
# keeps auto-updates on; a version number turns them off in managed settings
# so the pin holds. Rebuild without cache to pick up a newer latest.
ARG CLAUDE_CODE_VERSION=latest
RUN case "$CLAUDE_CODE_VERSION" in \
      latest|stable) ;; \
      *) install -d /etc/claude-code \
         && echo '{"env":{"DISABLE_AUTOUPDATER":"1"}}' > /etc/claude-code/managed-settings.json ;; \
    esac
USER ${USERNAME}
ENV CLAUDE_CONFIG_DIR=/home/${USERNAME}/.claude
RUN curl -fsSL https://claude.ai/install.sh | bash -s "$CLAUDE_CODE_VERSION" \
 && claude --version

WORKDIR /workspaces/bar

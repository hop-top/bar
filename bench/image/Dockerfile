# Benchmark image for `make bench`: toolchain, the three other status-line
# tools and ben, all pinned. linux/arm64 and linux/amd64.
#
# Pins: base images by index digest; downloads by sha256 (npm tarballs by
# their registry sha512 integrity); ben by commit. Every pinned release is at
# least 14 days older than the pin (docs/dependencies.md#benchmark-image).
# Debian packages come from snapshot.debian.org at a fixed date.

# ---- ben (Go benchmark runner), built from a pinned commit ----------------
FROM docker.io/library/golang:1.27.2-trixie@sha256:47667934874913a449b96faa7ef4777fa285d9ec29126ee80ed61c45b7cccf0e AS ben
ARG BEN_COMMIT=2b24904f43489450f378df106e2cc96da013f640
RUN git init -q /src/ben \
 && git -C /src/ben fetch -q --depth 1 https://github.com/hop-top/ben.git "$BEN_COMMIT" \
 && git -C /src/ben checkout -q FETCH_HEAD \
 && test "$(git -C /src/ben rev-parse HEAD)" = "$BEN_COMMIT"
WORKDIR /src/ben
# ben depends on hop.top/uri, a private module: the build reads a GitHub token
# from the optional `github_token` secret (scripts/bench passes one). The token
# is used through environment variables of this step only; no layer keeps it.
RUN --mount=type=secret,id=github_token,required=false \
    t=$(cat /run/secrets/github_token 2>/dev/null || true); \
    if [ -n "$t" ]; then \
      export GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0="url.https://x-access-token:${t}@github.com/hop-top/.insteadOf" \
             GIT_CONFIG_VALUE_0="https://github.com/hop-top/"; \
    fi; \
    GOPRIVATE=hop.top/uri GOWORK=off CGO_ENABLED=0 GOFLAGS=-mod=readonly go build -trimpath -o /out/ben ./cmd/ben

# ---- benchmark environment ------------------------------------------------
FROM docker.io/library/python:3.14.8-slim-trixie@sha256:fbecd461fc72d21b2143c6eaba25143738079154a6d0ea3da3aec724f50eed4a
ARG TARGETARCH
ARG DEBIAN_SNAPSHOT=20261020T000000Z
ARG NODE_VERSION=26.12.0
ARG NODE_SHA256_ARM64=fd6cc729fa4448c2ac27c3fca5606019ced63c65a15c152c8df03e0cbce7f4c5
ARG NODE_SHA256_AMD64=fd47865ecd6391ebb431fc815e01b9135bf83ded5cf373247e9331e196b7aded
ARG NPM_VERSION=12.2.1
ARG NPM_SHA512=bKCmFRVT/BB+YPEYbZwQ1XUI3N5t4xKSpU1MxOjo7z3iWzKBJ0ZXRC1xFdEN+TC7N5TdywaBzf2dMXGJq/e2PQ==
ARG BUN_VERSION=1.4.4
ARG BUN_SHA256_ARM64=baa6bb36f51d6e58561e02bbea825038a9df454f673d6d776e8bbf86f8c7850e
ARG BUN_SHA256_AMD64=bf8ccbfb2b0b80159e0f9a6d15c55317add8077534dbc6130a3edb52fccb395c
ARG RUST_VERSION=1.99.1
ARG RUST_SHA256_ARM64=a48fb1e853be56ce9b8e2751f5464dbd5e16440cd81e237920f3a5e7e806333e
ARG RUST_SHA256_AMD64=231fcae47ee950621374725b5fc3b5441d2fbda2955ac45dd7dc816d8876cfe7
ARG GIT_VERSION=2.56.0
ARG GIT_SHA256=26c56c296b38c0695b26fa95f475f1d01704d2d38e73465ca30b0b2f5dc789d3
ARG TIME_VERSION=1.10
ARG TIME_SHA256=e8c29fb4ab599d8478e41e8618f50db8aede9c90af27d0d2ef28ae50d5de09c3
ARG HYPERFINE_VERSION=2.0.0
ARG HYPERFINE_SHA256_ARM64=f8244d1e17f8da23ee6b3ae2cdc04a6a185a450f6b74e09bff12e498038fcbc9
ARG HYPERFINE_SHA256_AMD64=ae2beda2ac99c098427e4f755244552daf20e514422889cca6d44421e2d93c87
ARG CCSTATUSLINE_VERSION=2.2.32
ARG CCSTATUSLINE_SHA512=JqlfYK7P+1kgh6bs7ailGwDT5c6qprfRvoJQnmbfxY+VYnAiR6dVxIYhrwsR6u3LvhB7Laio5yb6g3DxKKAndg==
ARG POWERLINE_VERSION=1.32.5
ARG POWERLINE_SHA512=ZlRL9+KfeLS24FTW1ZrlhJaE1qK/mW/1w2V8Z2qgN0lvm8X8D40ulxa6RMl1KRhrXUTzNnuf0TkDc6ax08WXRA==
ARG CCLINE_VERSION=2.0.1
ARG CCLINE_SHA256_ARM64=7d11331cf73db9d1a0017721601bdc27466120fd964531f117f797fe2c90fcc6
ARG CCLINE_SHA256_AMD64=9d21bd6e93a1d74ead75a59e428f05266b4b472eb2bce5bcd92422d6cb205985
ARG BEN_COMMIT=2b24904f43489450f378df106e2cc96da013f640

SHELL ["/bin/bash", "-euo", "pipefail", "-c"]

# Debian packages: build tools for git, GNU time and bar; jq and procps for run.sh.
RUN rm -f /etc/apt/sources.list.d/* \
 && echo "deb [check-valid-until=no] http://snapshot.debian.org/archive/debian/${DEBIAN_SNAPSHOT} trixie main" > /etc/apt/sources.list \
 && echo "deb [check-valid-until=no] http://snapshot.debian.org/archive/debian-security/${DEBIAN_SNAPSHOT} trixie-security main" >> /etc/apt/sources.list \
 && apt-get -o Acquire::Retries=5 update \
 && apt-get install -y --no-install-recommends ca-certificates curl xz-utils unzip jq procps gcc libc6-dev make zlib1g-dev \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /tmp/dl
COPY helpers.sh /usr/local/lib/bench-helpers.sh

# GNU time (run.sh calls /usr/bin/time) and git, from source.
RUN curl -fsSLO "https://ftp.gnu.org/gnu/time/time-${TIME_VERSION}.tar.gz" \
 && echo "${TIME_SHA256}  time-${TIME_VERSION}.tar.gz" | sha256sum -c - \
 && tar -xzf "time-${TIME_VERSION}.tar.gz" && cd "time-${TIME_VERSION}" \
 && ./configure --prefix=/usr -q && make -s -j"$(nproc)" && make -s install \
 && cd /tmp/dl \
 && curl -fsSLO "https://mirrors.edge.kernel.org/pub/software/scm/git/git-${GIT_VERSION}.tar.xz" \
 && echo "${GIT_SHA256}  git-${GIT_VERSION}.tar.xz" | sha256sum -c - \
 && tar -xJf "git-${GIT_VERSION}.tar.xz" && cd "git-${GIT_VERSION}" \
 && make -s -j"$(nproc)" prefix=/usr/local NO_CURL=1 NO_EXPAT=1 NO_OPENSSL=1 NO_GETTEXT=1 NO_TCLTK=1 NO_PERL=1 NO_PYTHON=1 NO_ICONV=1 NO_RUST=1 install >/dev/null \
 && rm -rf /tmp/dl/*

# Node and npm
RUN . /usr/local/lib/bench-helpers.sh \
 && case "$TARGETARCH" in arm64) a=arm64;; amd64) a=x64;; esac \
 && f="node-v${NODE_VERSION}-linux-${a}.tar.xz" \
 && curl -fsSLO "https://nodejs.org/dist/v${NODE_VERSION}/${f}" \
 && echo "$(pin NODE_SHA256)  ${f}" | sha256sum -c - \
 && tar -xJf "$f" -C /usr/local --strip-components=1 --exclude='*/CHANGELOG.md' --exclude='*/README.md' \
 && curl -fsSLo npm.tgz "https://registry.npmjs.org/npm/-/npm-${NPM_VERSION}.tgz" \
 && sri_check npm.tgz "$NPM_SHA512" \
 && npm install -g --ignore-scripts --no-audit --no-fund ./npm.tgz \
 && rm -rf /tmp/dl/* /root/.npm

# Bun, Rust (rustc, cargo, std), hyperfine
RUN . /usr/local/lib/bench-helpers.sh \
 && case "$TARGETARCH" in arm64) b=aarch64; r=aarch64;; amd64) b=x64; r=x86_64;; esac \
 && curl -fsSLo bun.zip "https://github.com/oven-sh/bun/releases/download/bun-v${BUN_VERSION}/bun-linux-${b}.zip" \
 && echo "$(pin BUN_SHA256)  bun.zip" | sha256sum -c - \
 && unzip -q bun.zip && install -D -m 0755 "bun-linux-${b}/bun" /opt/slbench/bun/bin/bun \
 && ln -s bun /opt/slbench/bun/bin/bunx \
 && f="rust-${RUST_VERSION}-${r}-unknown-linux-gnu" \
 && curl -fsSLO "https://static.rust-lang.org/dist/${f}.tar.xz" \
 && echo "$(pin RUST_SHA256)  ${f}.tar.xz" | sha256sum -c - \
 && tar -xJf "${f}.tar.xz" \
 && "./${f}/install.sh" --prefix=/usr/local --components="rustc,cargo,rust-std-${r}-unknown-linux-gnu" --disable-ldconfig >/dev/null \
 && f="hyperfine-v${HYPERFINE_VERSION}-${r}-unknown-linux-gnu" \
 && curl -fsSLO "https://github.com/sharkdp/hyperfine/releases/download/v${HYPERFINE_VERSION}/${f}.tar.gz" \
 && echo "$(pin HYPERFINE_SHA256)  ${f}.tar.gz" | sha256sum -c - \
 && tar -xzf "${f}.tar.gz" && install -m 0755 "${f}/hyperfine" /usr/local/bin/hyperfine \
 && rm -rf /tmp/dl/*

# The other status-line tools: ccstatusline and claude-powerline (npm, no
# dependencies, no install scripts), ccline (release binary).
RUN . /usr/local/lib/bench-helpers.sh \
 && curl -fsSLo ccstatusline.tgz "https://registry.npmjs.org/ccstatusline/-/ccstatusline-${CCSTATUSLINE_VERSION}.tgz" \
 && sri_check ccstatusline.tgz "$CCSTATUSLINE_SHA512" \
 && curl -fsSLo powerline.tgz "https://registry.npmjs.org/@owloops/claude-powerline/-/claude-powerline-${POWERLINE_VERSION}.tgz" \
 && sri_check powerline.tgz "$POWERLINE_SHA512" \
 && npm install -g --ignore-scripts --no-audit --no-fund ./ccstatusline.tgz ./powerline.tgz \
 && case "$TARGETARCH" in arm64) c=arm64;; amd64) c=x64;; esac \
 && curl -fsSLo ccline.tar.gz "https://github.com/Haleclipse/CCometixLine/releases/download/v${CCLINE_VERSION}/ccline-linux-${c}.tar.gz" \
 && echo "$(pin CCLINE_SHA256)  ccline.tar.gz" | sha256sum -c - \
 && mkdir -p /opt/slbench/ccline/dl && tar -xzf ccline.tar.gz -C /opt/slbench/ccline/dl \
 && test -x /opt/slbench/ccline/dl/ccline \
 && rm -rf /tmp/dl/* /root/.npm

COPY --from=ben /out/ben /opt/slbench/bin/ben
COPY entry.sh /usr/local/bin/bench-entry

# Recorded in the results (bench/results/ben-meta.json).
ENV BENCH_BASE_IMAGE="python:3.14.8-slim-trixie@sha256:fbecd461fc72d21b2143c6eaba25143738079154a6d0ea3da3aec724f50eed4a" \
    BENCH_DEBIAN_SNAPSHOT=${DEBIAN_SNAPSHOT} \
    BENCH_CCSTATUSLINE=${CCSTATUSLINE_VERSION} \
    BENCH_POWERLINE=${POWERLINE_VERSION} \
    BENCH_CCLINE=${CCLINE_VERSION} \
    BENCH_BEN_COMMIT=${BEN_COMMIT} \
    CARGO_HOME=/opt/slbench/cargo \
    PATH=/opt/slbench/bun/bin:/usr/local/bin:/usr/bin:/bin \
    COLUMNS=200

WORKDIR /opt/slbench
ENTRYPOINT ["/usr/local/bin/bench-entry"]

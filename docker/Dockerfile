# Base image for local coding agents, and the localfiles toolchain on top.
# Fedora.
#
#   docker build --target base -t localfiles/agent-base .
#   docker build -t localfiles/agent .
#   docker run --rm -it -v "$PWD":/work -w /work localfiles/agent
#
# Multi-arch: amd64 and arm64, so it builds and runs on Apple silicon as well
# as a Linux box.
#
# Do not COPY a repo into this image. Mount it. The image is a toolchain, not
# a snapshot of any one project — a COPYed tree goes stale and silently
# shadows the checkout you meant to work on.
#
# ---------------------------------------------------------------------------
# Provenance rule: dnf first.
#
# Everything that Fedora packages comes from Fedora. That is the whole tool
# set except two agent CLIs. The distro has already done the work — building
# for both arches, tracking CVEs, keeping the runtime deps coherent — and a
# missing package is a name to fix, where a 404 from a hand-pinned tarball URL
# is a URL to go and re-derive.
#
# dnf is NOT allowed to fail the build over a name, though: both install lines
# pass --skip-unavailable, and the assertion layers are the only thing that
# decides required from nice-to-have. A raw "No match for argument" kills the
# build three lines before the comment that would have explained it.
#
# The cost is that a Fedora release moves within itself: `ripgrep` in
# fedora:44 today and in six months are not the same build. If you need the
# image to be bit-reproducible, pin the base by digest
# (`FROM fedora:44@sha256:…`) and add explicit NVRs to the dnf lines
# (`ripgrep-14.1.1-3.fc44`). Pinning the base alone gets you most of it.
#
# Fallbacks, in order, when something is not packaged:
#   1. run-time install into a persistent volume — `cargo install` or
#      `go install` into ~/.cache, which costs the image nothing and is where
#      zellij and lazygit live, neither being in Fedora
#   2. the language's own registry at build time (npm, and it is pinned)
#   3. a pinned tarball verified with `fetch-verify` (see below)
#   4. a vendor install script — INSTALL_CURSOR is the only one, and it is
#      the only unpinned step in the image
# ---------------------------------------------------------------------------

FROM fedora:44 AS base

SHELL ["/bin/bash", "-o", "pipefail", "-c"]

# ---------------------------------------------------------------------------
# The tool set, from Fedora, in two groups.
#
# The split is the lesson from the first build of this file: `zellij` is not
# in Fedora 44, dnf failed the transaction on the name, and the build died at
# dnf with a raw "No match for argument" instead of reaching the assertion
# layer below that exists to explain exactly that. So dnf is no longer allowed
# to decide what is fatal. Both groups install with --skip-unavailable, and
# the assertion layer is the single place that decides required vs nice.
#
# REQUIRED: the build fails, by tool name, if any of these is missing.
# OPTIONAL: skipped with a printed list. An agent works without them.
#
# --allowerasing: fedora:44 ships full curl, but the -minimal variants ship
# `curl-minimal`, which conflicts. Without the flag dnf fails the transaction
# instead of swapping the two.
#
# install_weak_deps=False keeps recommends out — in a container they pull a
# surprising amount of desktop plumbing.
#
# tar, gzip, xz, findutils and shadow-utils are already in fedora:44 and are
# listed anyway: they are not guaranteed across tags or the -minimal variants,
# and every one is load-bearing below. dnf reports them as already installed,
# which --skip-unavailable makes harmless rather than fatal.
# ---------------------------------------------------------------------------
RUN dnf install -y --allowerasing --skip-unavailable \
        --setopt=install_weak_deps=False \
        ca-certificates \
        curl \
        git \
        git-lfs \
        gh \
        openssh-clients \
        gcc \
        gcc-c++ \
        make \
        pkgconf-pkg-config \
        python3 \
        python3-pip \
        nodejs \
        npm \
        ripgrep \
        fd-find \
        jq \
        diffutils \
        patch \
        less \
        tmux \
        glibc-langpack-en \
        tzdata \
        tar \
        gzip \
        xz \
        unzip \
        procps-ng \
        findutils \
        shadow-utils \
    && dnf clean all && rm -rf /var/cache/dnf

# Convenience tools. Not packaged in every Fedora, and nothing breaks without
# them. EXTRA_PACKAGES lets you add whatever your Fedora does have without
# editing this file: --build-arg EXTRA_PACKAGES="tig bat fzf".
ARG EXTRA_PACKAGES=""
RUN dnf install -y --skip-unavailable --setopt=install_weak_deps=False \
        helix \
        eza \
        btop \
        ${EXTRA_PACKAGES} \
    && dnf clean all && rm -rf /var/cache/dnf

# ---------------------------------------------------------------------------
# Assert the tool set, by binary name, in one layer.
#
# This is the layer that makes "dnf first" safe to maintain, and it is now the
# only thing that can fail the build over a package name. A package renamed,
# retired, or absent from a future Fedora otherwise produces an image that
# builds fine and is missing ripgrep — discovered by an agent, at work, as a
# confusing failure. Here it stops the build and names the tool.
#
# fd is the known offender: Fedora's fd-find installs /usr/bin/fd, Debian
# renames it to fdfind. Symlinked if that ever changes here.
#
# The node check is the same idea one level down: pi requires node >= 22.19,
# Fedora's default is well past it, and asserting that here beats discovering
# it as an npm resolution error three layers later.
# ---------------------------------------------------------------------------
RUN set -euo pipefail; \
    [ -x /usr/bin/fd ] || { [ -x /usr/bin/fdfind ] && ln -s /usr/bin/fdfind /usr/local/bin/fd; }; \
    missing=(); \
    for t in git git-lfs gh ssh gcc make python3 node npm rg fd jq diff patch less tmux; do \
      command -v "$t" >/dev/null || missing+=("$t"); \
    done; \
    if [ ${#missing[@]} -gt 0 ]; then \
      echo "FATAL: Fedora did not provide: ${missing[*]}" >&2; \
      echo "       The package was renamed or dropped in this Fedora release." >&2; \
      echo "       Fix the REQUIRED dnf line above, or fall back to a pinned" >&2; \
      echo "       tarball via fetch-verify (see the provenance rule at top)." >&2; \
      exit 1; \
    fi; \
    absent=(); \
    for t in hx eza btop; do command -v "$t" >/dev/null || absent+=("$t"); done; \
    if [ ${#absent[@]} -gt 0 ]; then \
      echo "note: optional tools not packaged in this Fedora: ${absent[*]}" >&2; \
    fi; \
    node -e 'const [a,b]=process.versions.node.split(".").map(Number); if (a<22||(a===22&&b<19)) { console.error("FATAL: node "+process.versions.node+" < 22.19 (pi requires it); install nodejs22 or newer explicitly"); process.exit(1) }'

# ---------------------------------------------------------------------------
# Not packaged in Fedora, deliberately not installed here: zellij, lazygit.
#
# Neither is in the Fedora repositories (verified the hard way — the first
# build of this file died on `zellij`). Rather than guess at a COPR name this
# file cannot verify, install them at run time into the persistent volumes,
# where they survive restarts and cost the image nothing:
#
#   cargo install zellij                                  -> ~/.cargo/bin
#   go install github.com/jesseduffield/lazygit@latest    -> ~/go/bin
#
# Both toolchains are present in the `localfiles` stage and both target
# directories are volumes. tmux is installed above as the packaged
# multiplexer, so the container is not without one meanwhile.
# ---------------------------------------------------------------------------

# TUIs (tmux, helix, btop) render boxes and glyphs as mojibake without a
# UTF-8 locale, and the failure looks like a font problem rather than a
# locale one. glibc-langpack-en above is what makes en_US.UTF-8 real; Fedora
# has no locale-gen step.
ENV LANG=en_US.UTF-8 \
    LC_ALL=en_US.UTF-8 \
    TERM=xterm-256color

# ---------------------------------------------------------------------------
# fetch-verify: download to a path and fail unless the sha256 matches.
#
# Nothing in the image uses it today — dnf covers the tool set. It is here as
# the sanctioned fallback (rule 2 above) so that adding a tarball-installed
# tool cannot quietly skip the digest check. Usage:
#
#   fetch-verify <url> <sha256> <dest>
# ---------------------------------------------------------------------------
RUN printf '%s\n' \
    '#!/bin/bash' \
    'set -euo pipefail' \
    'url="$1"; want="$2"; dest="$3"' \
    'curl -fsSL --retry 3 --retry-delay 2 --max-time 600 -o "$dest" "$url"' \
    'echo "${want}  ${dest}" | sha256sum -c - >/dev/null' \
    'echo "verified $(basename "$dest")"' \
    > /usr/local/bin/fetch-verify && chmod +x /usr/local/bin/fetch-verify

# ---------------------------------------------------------------------------
# Non-root user. UID/GID are build args so that files written into a mounted
# /work land owned by you on the host, not by an arbitrary 1000.
# ---------------------------------------------------------------------------
ARG USERNAME=agent
ARG UID=1000
ARG GID=1000
RUN set -euo pipefail; \
    if ! getent group "$GID" >/dev/null; then groupadd -g "$GID" "$USERNAME"; fi; \
    useradd -m -u "$UID" -g "$GID" -s /bin/bash "$USERNAME"

# /work is a bind mount whose ownership comes from the host, and git refuses
# to operate on a repo it thinks belongs to someone else. Every repo here is
# one you mounted on purpose.
#
# --system, not --global: the global config now lives in the .config volume
# (GIT_CONFIG_GLOBAL below), and a volume shadows whatever the image wrote
# there. A rule the image must always carry belongs in /etc/gitconfig, where
# no mount can hide it.
RUN git config --system --add safe.directory '*'

# ssh client policy at image level for the same reason: ~/.ssh is a volume,
# so a config file written there would be frozen at the volume's first use and
# never updated by a rebuild. Fedora's /etc/ssh/ssh_config includes this dir.
#
# accept-new is trust-on-first-use: the first connection to github.com is
# accepted unseen and pinned in the known_hosts volume, and a *changed* key
# afterwards is refused loudly. That is weaker than shipping GitHub's host
# keys and stronger than StrictHostKeyChecking=no, which accepts a changed key
# silently. To close the first-use gap, run this once and compare against
# GitHub's published fingerprints:
#   ssh-keyscan github.com >> ~/.ssh/known_hosts
RUN set -euo pipefail; \
    mkdir -p /etc/ssh/ssh_config.d; \
    printf '%s\n' \
      'Host *' \
      '    StrictHostKeyChecking accept-new' \
      '    ServerAliveInterval 30' \
      '    ControlMaster auto' \
      '    ControlPath /tmp/.ssh-mux-%r@%h:%p' \
      '    ControlPersist 10m' \
      > /etc/ssh/ssh_config.d/10-agent.conf; \
    grep -qs '^Include /etc/ssh/ssh_config.d/\*.conf' /etc/ssh/ssh_config \
      || sed -i '1i Include /etc/ssh/ssh_config.d/*.conf' /etc/ssh/ssh_config; \
    grep -q 'StrictHostKeyChecking accept-new' /etc/ssh/ssh_config.d/10-agent.conf \
      && grep -qs 'ssh_config.d' /etc/ssh/ssh_config \
      || { echo "FATAL: ssh client drop-in written but not included" >&2; exit 1; }

COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod 0755 /usr/local/bin/entrypoint.sh

USER ${USERNAME}
ENV HOME=/home/${USERNAME}
ENV PATH=${HOME}/.local/bin:${PATH}

# ---------------------------------------------------------------------------
# The four persistent directories, created here as `agent` on purpose.
#
# A named volume mounted over a path that exists in the image is initialised
# from that path — contents *and ownership* — the first time it is used. That
# is the whole mechanism by which these volumes end up writable by a non-root
# user without an entrypoint that runs as root to chown them. Create them
# after USER, or they are root-owned and every volume inherits that.
#
#   .ssh      keys and known_hosts
#   .config   gh login, git global config, agent CLI config (XDG)
#   .claude   Claude Code's own state; it does not use XDG
#   .cache    CARGO_HOME/RUSTUP_HOME/GOPATH live under here in stage 2
#
# ~/.local is deliberately NOT among them: it holds pi and cursor-agent, and a
# volume there would shadow the image's copies and pin them at whatever
# version the volume was first created with. Rebuilding the image is how those
# two get upgraded, and that only works if nothing is mounted over them.
# ---------------------------------------------------------------------------
RUN mkdir -p "$HOME/.ssh" "$HOME/.config" "$HOME/.claude" "$HOME/.cache" \
    && chmod 700 "$HOME/.ssh"

# Config that must land inside the .config volume so it survives a restart.
# git reads GIT_CONFIG_GLOBAL in place of ~/.gitconfig, which would otherwise
# sit in the unmounted part of $HOME and be lost on every `compose down`.
ENV GIT_CONFIG_GLOBAL=${HOME}/.config/git/config \
    HISTFILE=${HOME}/.config/bash_history \
    SSH_AUTH_SOCK=/tmp/agent-ssh-auth.sock \
    GH_NO_UPDATE_NOTIFIER=1

WORKDIR /work

# ---------------------------------------------------------------------------
# Agent CLIs — the two things Fedora does not package.
#
# pi comes from npm, pinned. npm is a package manager, not a curl|bash, and
# --prefix keeps it in $HOME so it is upgradeable without rebuilding.
# ---------------------------------------------------------------------------
ARG PI_VERSION=0.85.1
RUN set -euo pipefail; \
    npm install -g --prefix "${HOME}/.local" \
        "@earendil-works/pi-coding-agent@${PI_VERSION}"; \
    npm cache clean --force; \
    command -v pi >/dev/null || { echo "FATAL: pi not on PATH after install" >&2; exit 1; }

# The Cursor CLI ships only as an install script — no package, no release
# tarball with a published digest. It is therefore the one unpinned step in
# the image, and the one you should look at first if a rebuild behaves
# differently. INSTALL_CURSOR=0 leaves it out entirely.
ARG INSTALL_CURSOR=1
RUN set -euo pipefail; \
    if [ "$INSTALL_CURSOR" = "1" ]; then \
      curl -fsS https://cursor.com/install | bash; \
      command -v cursor-agent >/dev/null || { echo "FATAL: cursor-agent not on PATH" >&2; exit 1; }; \
    else \
      echo "skipping Cursor CLI (INSTALL_CURSOR=0)"; \
    fi

# Credentials are never baked into the image. They live in the .ssh and
# .config volumes, written once by you at run time — see compose.yaml.
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
CMD ["/bin/bash"]


# ===========================================================================
# Stage 2 — localfiles toolchains.
#
# `--target base` gives the image above on its own: terminal working set and
# agent CLIs, no language toolchains, useful for work outside this family.
# This stage adds what the five localfiles repos build with on Linux:
#
#   rust + GTK4/libadwaita   localgallery core/ and linux/
#   go + cgo                 localhealth (mattn/go-sqlite3 needs a C compiler)
#   exiftool                 localgallery conformance asset generation
#
# The Apple halves (xcodegen, xcodebuild, the iOS slices of the xcframework)
# are not here and cannot be: they need Xcode. This image builds and tests
# the portable parts, which is every Rust crate, all of localhealth, and the
# GTK shell.
#
# Not here on purpose: uv and the model-pack venv. torch is ~1 GB, the
# scripts that want it are localgallery's alone, and its own Dockerfile
# already carries them. A shared image should not pay that for four repos
# that never use it.
# ===========================================================================
FROM base AS localfiles

USER root

# rustup from Fedora rather than rustup-init.sh — same provenance rule, and
# it is the reason no Rust version appears in this file. `cargo` here is
# rustup's shim; `core/rust-toolchain.toml` names the actual compiler and
# rustup installs it on first use, into the mounted CARGO_HOME. A distro
# `rust` package instead of rustup would drift ahead of that pin and turn
# every clippy run into a different set of lints.
RUN dnf install -y --skip-unavailable --setopt=install_weak_deps=False \
        rustup \
        golang \
        gtk4-devel \
        libadwaita-devel \
        glib2-devel \
        pango-devel \
        cairo-devel \
        gdk-pixbuf2-devel \
        graphene-devel \
        sqlite-devel \
        perl-Image-ExifTool \
        desktop-file-utils \
    && dnf clean all && rm -rf /var/cache/dnf

# Assert the toolchain, for the same reason and in the same shape as stage 1.
# These are the least-verified package names in the file — `rustup` and
# `graphene-devel` especially — so the failure has to name the tool rather
# than the package, and has to come from here rather than from dnf.
#
# pkg-config is the real test for the GTK half: the -devel packages exist to
# put .pc files on the search path, and a renamed subpackage shows up as a
# missing .pc long before it shows up as a missing header.
RUN set -euo pipefail; \
    missing=(); \
    for t in rustup-init go exiftool; do \
      command -v "$t" >/dev/null || missing+=("$t"); \
    done; \
    if [ ${#missing[@]} -gt 0 ]; then \
      echo "FATAL: Fedora did not provide: ${missing[*]}" >&2; \
      echo "       rustup-init comes from 'rustup', go from 'golang'," >&2; \
      echo "       exiftool from 'perl-Image-ExifTool'. Check the names" >&2; \
      echo "       against this Fedora release and fix the dnf line above." >&2; \
      exit 1; \
    fi; \
    for m in gtk4 libadwaita-1 glib-2.0 pango cairo gdk-pixbuf-2.0 graphene-1.0 sqlite3; do \
      pkg-config --exists "$m" || { echo "FATAL: no pkg-config module '$m' — the -devel package for it is missing or renamed" >&2; exit 1; }; \
    done

USER agent

# Every expensive, re-downloadable thing lives under ~/.cache, which is one
# volume. That is the difference between "first build needs the network" once
# and once per container: the rustup toolchain named by rust-toolchain.toml,
# the crates.io registry, the ORT static library (~85 MB, see localgallery's
# build_core.sh) and the Go module cache all survive `compose down`.
#
# CARGO_HOME and RUSTUP_HOME are deliberately under .cache rather than at
# their conventional ~/.cargo and ~/.rustup, so that one volume covers all of
# it instead of four. Nothing else reads those paths; cargo and rustup take
# them from the environment.
ENV RUSTUP_HOME=/home/agent/.cache/rustup \
    CARGO_HOME=/home/agent/.cache/cargo \
    GOPATH=/home/agent/.cache/go \
    GOMODCACHE=/home/agent/.cache/go/pkg/mod \
    GOCACHE=/home/agent/.cache/go-build \
    NPM_CONFIG_CACHE=/home/agent/.cache/npm \
    CGO_ENABLED=1 \
    PATH=/home/agent/.local/bin:/home/agent/.cache/cargo/bin:/home/agent/.cache/go/bin:${PATH}

# rustup-init is NOT run here. CARGO_HOME is inside a volume, so shims laid
# down at build time would be captured into that volume on its first use and
# then shadow every later image. The entrypoint lays them down instead, which
# is idempotent and always matches the running image.

WORKDIR /work
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
CMD ["/bin/bash"]

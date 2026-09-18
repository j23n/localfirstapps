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
# Everything Fedora packages comes from Fedora. If a name is wrong, dnf fails
# the build. Optional extras (helix, eza, btop, EXTRA_PACKAGES) use
# --skip-unavailable so they do not.
#
# Fallbacks when something is not packaged:
#   1. run-time install into a persistent volume (`cargo install` / `go install`)
#   2. the language's own registry at build time (npm, pinned)
#   3. a pinned tarball via fetch-verify
#   4. a vendor install script — INSTALL_CURSOR is the only one

FROM fedora:44 AS base

SHELL ["/bin/bash", "-o", "pipefail", "-c"]

# --allowerasing: fedora:44 ships full curl; -minimal variants ship
# curl-minimal, which conflicts.
# install_weak_deps=False keeps desktop recommends out of a container.
RUN dnf install -y --allowerasing --setopt=install_weak_deps=False \
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
        openssl-devel \
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

ARG EXTRA_PACKAGES=""
RUN dnf install -y --skip-unavailable --setopt=install_weak_deps=False \
        helix \
        eza \
        btop \
        ${EXTRA_PACKAGES} \
    && dnf clean all && rm -rf /var/cache/dnf

# zellij and lazygit are not in Fedora. Install at run time into the
# persistent volumes:
#   cargo install zellij
#   go install github.com/jesseduffield/lazygit@latest
# tmux is the packaged multiplexer meanwhile.

# TUIs need a real UTF-8 locale. glibc-langpack-en makes en_US.UTF-8 exist;
# Fedora has no locale-gen step.
ENV LANG=en_US.UTF-8 \
    LC_ALL=en_US.UTF-8 \
    TERM=xterm-256color

# fetch-verify <url> <sha256> <dest>
RUN printf '%s\n' \
    '#!/bin/bash' \
    'set -euo pipefail' \
    'url="$1"; want="$2"; dest="$3"' \
    'curl -fsSL --retry 3 --retry-delay 2 --max-time 600 -o "$dest" "$url"' \
    'echo "${want}  ${dest}" | sha256sum -c - >/dev/null' \
    'echo "verified $(basename "$dest")"' \
    > /usr/local/bin/fetch-verify && chmod +x /usr/local/bin/fetch-verify

# Non-root user. UID/GID are build args so files written into a mounted
# /work land owned by you on the host.
ARG USERNAME=agent
ARG UID=1000
ARG GID=1000
RUN set -euo pipefail; \
    if ! getent group "$GID" >/dev/null; then groupadd -g "$GID" "$USERNAME"; fi; \
    useradd -m -u "$UID" -g "$GID" -s /bin/bash "$USERNAME"

# /work is a bind mount. git refuses a repo it thinks belongs to someone else.
# --system, not --global: GIT_CONFIG_GLOBAL lives in the .config volume and
# a volume shadows whatever the image wrote there.
RUN git config --system --add safe.directory '*'

# ssh client policy at image level: ~/.ssh is a volume, so a config written
# there would freeze at first use. accept-new is trust-on-first-use.
# To close the first-use gap:
#   ssh-keyscan github.com >> ~/.ssh/known_hosts
RUN mkdir -p /etc/ssh/ssh_config.d \
    && printf '%s\n' \
      'Host *' \
      '    StrictHostKeyChecking accept-new' \
      '    ServerAliveInterval 30' \
      '    ControlMaster auto' \
      '    ControlPath /tmp/.ssh-mux-%r@%h:%p' \
      '    ControlPersist 10m' \
      > /etc/ssh/ssh_config.d/10-agent.conf \
    && grep -qs '^Include /etc/ssh/ssh_config.d/\*.conf' /etc/ssh/ssh_config \
      || sed -i '1i Include /etc/ssh/ssh_config.d/*.conf' /etc/ssh/ssh_config

COPY entrypoint.sh /usr/local/bin/entrypoint.sh
COPY agent-home-CLAUDE.md /usr/local/share/agent/CLAUDE.md
RUN chmod 0755 /usr/local/bin/entrypoint.sh

USER ${USERNAME}
ENV HOME=/home/${USERNAME}
ENV PATH=${HOME}/.local/bin:${PATH}

# The persistent directories, created here as `agent` so named volumes
# inherit ownership on first use (no root entrypoint chown).
#
#   .ssh      keys and known_hosts
#   .config   gh login, git global config, agent CLI config (XDG)
#   .claude   Claude Code's own state; it does not use XDG
#   .cursor   Cursor chats, model prefs, CLI state; also not XDG
#   .cache    CARGO_HOME/RUSTUP_HOME/GOPATH live under here in stage 2
#
# ~/.local is not among them: it holds pi and cursor-agent. A volume there
# would pin them at first-create and a rebuild would not upgrade them.
RUN mkdir -p "$HOME/.ssh" "$HOME/.config" "$HOME/.claude" "$HOME/.cursor" "$HOME/.cache" \
    && chmod 700 "$HOME/.ssh"

ENV GIT_CONFIG_GLOBAL=${HOME}/.config/git/config \
    HISTFILE=${HOME}/.config/bash_history \
    SSH_AUTH_SOCK=/tmp/agent-ssh-auth.sock \
    GH_NO_UPDATE_NOTIFIER=1

WORKDIR /work

ARG PI_VERSION=0.85.1
RUN npm install -g --prefix "${HOME}/.local" \
        "@earendil-works/pi-coding-agent@${PI_VERSION}" \
    && npm cache clean --force

# cursor.com/install is the one unpinned step. INSTALL_CURSOR=0 leaves it out.
ARG INSTALL_CURSOR=1
RUN if [ "$INSTALL_CURSOR" = "1" ]; then \
      curl -fsS https://cursor.com/install | bash; \
    else \
      echo "skipping Cursor CLI (INSTALL_CURSOR=0)"; \
    fi

# Credentials live in the .ssh and .config volumes — see compose.yaml.
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
CMD ["/bin/bash"]


# ===========================================================================
# Stage 2 — localfiles toolchains.
#
# `--target base` is the image above: terminal working set and agent CLIs.
# This stage adds what the localfiles repos build with on Linux:
#
#   rust + GTK4/libadwaita   gallery core/linux; contacts/music GTK
#   mutter + dbus-daemon     headless Wayland (gtk-snapshots.sh, GTK perf)
#   gstreamer1               music-gtk --features gstreamer-playback
#   openssl-devel            rust openssl-sys / native-tls
#   go + cgo                 localhealth (mattn/go-sqlite3)
#   exiftool                 gallery conformance assets
#
# Apple halves (xcodegen, xcodebuild, iOS xcframework) need Xcode — not here.
# uv / the model-pack venv stay in gallery's own Dockerfile (torch is ~1 GB).
# ===========================================================================
FROM base AS localfiles

USER root

# rustup from Fedora, not rustup-init.sh. `cargo` is rustup's shim;
# rust-toolchain.toml names the compiler and rustup installs it on first use
# into the mounted CARGO_HOME.
RUN dnf install -y --setopt=install_weak_deps=False \
        rustup \
        golang \
        gtk4-devel \
        libadwaita-devel \
        mutter \
        dbus-daemon \
        gstreamer1-devel \
        gstreamer1-plugins-base-devel \
        gstreamer1-plugins-base \
        gstreamer1-plugins-good \
        glib2-devel \
        pango-devel \
        cairo-devel \
        gdk-pixbuf2-devel \
        graphene-devel \
        sqlite-devel \
        openssl-devel \
        perl-Image-ExifTool \
        desktop-file-utils \
        swift-lang \
    && dnf clean all && rm -rf /var/cache/dnf

USER agent

# Expensive caches under ~/.cache (one volume): rustup toolchain, crates.io,
# ORT static lib, Go module cache.
ENV RUSTUP_HOME=/home/agent/.cache/rustup \
    CARGO_HOME=/home/agent/.cache/cargo \
    GOPATH=/home/agent/.cache/go \
    GOMODCACHE=/home/agent/.cache/go/pkg/mod \
    GOCACHE=/home/agent/.cache/go-build \
    NPM_CONFIG_CACHE=/home/agent/.cache/npm \
    CGO_ENABLED=1 \
    PATH=/home/agent/.local/bin:/home/agent/.cache/cargo/bin:/home/agent/.cache/go/bin:${PATH}

# rustup-init is not run here. CARGO_HOME is inside a volume, so shims laid
# down at build time would be captured on first use and shadow later images.
# The entrypoint lays them down instead.

WORKDIR /work
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
CMD ["/bin/bash"]

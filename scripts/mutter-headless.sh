# Sourceable headless mutter + session bus for GTK binaries in this
# container. `dbus-run-session -- mutter` isolates the compositor from
# the app (Failed to open display). One bus, one XDG_RUNTIME_DIR, both
# processes inherit it.
#
#   source scripts/mutter-headless.sh
#   mutter_headless_start || exit 0
#   trap mutter_headless_stop EXIT

mutter_headless_start() {
  if ! command -v mutter >/dev/null 2>&1; then
    return 1
  fi
  if ! command -v dbus-daemon >/dev/null 2>&1; then
    return 1
  fi

  MUTTER_XDG_CONFIG="${MUTTER_XDG_CONFIG:-$(mktemp -d)}"
  MUTTER_XDG_CACHE="${MUTTER_XDG_CACHE:-$(mktemp -d)}"
  MUTTER_XDG_DATA="${MUTTER_XDG_DATA:-$(mktemp -d)}"
  MUTTER_RUNTIME="${XDG_RUNTIME_DIR:-}"
  if [[ -z "$MUTTER_RUNTIME" || ! -w "$MUTTER_RUNTIME" ]]; then
    MUTTER_RUNTIME="$(mktemp -d)"
  fi
  export XDG_CONFIG_HOME="$MUTTER_XDG_CONFIG"
  export XDG_CACHE_HOME="$MUTTER_XDG_CACHE"
  export XDG_DATA_HOME="$MUTTER_XDG_DATA"
  export XDG_RUNTIME_DIR="$MUTTER_RUNTIME"
  export DBUS_SESSION_BUS_ADDRESS="unix:path=${MUTTER_RUNTIME}/bus"
  export GDK_BACKEND=wayland
  export WAYLAND_DISPLAY=wayland-0
  export GTK_A11Y=none
  export GSK_RENDERER="${GSK_RENDERER:-cairo}"

  dbus-daemon --session --address="$DBUS_SESSION_BUS_ADDRESS" --fork --nopidfile
  MUTTER_DBUS=1

  local -a flags=(--headless --wayland --virtual-monitor 1280x800 --wayland-display wayland-0)
  if mutter --help 2>&1 | grep -q -- '--no-x11'; then
    flags+=(--no-x11)
  fi
  MUTTER_LOG="$(mktemp)"
  mutter "${flags[@]}" >"$MUTTER_LOG" 2>&1 &
  MUTTER_PID=$!

  local attempt
  for attempt in $(seq 1 80); do
    if ! kill -0 "$MUTTER_PID" 2>/dev/null; then
      echo "mutter-headless: mutter exited before creating a display" >&2
      cat "$MUTTER_LOG" >&2 || true
      return 1
    fi
    if [[ -S "${MUTTER_RUNTIME}/wayland-0" ]]; then
      return 0
    fi
    sleep 0.1
  done
  echo "mutter-headless: timed out waiting for wayland-0" >&2
  cat "$MUTTER_LOG" >&2 || true
  return 1
}

mutter_headless_stop() {
  if [[ -n "${MUTTER_PID:-}" ]] && kill -0 "$MUTTER_PID" 2>/dev/null; then
    kill "$MUTTER_PID" 2>/dev/null || true
    wait "$MUTTER_PID" 2>/dev/null || true
  fi
  MUTTER_PID=""
  if [[ -n "${MUTTER_LOG:-}" ]]; then
    rm -f "$MUTTER_LOG"
    MUTTER_LOG=""
  fi
}

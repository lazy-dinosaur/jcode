#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/lazydino/install-custom-jcode.sh [OPTIONS]

Build and install the current custom jcode checkout into every path used by the
local harness, including ~/.local/bin/jcode and the Jcode-managed stable/current
binary slots under ~/.jcode/builds/.

Options:
  --no-build          reuse target/release/jcode instead of running cargo build --release
  --restart-server   after installing, ask the running server to cleanly drain sessions,
                     then terminate shared jcode server processes so the next client
                     starts the newly installed binary
  -h, --help         show this help

Notes:
  - Replacing ~/.local/bin/jcode is done with an atomic temp-file rename so it
    works even when the old executable is currently running.
  - --restart-server only targets daemon-style `jcode ... serve` processes from
    ~/.jcode/builds/*; it does not kill foreground TUI client processes.
USAGE
}

DO_BUILD=1
DO_RESTART=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --no-build) DO_BUILD=0 ;;
    --restart-server) DO_RESTART=1 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
  shift
done

repo_root=$(git rev-parse --show-toplevel 2>/dev/null || true)
if [[ -z "$repo_root" ]]; then
  echo "error: not inside a git repository" >&2
  exit 1
fi
cd "$repo_root"

log() { printf '[install-custom-jcode] %s\n' "$*"; }

if [[ "$DO_BUILD" == 1 ]]; then
  log "building release binary"
  cargo build --release
fi

binary="$repo_root/target/release/jcode"
if [[ ! -x "$binary" ]]; then
  echo "error: release binary not found or not executable: $binary" >&2
  exit 1
fi

short_sha=$(git rev-parse --short=8 HEAD)
version_name="lazydino-${short_sha}"
version_dir="$HOME/.jcode/builds/versions/${version_name}"
local_bin_dir="$HOME/.local/bin"
local_bin="$local_bin_dir/jcode"

log "installing versioned binary to $version_dir"
mkdir -p "$version_dir" "$HOME/.jcode/builds/stable" "$HOME/.jcode/builds/current" "$local_bin_dir"
install -m 0755 "$binary" "$version_dir/jcode"

log "updating Jcode-managed stable/current symlinks"
ln -sfn "$version_dir/jcode" "$HOME/.jcode/builds/stable/jcode"
ln -sfn "$version_dir/jcode" "$HOME/.jcode/builds/current/jcode"

log "updating PATH binary $local_bin"
tmp_local="${local_bin}.tmp.$$"
install -m 0755 "$binary" "$tmp_local"
mv -f "$tmp_local" "$local_bin"

log "installed versions"
"$local_bin" --version
"$HOME/.jcode/builds/stable/jcode" --version

if [[ "$DO_RESTART" == 1 ]]; then
  log "attempting clean shutdown via debug socket"
  if "$local_bin" debug shutdown drain >/dev/null 2>&1; then
    log "clean drain initiated"
    sleep 2
  else
    log "debug drain unavailable, falling back to SIGTERM"
  fi

  mapfile -t server_pids < <(
    ps -eo pid=,args= \
      | awk -v home="$HOME" '$0 ~ home "/.jcode/builds/" && $0 ~ / serve( |$)/ { print $1 }'
  )

  if [[ "${#server_pids[@]}" -eq 0 ]]; then
    log "no running Jcode-managed server processes found"
  else
    log "terminating Jcode-managed server process(es): ${server_pids[*]}"
    kill -TERM "${server_pids[@]}" 2>/dev/null || true
    for _ in {1..50}; do
      alive=0
      for pid in "${server_pids[@]}"; do
        if kill -0 "$pid" 2>/dev/null; then
          alive=1
          break
        fi
      done
      [[ "$alive" == 0 ]] && break
      sleep 0.2
    done

    still_alive=()
    for pid in "${server_pids[@]}"; do
      if kill -0 "$pid" 2>/dev/null; then
        still_alive+=("$pid")
      fi
    done
    if [[ "${#still_alive[@]}" -gt 0 ]]; then
      log "force killing stubborn server process(es): ${still_alive[*]}"
      kill -KILL "${still_alive[@]}" 2>/dev/null || true
    fi
  fi

  log "server restart requested; the next jcode client will start $version_name"
else
  log "server not restarted. Run again with --restart-server or use /reload/restart clients to apply runtime changes."
fi

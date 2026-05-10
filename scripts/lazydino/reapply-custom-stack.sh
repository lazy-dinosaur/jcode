#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/lazydino/reapply-custom-stack.sh [OPTIONS]

Safely replay Lazydino's custom Jcode patch stack onto a fresh upstream base.

Default mode is dry-run: it prints what would happen and performs preflight checks.
It never force-pushes and never updates custom/lazydino-harness unless explicitly asked.

Options:
  --apply                 create a reapply branch and cherry-pick the patch stack
  --base REF              upstream base to replay onto (default: origin/master)
  --target BRANCH         branch to update after successful apply (default: custom/lazydino-harness)
  --work-branch NAME      temporary branch name (default: reapply/lazydino-UTC_TIMESTAMP)
  --no-fetch              skip git fetch origin
  --validate              run cargo check after successful apply
  --update-target         after successful apply, fast-forward/replace target branch to work branch
  --list                  print the ordered patch refs and exit
  -h, --help              show this help

Failure policy:
  - dirty working tree => stop
  - missing patch ref => stop
  - cherry-pick conflict => stop and print recovery instructions
  - validation failure => stop
  - no automatic push
USAGE
}

BASE="origin/master"
TARGET="custom/lazydino-harness"
WORK_BRANCH="reapply/lazydino-$(date -u +%Y%m%dT%H%M%SZ)"
DO_APPLY=0
DO_FETCH=1
DO_VALIDATE=0
DO_UPDATE_TARGET=0
DO_LIST=0

PATCH_REFS=(
  patch/mermaid-label-rendering
  patch/tool-lifecycle-hooks
  patch/custom-maintenance-docs
  patch/project-local-hook-config
  patch/subagent-model-routing
  patch/private-jcode-harness
  patch/opencode-category-routing-docs
  patch/canonical-routing-model-ids
  patch/rich-agent-routes
  patch/dated-haiku-route-id
  patch/agent-profiles
  patch/tmux-jcode-passthrough-docs
  patch/jcode-init-skill-docs
  patch/reapply-custom-stack
  patch/native-project-init
  patch/ambient-serde-args
  patch/upstream-pr-triage-docs
  patch/project-skill-sync
  patch/custom-install-server-paths
)

while [[ $# -gt 0 ]]; do
  case "$1" in
    --apply) DO_APPLY=1 ;;
    --base) BASE="${2:?--base requires a ref}"; shift ;;
    --target) TARGET="${2:?--target requires a branch}"; shift ;;
    --work-branch) WORK_BRANCH="${2:?--work-branch requires a name}"; shift ;;
    --no-fetch) DO_FETCH=0 ;;
    --validate) DO_VALIDATE=1 ;;
    --update-target) DO_UPDATE_TARGET=1 ;;
    --list) DO_LIST=1 ;;
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

if [[ "$DO_LIST" == 1 ]]; then
  printf '%s\n' "${PATCH_REFS[@]}"
  exit 0
fi

log() { printf '[reapply] %s\n' "$*"; }
fail() { printf '[reapply] error: %s\n' "$*" >&2; exit 1; }

require_clean_tree() {
  if [[ -n "$(git status --porcelain)" ]]; then
    git status --short >&2
    fail "working tree is dirty. Commit/stash changes before replaying the patch stack."
  fi
}

ensure_ref() {
  local ref="$1"
  git rev-parse --verify --quiet "$ref^{commit}" >/dev/null || fail "missing ref: $ref"
}

preflight() {
  log "repo: $repo_root"
  log "base: $BASE"
  log "target: $TARGET"
  log "work branch: $WORK_BRANCH"
  log "mode: $([[ "$DO_APPLY" == 1 ]] && echo apply || echo dry-run)"

  require_clean_tree

  if [[ "$DO_FETCH" == 1 ]]; then
    log "fetching origin"
    git fetch origin
  else
    log "skipping fetch"
  fi

  ensure_ref "$BASE"
  ensure_ref "$TARGET"

  log "checking ordered patch refs"
  local ref
  for ref in "${PATCH_REFS[@]}"; do
    ensure_ref "$ref"
    printf '  %-42s %s %s\n' "$ref" "$(git rev-parse --short "$ref")" "$(git log -1 --format=%s "$ref")"
  done
}

print_recovery() {
  cat <<EOF_RECOVERY

Cherry-pick stopped on a conflict.

Inspect:
  git status
  git diff

Resolve and continue:
  git add <resolved-files>
  git cherry-pick --continue

Abort this replay:
  git cherry-pick --abort
  git switch -

Use LAZYDINO_MAINTENANCE.md and the corresponding patch/* branch as the source of truth.
EOF_RECOVERY
}

apply_stack() {
  log "creating work branch $WORK_BRANCH from $BASE"
  git switch --create "$WORK_BRANCH" "$BASE"

  local ref
  for ref in "${PATCH_REFS[@]}"; do
    log "cherry-pick $ref ($(git rev-parse --short "$ref"))"
    if ! git cherry-pick "$ref"; then
      print_recovery >&2
      exit 1
    fi
  done

  if [[ "$DO_VALIDATE" == 1 ]]; then
    log "running cargo check"
    cargo check
  fi

  if [[ "$DO_UPDATE_TARGET" == 1 ]]; then
    log "updating $TARGET to $WORK_BRANCH"
    git branch -f "$TARGET" "$WORK_BRANCH"
    log "target updated locally only. Push manually with: git push fork $TARGET --force-with-lease"
  fi

  log "success: patch stack replayed on $WORK_BRANCH"
}

preflight

if [[ "$DO_APPLY" != 1 ]]; then
  cat <<EOF_DRYRUN

Dry-run only. To actually replay the stack:

  scripts/lazydino/reapply-custom-stack.sh --apply --validate

To replay onto a specific base:

  scripts/lazydino/reapply-custom-stack.sh --apply --base origin/master --validate

After success, inspect the work branch before updating or pushing anything.
EOF_DRYRUN
  exit 0
fi

apply_stack

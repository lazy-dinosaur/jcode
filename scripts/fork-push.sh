#!/bin/bash
# Lazydino fork push helper for M21.
# Loads .env (gitignored) and pushes specified refs to fork using the
# token's repo+workflow scopes.
#
# Usage:
#   scripts/fork-push.sh                       # default: deploy + 3 code patches
#   scripts/fork-push.sh master                # ff fork/master to origin/master
#   scripts/fork-push.sh deploy/m9-m10 patch/<name> ...

set -euo pipefail
cd "$(dirname "$0")/.."

if [[ ! -f .env ]]; then
  echo "ERROR: .env not found. See LAZYDINO_MAINTENANCE.md '.env file' section." >&2
  exit 1
fi

# shellcheck disable=SC1091
source .env
: "${GH_TOKEN:?GH_TOKEN missing from .env}"
: "${GH_USERNAME:?GH_USERNAME missing from .env}"

# Temp askpass that never writes the token to disk in plaintext form
# we don't already have.
ASKPASS=$(mktemp)
trap 'rm -f "$ASKPASS"' EXIT
cat > "$ASKPASS" <<'INNER'
#!/bin/bash
case "$1" in
  Username*) echo "$GH_USERNAME" ;;
  Password*) echo "$GH_TOKEN" ;;
esac
INNER
chmod 700 "$ASKPASS"

REFS=("$@")
if [[ ${#REFS[@]} -eq 0 ]]; then
  REFS=(deploy/m9-m10 patch/sdk-history-images patch/config-hot-reload patch/bash-tool-timeout)
fi

# Special case: 'master' means ff fork/master to origin/master.
if [[ "${REFS[0]:-}" == "master" ]]; then
  echo "==> fork/master fast-forward to origin/master"
  GH_USERNAME="$GH_USERNAME" GH_TOKEN="$GH_TOKEN" \
    GIT_ASKPASS="$ASKPASS" git push fork origin/master:master
  exit 0
fi

echo "==> pushing to fork: ${REFS[*]}"
GH_USERNAME="$GH_USERNAME" GH_TOKEN="$GH_TOKEN" \
  GIT_ASKPASS="$ASKPASS" git push fork "${REFS[@]}"

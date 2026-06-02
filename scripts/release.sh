#!/usr/bin/env bash
# Cut a new DayTrail release.
#
# Usage:
#   scripts/release.sh 0.1.4              # bump to 0.1.4, commit, tag v0.1.4, push
#   scripts/release.sh 0.1.4 --dry-run    # preview changes only
#
# What it does:
#   1. Validates version is semver (x.y.z) and not already tagged.
#   2. Updates the canonical version in:
#        - apps/desktop/package.json
#        - apps/desktop/src-tauri/tauri.conf.json
#        - apps/desktop/src-tauri/Cargo.toml
#        - apps/desktop/src-tauri/Cargo.lock (daytrail-desktop entry)
#   3. Verifies all four files agree on the new version.
#   4. Commits as "chore(release): vX.Y.Z".
#   5. Tags vX.Y.Z and pushes the commit + tag to origin.
#
# GitHub Actions (macos-release.yml / windows-release.yml) will pick up the
# tag push, build, and attach the DMG/installers to a NEW GitHub Release.
#
# Requirements: clean working tree, on main branch.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

DRY_RUN=0
VERSION=""
for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN=1 ;;
    -h|--help)
      sed -n '2,20p' "$0"; exit 0 ;;
    *)
      if [ -z "$VERSION" ]; then VERSION="$arg"; else
        echo "Unexpected argument: $arg" >&2; exit 2
      fi
      ;;
  esac
done

if [ -z "$VERSION" ]; then
  echo "Usage: scripts/release.sh <new-version> [--dry-run]" >&2
  exit 2
fi

if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Version must be semver x.y.z, got: $VERSION" >&2
  exit 2
fi

TAG="v$VERSION"

# Preflight checks.
BRANCH="$(git rev-parse --abbrev-ref HEAD)"
if [ "$BRANCH" != "main" ]; then
  echo "Refusing to release from branch '$BRANCH'; switch to main." >&2
  exit 1
fi

if ! git diff --quiet || ! git diff --cached --quiet; then
  echo "Working tree is dirty. Commit or stash changes first." >&2
  git status --short >&2
  exit 1
fi

if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
  echo "Tag $TAG already exists locally." >&2
  exit 1
fi

if git ls-remote --tags origin "$TAG" | grep -q "$TAG"; then
  echo "Tag $TAG already exists on origin." >&2
  exit 1
fi

# Read current version from tauri.conf.json as source of truth.
CURRENT="$(node -p "require('./apps/desktop/src-tauri/tauri.conf.json').version")"
echo "Current version: $CURRENT"
echo "New version:     $VERSION"

if [ "$CURRENT" = "$VERSION" ]; then
  echo "New version matches current. Nothing to bump." >&2
  exit 1
fi

bump_file() {
  local file="$1" pattern="$2" replacement="$3"
  if [ "$DRY_RUN" = "1" ]; then
    echo "[dry-run] would update $file"
    return
  fi
  # Use perl for portable in-place edit (BSD/GNU sed differ on -i).
  perl -i -pe "s|$pattern|$replacement|" "$file"
}

# 1. apps/desktop/package.json
bump_file "apps/desktop/package.json" \
  "\"version\": \"$CURRENT\"" \
  "\"version\": \"$VERSION\""

# 2. apps/desktop/src-tauri/tauri.conf.json
bump_file "apps/desktop/src-tauri/tauri.conf.json" \
  "\"version\": \"$CURRENT\"" \
  "\"version\": \"$VERSION\""

# 3. apps/desktop/src-tauri/Cargo.toml (only the [package] version line)
if [ "$DRY_RUN" != "1" ]; then
  perl -i -pe "BEGIN{\$n=0} if (/^name = \"daytrail-desktop\"/) {\$n=1} elsif (\$n && s|^version = \"$CURRENT\"|version = \"$VERSION\"|) {\$n=0}" \
    apps/desktop/src-tauri/Cargo.toml
fi

# 4. apps/desktop/src-tauri/Cargo.lock (daytrail-desktop entry)
if [ "$DRY_RUN" != "1" ]; then
  perl -i -pe "BEGIN{\$n=0} if (/^name = \"daytrail-desktop\"/) {\$n=1} elsif (\$n && s|^version = \"$CURRENT\"|version = \"$VERSION\"|) {\$n=0}" \
    apps/desktop/src-tauri/Cargo.lock
fi

if [ "$DRY_RUN" = "1" ]; then
  echo "[dry-run] skipping commit/tag/push."
  exit 0
fi

# Verify all files agree.
v_pkg="$(node -p "require('./apps/desktop/package.json').version")"
v_tauri="$(node -p "require('./apps/desktop/src-tauri/tauri.conf.json').version")"
v_cargo="$(grep -m1 '^version' apps/desktop/src-tauri/Cargo.toml | sed -E 's/.*"(.*)".*/\1/')"
if [ "$v_pkg" != "$VERSION" ] || [ "$v_tauri" != "$VERSION" ] || [ "$v_cargo" != "$VERSION" ]; then
  echo "Version mismatch after bump:" >&2
  echo "  package.json     = $v_pkg" >&2
  echo "  tauri.conf.json  = $v_tauri" >&2
  echo "  Cargo.toml       = $v_cargo" >&2
  exit 1
fi

echo "Bumped to $VERSION. Committing and tagging..."
git add \
  apps/desktop/package.json \
  apps/desktop/src-tauri/tauri.conf.json \
  apps/desktop/src-tauri/Cargo.toml \
  apps/desktop/src-tauri/Cargo.lock

git commit -m "chore(release): $TAG"
git tag -a "$TAG" -m "DayTrail $TAG"

echo "Pushing commit and tag to origin..."
git push origin main
git push origin "$TAG"

echo
echo "Done. GitHub Actions will build and publish $TAG release."
echo "Watch: https://github.com/varaprasadreddy9676/DayTrail/actions"

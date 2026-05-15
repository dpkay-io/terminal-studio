#!/usr/bin/env bash
# Tag and push a release based on the version in Cargo.toml.
# Usage:  scripts/release.sh            # tags v<Cargo.toml version> at HEAD and pushes
#         scripts/release.sh --dry-run  # show what would happen, don't tag/push
#         scripts/release.sh --force    # skip clean-tree check
set -euo pipefail

DRY_RUN=0
FORCE=0
for arg in "$@"; do
    case "$arg" in
        --dry-run) DRY_RUN=1 ;;
        --force)   FORCE=1 ;;
        -h|--help)
            sed -n '2,5p' "$0"; exit 0 ;;
        *) echo "unknown arg: $arg" >&2; exit 2 ;;
    esac
done

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

version="$(grep -E '^[[:space:]]*version[[:space:]]*=' Cargo.toml | head -n1 | sed -E 's/.*"([^"]+)".*/\1/')"
if [ -z "$version" ]; then
    echo 'Could not find version = "..." in Cargo.toml' >&2
    exit 1
fi
tag="v$version"

echo "Cargo.toml version: $version"
echo "Tag to create:      $tag"

if [ -n "$(git status --porcelain)" ] && [ "$FORCE" -ne 1 ]; then
    echo
    echo 'Working tree is not clean:'
    git status --porcelain
    echo 'Refusing to tag a dirty tree. Commit/stash first, or rerun with --force.' >&2
    exit 1
fi

if [ -n "$(git tag --list "$tag")" ]; then
    echo "Tag $tag already exists locally. Bump Cargo.toml version or delete the tag." >&2
    exit 1
fi

if [ -n "$(git ls-remote --tags origin "refs/tags/$tag")" ]; then
    echo "Tag $tag already exists on origin. Bump Cargo.toml version." >&2
    exit 1
fi

head="$(git rev-parse --short HEAD)"
echo "HEAD commit:        $head"

if [ "$DRY_RUN" -eq 1 ]; then
    echo
    echo 'DRY RUN: would execute:'
    echo "  git tag $tag $head"
    echo "  git push origin $tag"
    exit 0
fi

git tag "$tag"
if ! git push origin "$tag"; then
    git tag -d "$tag" >/dev/null
    echo 'git push failed; local tag removed so you can retry.' >&2
    exit 1
fi

echo
echo "Pushed $tag. Release workflow should be running now."
echo 'Watch:  gh run list --workflow=release.yml --limit 3'

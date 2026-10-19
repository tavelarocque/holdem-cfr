#!/usr/bin/env bash
# Copy the current commit of this (private, development) repo into the public
# repo as one new commit: no development history, no files outside git.
#
#   scripts/sync_public.sh [path-to-public-clone] ["commit message"]
set -euo pipefail
src="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
dst="${1:-$src/../holdem-cfr-public}"
msg="${2:-Sync with development repo}"
if [ -n "$(git -C "$src" status --porcelain)" ]; then
  echo "commit or stash your changes first: the public copy is taken from HEAD" >&2
  exit 1
fi
git -C "$dst" rm -rq --ignore-unmatch .
git -C "$src" archive HEAD | tar -x -C "$dst"
git -C "$dst" add -A
if git -C "$dst" diff --cached --quiet; then
  echo "public copy already matches $(git -C "$src" rev-parse --short HEAD)"
  exit 0
fi
git -C "$dst" commit -q -m "$msg"
echo "committed to $dst; push with: git -C \"$dst\" push"

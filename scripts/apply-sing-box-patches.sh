#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE_DIR="${MAGICNET_SINGBOX_SOURCE_DIR:-$ROOT/sing-box}"
PATCH_DIR="$ROOT/sing-box-patches"

usage() {
    printf 'usage: %s [--check]\n' "${0##*/}" >&2
    printf '\n' >&2
    printf 'Applies the MagicNet patch series to the sing-box fork checkout.\n' >&2
    printf 'The module build rejects a dirty submodule, so commit the result in\n' >&2
    printf 'the fork and bump the sing-box gitlink before building.\n' >&2
}

check_only=0
case "${1:-}" in
"") ;;
--check) check_only=1 ;;
-h | --help)
    usage
    exit 0
    ;;
*)
    usage
    exit 64
    ;;
esac

if [ ! -f "$SOURCE_DIR/go.mod" ]; then
    printf 'sing-box patch: source submodule is not initialized: %s\n' "$SOURCE_DIR" >&2
    printf 'run: git submodule update --init sing-box\n' >&2
    exit 1
fi
if ! git -C "$SOURCE_DIR" rev-parse --verify HEAD >/dev/null 2>&1; then
    printf 'sing-box patch: source submodule is not a git checkout: %s\n' "$SOURCE_DIR" >&2
    exit 1
fi

patches=()
while IFS= read -r patch; do
    [ -n "$patch" ] || continue
    patches+=("$patch")
done < <(find "$PATCH_DIR" -maxdepth 1 -type f -name '*.patch' | LC_ALL=C sort)
if [ "${#patches[@]}" -eq 0 ]; then
    printf 'sing-box patch: no patches found under %s\n' "$PATCH_DIR" >&2
    exit 1
fi

# The build helper refuses a dirty submodule. Fail before touching the tree so a
# half applied series can never be mistaken for a clean checkout.
changes="$(git -C "$SOURCE_DIR" status --porcelain --untracked-files=normal --ignore-submodules=all)"
if [ -n "$changes" ]; then
    printf 'sing-box patch: source submodule has uncommitted changes: %s\n%s\n' \
        "$SOURCE_DIR" "$changes" >&2
    printf 'commit or discard them before applying the MagicNet patch series\n' >&2
    exit 1
fi

applied=0
for patch in "${patches[@]}"; do
    if git -C "$SOURCE_DIR" apply --check -p1 "$patch" >/dev/null 2>&1; then
        if [ "$check_only" -eq 1 ]; then
            printf 'can-apply %s\n' "${patch#"$ROOT"/}"
            continue
        fi
        git -C "$SOURCE_DIR" apply -p1 "$patch"
        printf 'applied %s\n' "${patch#"$ROOT"/}"
        applied=$((applied + 1))
        continue
    fi
    if git -C "$SOURCE_DIR" apply --reverse --check -p1 "$patch" >/dev/null 2>&1; then
        printf 'already applied %s\n' "${patch#"$ROOT"/}"
        continue
    fi
    printf 'sing-box patch: %s neither applies nor is already applied\n' "${patch#"$ROOT"/}" >&2
    printf 'the sing-box checkout is not the revision the series was generated for\n' >&2
    exit 1
done

if [ "$check_only" -eq 1 ]; then
    printf 'sing-box patch: series is applicable to %s\n' "$(git -C "$SOURCE_DIR" rev-parse HEAD)"
    exit 0
fi

if [ "$applied" -gt 0 ]; then
    printf '\n' >&2
    printf 'Next steps (the module build rejects a dirty submodule):\n' >&2
    printf '  git -C %s add -A && git -C %s commit -m "feat(route): restore sniff override_destination"\n' \
        "$SOURCE_DIR" "$SOURCE_DIR" >&2
    printf '  git -C %s push <fork-remote> <branch>\n' "$SOURCE_DIR" >&2
    printf '  git add sing-box && git commit -m "build: bump sing-box for domain forwarding"\n' >&2
fi
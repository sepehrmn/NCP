#!/usr/bin/env bash
# repin-ncp.sh — re-pin every NCP consumer to a single target tag, in one command.
#
# DECOUPLED BY DESIGN: this script holds **no knowledge of any specific consumer**.
# It discovers consumers by globbing for a `.ncp-consumer` descriptor in each
# sibling repo under base-dir, and applies a standard re-pin recipe per declared
# pin type (or a consumer-supplied `repin_cmd` for custom cases like a vendored
# mirror). Onboarding a new consumer requires ZERO changes here — the consumer
# commits a `.ncp-consumer` to its own repo. See `INTEGRATING.md`.
#
# `.ncp-consumer` lines understood here:
#   cargo_tag  <Cargo.toml>     # rewrite ncp-core/ncp-zenoh `tag = "vX"`, then
#                               #   `cargo update -p ncp-core -p ncp-zenoh --manifest-path <Cargo.toml>`
#   npm_tag    <package.json>   # rewrite `github:.../NCP#vX` (key kept), then `bun install`
#   cargo_lock / npm_lock / mirror_ref  # declared for the pin CHECKER; refreshed implicitly here
#   repin_cmd  <cmd ... {TAG}>  # consumer-owned re-pin command, run in the consumer dir
#                               #   ({TAG} is substituted). Use for mirrors / bespoke flows.
#
# This script edits files + refreshes lockfiles ONLY. It does NOT git-commit, push, or
# stage anything. It prints a per-repo summary and the suggested review/commit commands.
#
# Usage:
#   scripts/repin-ncp.sh <tag> [base-dir]
#     <tag>       NCP git tag to pin to, e.g. v0.3.0 (v<MAJOR>.<MINOR>.<PATCH>[-pre][+build]).
#     [base-dir]  Directory holding the sibling repos. Defaults to the parent of the NCP repo.
#
# Conventions: macOS-portable in-place edits via `perl -pi -e`. No AI/agent attribution.
set -euo pipefail

if [[ $# -lt 1 || $# -gt 2 ]]; then
  echo "usage: $(basename "$0") <tag> [base-dir]" >&2
  exit 2
fi

TAG="$1"
if [[ ! "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?(\+[0-9A-Za-z.]+)?$ ]]; then
  echo "ERROR: tag '$TAG' is not a valid NCP tag (expected like v0.3.0 or v0.3.0-rc.1)." >&2
  exit 2
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
NCP_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BASE_DIR="${2:-$(cd "$NCP_ROOT/.." && pwd)}"
if [[ ! -d "$BASE_DIR" ]]; then
  echo "ERROR: base-dir '$BASE_DIR' does not exist." >&2
  exit 2
fi
BASE_DIR="$(cd "$BASE_DIR" && pwd)"

# Locate bun (prefer ~/.bun/bin/bun, else PATH).
BUN_BIN=""
if [[ -x "$HOME/.bun/bin/bun" ]]; then BUN_BIN="$HOME/.bun/bin/bun"
elif command -v bun >/dev/null 2>&1; then BUN_BIN="$(command -v bun)"; fi

SUMMARY=()
REVIEW_CMDS=()
HAD_WARNINGS=0
add_summary() { SUMMARY+=("$1"$'\t'"$2"$'\t'"$3"); }
note()  { printf '  . %s\n' "$1"; }
hdr()   { printf '\n== %s ==\n' "$1"; }
warn()  { printf '  ! %s\n' "$1" >&2; HAD_WARNINGS=1; }

# Portable in-place edits. Cargo: rewrite the NCP tag ONLY on dependency lines that
# point at a */NCP git source (a comment merely mentioning t<tag> is never touched).
repin_cargo_manifest() {
  perl -pi -e 'if (/^\s*[A-Za-z0-9_-]+\s*=\s*\{.*git\s*=\s*"[^"]*\/NCP"/) { s{(tag\s*=\s*")[^"]*(")}{${1}'"$TAG"'${2}}g; }' "$1"
}
# package.json / bun.lock spec: only the #<tag> fragment changes; the scope key is kept.
repin_package_json() {
  perl -pi -e 's{(github:[^/]+/NCP#)[^"]*}{${1}'"$TAG"'}g' "$1"
}

echo "Re-pinning all NCP consumers to tag: $TAG"
echo "  base-dir : $BASE_DIR"
echo "  NCP repo : $NCP_ROOT"

shopt -s nullglob
descriptors=("$BASE_DIR"/*/.ncp-consumer)
shopt -u nullglob
if [[ "${#descriptors[@]}" -eq 0 ]]; then
  echo "No consumers found under $BASE_DIR (no */.ncp-consumer descriptors). Nothing to do." >&2
  echo "A consumer registers by committing a .ncp-consumer file to its repo root (see INTEGRATING.md)." >&2
  exit 1
fi

for desc in "${descriptors[@]}"; do
  consumer_dir="$(cd "$(dirname "$desc")" && pwd)"
  consumer_name="$(basename "$consumer_dir")"
  hdr "$consumer_name"

  # Parse the descriptor into typed target lists.
  cargo_tomls=(); npm_jsons=(); repin_cmd=""
  while IFS= read -r line || [[ -n "$line" ]]; do
    line="${line%%#*}"
    # shellcheck disable=SC2206
    fields=($line)
    [[ "${#fields[@]}" -ge 2 ]] || continue
    case "${fields[0]}" in
      cargo_tag) cargo_tomls+=("${fields[1]}") ;;
      npm_tag)   npm_jsons+=("${fields[1]}") ;;
      repin_cmd) repin_cmd="${line#*repin_cmd }" ;;   # rest of line, verbatim
    esac
  done < "$desc"

  touched=0
  review_files=()

  # 1) Consumer-owned custom re-pin (e.g. a vendored mirror's sync script).
  if [[ -n "$repin_cmd" ]]; then
    cmd="${repin_cmd//\{TAG\}/$TAG}"
    note "running consumer repin_cmd: $cmd"
    if ( cd "$consumer_dir" && bash -c "$cmd" ); then
      touched=1
      add_summary "$consumer_name" "REPINNED" "via repin_cmd -> $TAG"
      REVIEW_CMDS+=("# $consumer_name (consumer-owned re-pin)
  git -C \"$consumer_dir\" diff
  git -C \"$consumer_dir\" add -A && git -C \"$consumer_dir\" commit -m \"chore: re-pin NCP to $TAG\"")
    else
      warn "repin_cmd failed in $consumer_name — NOT re-pinned"
      add_summary "$consumer_name" "FAILED" "repin_cmd returned non-zero"
    fi
    continue
  fi

  # 2) Standard cargo re-pin.
  for rel in "${cargo_tomls[@]}"; do
    f="$consumer_dir/$rel"
    if [[ -f "$f" ]]; then
      repin_cargo_manifest "$f"; note "rewrote ncp-core/ncp-zenoh tag -> $TAG in $rel"; touched=1; review_files+=("$rel")
      if command -v cargo >/dev/null 2>&1; then
        note "refreshing lockfile (cargo update -p ncp-core -p ncp-zenoh --manifest-path $rel) ..."
        if ( cd "$consumer_dir" && cargo update -p ncp-core -p ncp-zenoh --manifest-path "$rel" ); then
          note "Cargo.lock refreshed"
        else warn "cargo update failed in $consumer_name ($rel) — Cargo.lock NOT refreshed; re-run before committing"; fi
      else warn "cargo not found — Cargo.lock NOT refreshed for $consumer_name"; fi
    else warn "declared cargo_tag file missing: $f"; fi
  done

  # 3) Standard npm/bun re-pin.
  for rel in "${npm_jsons[@]}"; do
    f="$consumer_dir/$rel"
    if [[ -f "$f" ]]; then
      repin_package_json "$f"; note "rewrote @scope/ncp pin -> #$TAG in $rel (key kept)"; touched=1; review_files+=("$rel")
      if [[ -n "$BUN_BIN" ]]; then
        note "running 'bun install' to regenerate the lockfile ..."
        if ( cd "$consumer_dir" && "$BUN_BIN" install ); then note "bun lockfile refreshed"
        else warn "'bun install' failed in $consumer_name — lockfile NOT refreshed; re-run before committing"; fi
      else warn "bun not found — lockfile NOT refreshed for $consumer_name; run 'bun install' manually"; fi
    else warn "declared npm_tag file missing: $f"; fi
  done

  if [[ "$touched" -eq 1 ]]; then
    [[ -n "${review_files+x}" ]] && add_summary "$consumer_name" "REPINNED" "$(printf '%s ' "${review_files[@]}")-> $TAG"
    REVIEW_CMDS+=("# $consumer_name
  git -C \"$consumer_dir\" diff
  git -C \"$consumer_dir\" add -A && git -C \"$consumer_dir\" commit -m \"chore: re-pin NCP to $TAG\"")
  else
    add_summary "$consumer_name" "SKIPPED" "no re-pinnable target found (.ncp-consumer declared none present)"
  fi
done

hdr "Summary — re-pin to $TAG"
printf '\n%-22s %-9s %s\n' "REPO" "STATUS" "DETAIL"
printf -- '%-22s %-9s %s\n' "----" "------" "------"
for row in "${SUMMARY[@]}"; do
  IFS=$'\t' read -r repo status detail <<< "$row"
  printf '%-22s %-9s %s\n' "$repo" "$status" "$detail"
done

echo ""
echo "No commits, pushes, or staging were performed. Files + lockfiles were edited in place."
if [[ ${#REVIEW_CMDS[@]} -gt 0 ]]; then
  echo ""
  echo "Suggested review / commit commands (the maintainer pushes to main directly — no PRs):"
  echo ""
  for block in "${REVIEW_CMDS[@]}"; do printf '%s\n\n' "$block"; done
else
  echo ""
  echo "Nothing was re-pinned (no consumers with re-pinnable targets under $BASE_DIR)."
fi
if [[ "$HAD_WARNINGS" -ne 0 ]]; then
  echo "One or more steps were skipped or failed (see '!' lines above). Review before committing." >&2
fi

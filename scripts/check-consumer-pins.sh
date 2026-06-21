#!/usr/bin/env bash
# Report the NCP pin each downstream consumer references and verify they agree.
# READ-ONLY: never writes to any repo, runs no builds, makes no network/git calls —
# it only inspects pin files on disk. Use it as a pre-flight or CI guard around an
# NCP tag bump.
#
# DECOUPLED BY DESIGN: this script holds **no knowledge of any specific consumer**.
# It discovers consumers by globbing for a `.ncp-consumer` descriptor in each
# sibling repo under base-dir. A consumer registers itself (and declares which of
# its files carry the NCP pin) by committing that descriptor to its OWN repo —
# onboarding a new consumer therefore requires ZERO changes here. See
# `INTEGRATING.md` (§"Registering a consumer") for the descriptor format.
#
# `.ncp-consumer` format — one `<type> <relative-path>` per line, `#` comments:
#   cargo_tag   <Cargo.toml>     # ncp-core/ncp-zenoh git-dep `tag = "vX"`
#   cargo_lock  <Cargo.lock>     # resolved `NCP?tag=vX`
#   npm_tag     <package.json>   # `@sepehrmn/ncp": "github:sepahead/NCP#vX`
#   npm_lock    <bun.lock>       # same spec `#vX` (+ resolved commit, informational)
#   mirror_ref  <.mirror-ref>    # a vendored-mirror pin file containing the tag
#
# Tracks sepahead/NCP#8 (drift-guarded pins).
set -euo pipefail

usage() {
  cat <<'EOF'
check-consumer-pins.sh — report and verify the NCP pin each consumer references.

READ-ONLY: inspects pin files only. No writes, no builds, no git/network calls.

Usage:
  check-consumer-pins.sh [expected-tag] [base-dir]

  expected-tag   If given, every consumer MUST reference exactly this tag;
                 the script exits non-zero otherwise.
  base-dir       Directory holding the sibling repos. Defaults to the parent of
                 this NCP checkout.

  With no expected-tag the script only checks the consumers agree with one
  another; it exits non-zero if they disagree.

Consumers are DISCOVERED, not hardcoded: any sibling dir with a `.ncp-consumer`
descriptor is inspected. Onboard a consumer by committing that file to its repo.

Exit codes: 0 = ok; 1 = mismatch / missing / unresolved pin; 2 = bad usage.
EOF
}

# --- argument parsing (positional, but trap -h/--help and stray flags) --------
EXPECTED=""
BASE_DIR=""
positional=()
for arg in "$@"; do
  case "$arg" in
    -h|--help) usage; exit 0 ;;
    -*)        echo "ERROR: unknown option '$arg'" >&2; echo >&2; usage >&2; exit 2 ;;
    *)         positional+=("$arg") ;;
  esac
done
if [[ "${#positional[@]}" -ge 1 ]]; then EXPECTED="${positional[0]}"; fi
if [[ "${#positional[@]}" -ge 2 ]]; then BASE_DIR="${positional[1]}"; fi
if [[ "${#positional[@]}" -gt 2 ]]; then
  echo "ERROR: too many arguments" >&2; echo >&2; usage >&2; exit 2
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
NCP_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
if [[ -z "$BASE_DIR" ]]; then BASE_DIR="$(cd "$NCP_ROOT/.." && pwd)"; fi

if [[ ! -d "$BASE_DIR" ]]; then
  echo "ERROR: base-dir '$BASE_DIR' is not a directory" >&2
  exit 2
fi
BASE_DIR="$(cd "$BASE_DIR" && pwd)"

# Parallel arrays describing each (sub-)consumer row: a human label and the tag we
# extracted (or a sentinel: "__MISSING__" = file absent, "__UNRESOLVED__" = file
# present but no pin matched).
LABELS=()
TAGS=()
NOTES=()

add_row() { LABELS+=("$1"); TAGS+=("$2"); }

# Extract the first capture group of a Perl regex from a file, or a sentinel.
# The regex is passed via the environment so Perl compiles it directly — no need
# to escape "/" and no shell-injection into the // delimiters. We use Perl (not
# grep -P) because BSD/macOS grep lacks -P.
first_match() {
  local file="$1" re="$2"
  [[ -f "$file" ]] || { printf '%s' "__MISSING__"; return; }
  local out
  out="$(RE="$re" perl -ne 'if (/$ENV{RE}/) { print "$1\n"; exit }' "$file" 2>/dev/null || true)"
  if [[ -z "$out" ]]; then printf '%s' "__UNRESOLVED__"; else printf '%s' "$out"; fi
}

# Per-type pin extraction. Each maps a declared file to the pinned tag (or sentinel).
extract_pin() {
  local type="$1" file="$2"
  case "$type" in
    cargo_tag)
      first_match "$file" '^\s*ncp-(?:core|zenoh)\b.*\bgit\s*=\s*"[^"]*/NCP".*\btag\s*=\s*"([^"]+)"' ;;
    cargo_lock)
      first_match "$file" 'git\+https://github\.com/[^/]+/NCP\?tag=([^#"]+)' ;;
    npm_tag|npm_lock)
      first_match "$file" '"\@[^"/]+/ncp"\s*:\s*"github:[^/]+/NCP#([^"]+)"' ;;
    mirror_ref)
      if [[ -f "$file" ]]; then local v; v="$(tr -d '[:space:]' < "$file")"; [[ -n "$v" ]] && printf '%s' "$v" || printf '%s' "__UNRESOLVED__"; else printf '%s' "__MISSING__"; fi ;;
    *)
      printf '%s' "__UNRESOLVED__" ;;
  esac
}

# ---------------------------------------------------------------------------
# Discover consumers: every sibling dir with a `.ncp-consumer` descriptor.
# ---------------------------------------------------------------------------
shopt -s nullglob
descriptors=("$BASE_DIR"/*/.ncp-consumer)
shopt -u nullglob

if [[ "${#descriptors[@]}" -eq 0 ]]; then
  echo "No consumers found under $BASE_DIR (no */.ncp-consumer descriptors)." >&2
  echo "A consumer registers by committing a .ncp-consumer file to its repo root;" >&2
  echo "see INTEGRATING.md §\"Registering a consumer\"." >&2
  exit 1
fi

for desc in "${descriptors[@]}"; do
  consumer_dir="$(cd "$(dirname "$desc")" && pwd)"
  consumer_name="$(basename "$consumer_dir")"
  while IFS= read -r line || [[ -n "$line" ]]; do
    line="${line%%#*}"                       # strip comments
    # shellcheck disable=SC2206
    fields=($line)                           # split on whitespace
    [[ "${#fields[@]}" -ge 2 ]] || continue
    type="${fields[0]}"; rel="${fields[1]}"
    # Only pin-bearing types are checked here; directives like `repin_cmd` (used by
    # repin-ncp.sh) are not pins and are skipped.
    case "$type" in
      cargo_tag|cargo_lock|npm_tag|npm_lock|mirror_ref) ;;
      *) continue ;;
    esac
    file="$consumer_dir/$rel"
    tag="$(extract_pin "$type" "$file")"
    add_row "$consumer_name/$rel ($type)" "$tag"
    # bun.lock also records the resolved commit (NCP#<sha>) — informational only.
    if [[ "$type" == "npm_lock" && -f "$file" ]]; then
      commit="$(RE='/NCP#([0-9a-f]{7,40})"' perl -ne 'if (/$ENV{RE}/) { print "$1\n"; exit }' "$file" 2>/dev/null || true)"
      [[ -n "$commit" ]] && NOTES+=("$consumer_name/$rel resolved commit = $commit (informational)")
    fi
  done < "$desc"
done

# ---------------------------------------------------------------------------
# Render the table.
# ---------------------------------------------------------------------------
render_tag() {
  case "$1" in
    __MISSING__)    printf '<file not found>' ;;
    __UNRESOLVED__) printf '<no pin matched>' ;;
    *)              printf '%s' "$1" ;;
  esac
}

maxw=0
for l in "${LABELS[@]}"; do (( ${#l} > maxw )) && maxw=${#l}; done

echo "NCP consumer pins (base-dir: $BASE_DIR)"
echo
printf '  %-*s  %s\n' "$maxw" "CONSUMER" "PIN"
printf '  %-*s  %s\n' "$maxw" "$(printf '%.0s-' $(seq 1 "$maxw"))" "----------------"
for i in "${!LABELS[@]}"; do
  printf '  %-*s  %s\n' "$maxw" "${LABELS[$i]}" "$(render_tag "${TAGS[$i]}")"
done
if [[ "${#NOTES[@]}" -gt 0 ]]; then
  echo
  for n in "${NOTES[@]}"; do printf '  note: %s\n' "$n"; done
fi
echo

# ---------------------------------------------------------------------------
# Verdict.
# ---------------------------------------------------------------------------
rc=0
problems=()

for i in "${!LABELS[@]}"; do
  case "${TAGS[$i]}" in
    __MISSING__)    problems+=("${LABELS[$i]}: file not found"); rc=1 ;;
    __UNRESOLVED__) problems+=("${LABELS[$i]}: file present but no NCP pin matched"); rc=1 ;;
  esac
done

concrete_tags=()
for t in "${TAGS[@]}"; do
  case "$t" in __MISSING__|__UNRESOLVED__) : ;; *) concrete_tags+=("$t") ;; esac
done

if [[ -n "$EXPECTED" ]]; then
  for i in "${!LABELS[@]}"; do
    case "${TAGS[$i]}" in
      __MISSING__|__UNRESOLVED__) : ;;
      *)
        if [[ "${TAGS[$i]}" != "$EXPECTED" ]]; then
          problems+=("${LABELS[$i]}: pinned to '${TAGS[$i]}', expected '$EXPECTED'"); rc=1
        fi ;;
    esac
  done
  [[ "$rc" -eq 0 ]] && echo "OK: all consumers pin NCP $EXPECTED"
else
  # Agreement mode: consumers must be WIRE-compatible, not on the identical patch tag.
  # Since v0.4, additive changes are non-breaking (a patch bump does NOT force a fleet
  # re-pin — see NCP VERSIONING.md), so v0.4.0 and v0.4.1 consumers interoperate. We
  # require all to share the same wire (vMAJOR.MINOR); a patch difference is a note,
  # not a failure. (Pass an explicit expected-tag for a strict coordinated-re-pin check.)
  wire_of() { sed -E 's/^v?([0-9]+\.[0-9]+).*$/\1/' <<<"$1"; }
  if [[ "${#concrete_tags[@]}" -gt 0 ]]; then
    uniq_tags="$(printf '%s\n' "${concrete_tags[@]}" | sort -u)"
    wires=(); for t in "${concrete_tags[@]}"; do wires+=("$(wire_of "$t")"); done
    uniq_wires="$(printf '%s\n' "${wires[@]}" | sort -u)"
    n_wires="$(printf '%s\n' "$uniq_wires" | grep -c . || true)"
    if [[ "$n_wires" -gt 1 ]]; then
      problems+=("consumers are on INCOMPATIBLE wires: $(printf '%s ' $uniq_wires) (tags: $(printf '%s ' $uniq_tags))"); rc=1
    elif [[ "$rc" -eq 0 ]]; then
      n_uniq="$(printf '%s\n' "$uniq_tags" | grep -c . || true)"
      if [[ "$n_uniq" -gt 1 ]]; then
        echo "OK: all consumers are wire-compatible (wire ${uniq_wires}); patch tags differ: $(printf '%s ' $uniq_tags)"
        echo "    (additive changes are non-breaking since v0.4 — a patch bump does not force a re-pin)"
      else
        echo "OK: all consumers pin NCP ${concrete_tags[0]}"
      fi
    fi
  fi
fi

if [[ "$rc" -ne 0 ]]; then
  echo "MISMATCH:" >&2
  for p in "${problems[@]}"; do echo "  - $p" >&2; done
  echo >&2
  echo "  Re-pin each consumer to the target tag (manifest AND lockfile move together)," >&2
  echo "  then re-run. Each consumer owns its re-pin recipe; see its .ncp-consumer and" >&2
  echo "  README. Mirror-style consumers run their own sync script (do NOT hand-edit a mirror)." >&2
fi

exit "$rc"

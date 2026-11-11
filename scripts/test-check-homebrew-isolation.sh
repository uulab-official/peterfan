#!/usr/bin/env bash
# Exercise the native-audit wrapper without touching the user's Homebrew.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
test_dir="$(mktemp -d "${TMPDIR:-/tmp}/peterfan-homebrew-test.XXXXXXXX")"
trap 'rm -rf -- "$test_dir"' EXIT
mkdir -p "$test_dir/bin" "$test_dir/user-config"
printf 'unchanged\n' > "$test_dir/user-config/sentinel"
export TEST_DIR="$test_dir"
cat > "$test_dir/bin/brew" <<'BREW'
#!/usr/bin/env bash
set -euo pipefail
[[ "$XDG_CONFIG_HOME" != "$TEST_DIR/user-config" ]]
[[ "${HOMEBREW_DEVELOPER:-}" == 1 ]]
printf '%s\n' "$*" >> "$TEST_DIR/calls"
case "$1" in
  tap) ;;
  tap-new) mkdir -p "$TEST_DIR/tap" ;;
  --repository) printf '%s\n' "$TEST_DIR/tap" ;;
  help) [[ "$2" == trust && "${TEST_LEGACY:-0}" != 1 ]] ;;
  trust) mkdir -p "$XDG_CONFIG_HOME/homebrew"; printf '%s\n' "$3" > "$XDG_CONFIG_HOME/homebrew/trust.json"; printf '%s\n' "$XDG_CONFIG_HOME" > "$TEST_DIR/audit-config" ;;
  audit)
    if [[ "${TEST_LEGACY:-0}" != 1 ]]; then
      [[ "$(cat "$XDG_CONFIG_HOME/homebrew/trust.json")/peterfan" == "$4" ]]
    fi
    [[ -f "$TEST_DIR/tap/Casks/peterfan.rb" && -f "$TEST_DIR/tap/Formula/peterfan.rb" ]]
    [[ "${TEST_FAIL_AUDIT:-0}" != 1 ]] ;;
  untap) ;;
  *) exit 2 ;;
esac
BREW
chmod +x "$test_dir/bin/brew"
export PATH="$test_dir/bin:$PATH"
export XDG_CONFIG_HOME="$test_dir/user-config"
"$ROOT/scripts/check-homebrew.sh"
grep -q '^trust --tap uulab/peterfan-audit-' "$test_dir/calls"
grep -q '^untap uulab/peterfan-audit-' "$test_dir/calls"
[[ "$(cat "$test_dir/user-config/sentinel")" == unchanged ]]
[[ ! -e "$test_dir/user-config/homebrew/trust.json" ]]
[[ ! -e "$(cat "$test_dir/audit-config")" ]]
: > "$test_dir/calls"
TEST_LEGACY=1 "$ROOT/scripts/check-homebrew.sh"
if grep -q '^trust ' "$test_dir/calls"; then
  echo 'error: legacy Homebrew unexpectedly invoked trust' >&2
  exit 1
fi
: > "$test_dir/calls"
if TEST_FAIL_AUDIT=1 "$ROOT/scripts/check-homebrew.sh"; then
  echo 'error: failed audit unexpectedly succeeded' >&2
  exit 1
fi
grep -q '^untap uulab/peterfan-audit-' "$test_dir/calls"
[[ "$(cat "$test_dir/user-config/sentinel")" == unchanged ]]
[[ ! -e "$(cat "$test_dir/audit-config")" ]]
echo 'PASS: Homebrew audit trust isolation and failure cleanup'

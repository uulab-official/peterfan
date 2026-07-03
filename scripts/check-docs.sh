#!/usr/bin/env bash
# Validate documentation assets and version references that commonly drift
# during fast release iterations.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FAILED=0

ok() {
  printf '  \033[32m✓\033[0m %s\n' "$1"
}

fail() {
  printf '  \033[31m✗\033[0m %s\n' "$1"
  FAILED=1
}

version=$(
  awk -F\" '
    $0 == "[workspace.package]" { in_workspace_package=1; next }
    /^\[/ && $0 != "[workspace.package]" { in_workspace_package=0 }
    in_workspace_package && $1 ~ /^version = / { print $2; exit }
  ' "$ROOT/Cargo.toml"
)

echo "PeterFan docs readiness"
echo

if [[ -n "$version" ]]; then
  ok "workspace version detected: v$version"
else
  fail "could not read workspace package version from Cargo.toml"
fi

if grep -q "^## \\[$version\\]" "$ROOT/CHANGELOG.md"; then
  ok "CHANGELOG.md has a v$version section"
else
  fail "CHANGELOG.md is missing a [${version}] section"
fi

if grep -q "v${version}" "$ROOT/README.ko.md"; then
  ok "README.ko.md mentions current version v$version"
else
  fail "README.ko.md does not mention current version v$version"
fi

if grep -R "v1\\.26\\.2\\b" "$ROOT/README.md" "$ROOT/README.ko.md" >/dev/null; then
  fail "README files still contain stale v1.26.2 references"
else
  ok "README files do not contain the old v1.26.2 reference"
fi

while IFS= read -r image_ref; do
  if [[ "$image_ref" =~ ^https?:// ]]; then
    ok "README external image skipped: $image_ref"
    continue
  fi
  image_path=${image_ref#./}
  if [[ -f "$ROOT/$image_path" ]]; then
    ok "README image exists: $image_ref"
  else
    fail "README image is missing: $image_ref"
  fi
done < <(
  grep -hoE '!\[[^]]*\]\((\./)?[^)]*\.(png|jpg|jpeg|webp|gif|svg)\)' \
    "$ROOT"/README*.md \
    | sed -E 's/^!\[[^]]*\]\((.*)\)$/\1/' \
    | sort -u
)

if [[ -f "$ROOT/docs/images/peterfan-readme-overview.png" ]]; then
  size=$(wc -c < "$ROOT/docs/images/peterfan-readme-overview.png" | tr -d ' ')
  if [[ "$size" -gt 0 ]]; then
    ok "README overview image is non-empty (${size} bytes)"
  else
    fail "README overview image is empty"
  fi
else
  fail "README overview image is missing"
fi

if [[ -x "$ROOT/scripts/render-readme-overview.swift" ]]; then
  ok "README overview renderer exists and is executable"
elif [[ -f "$ROOT/scripts/render-readme-overview.swift" ]]; then
  fail "README overview renderer exists but is not executable"
else
  fail "README overview renderer is missing"
fi

echo
if [[ "$FAILED" -eq 0 ]]; then
  ok "docs are ready"
else
  fail "docs need attention"
fi

exit "$FAILED"

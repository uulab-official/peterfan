#!/usr/bin/env bash
# Load repo-local release/signing settings. This file is meant to be sourced.

PETERFAN_REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ -f "$PETERFAN_REPO_ROOT/.env" ]]; then
  set -a
  # shellcheck disable=SC1091
  source "$PETERFAN_REPO_ROOT/.env"
  set +a
fi

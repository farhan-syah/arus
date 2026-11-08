#!/bin/sh
set -eu

[ "$#" -le 1 ] || {
    printf '%s\n' 'Usage: sh scripts/check.sh [--offline|--online]' >&2
    exit 2
}

case "${1:-}" in
    ""|--offline) set -- --locked --offline ;;
    --online) set -- --locked ;;
    *) printf '%s\n' 'Usage: sh scripts/check.sh [--offline|--online]' >&2; exit 2 ;;
esac

cd "$(CDPATH= cd "$(dirname "$0")/.." && pwd)"
python3 scripts/check_repository.py
cargo fmt --all -- --check
cargo check --workspace --all-targets "$@"
cargo test --workspace --all-features "$@"
cargo clippy --workspace --all-targets --all-features "$@" -- -D warnings

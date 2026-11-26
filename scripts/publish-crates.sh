#!/usr/bin/env bash
set -euo pipefail

# Publish all 20 ARUS workspace crates to crates.io in topological dependency order.
# Usage:
#   ./scripts/publish-crates.sh --dry-run   # Dry-run leaf crates (cannot dry-run downstream before upstream is live)
#   ./scripts/publish-crates.sh             # Publish all 20 crates (requires `cargo login` first)

DRY_RUN=0
DELAY_SECS="${ARUS_PUBLISH_DELAY_SECS:-20}"

for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN=1 ;;
    --delay=*) DELAY_SECS="${arg#*=}" ;;
    *) echo "Unknown argument: $arg" >&2; exit 2 ;;
  esac
done

CRATES=(
  arus-domain
  arus-media
  arus-api-types
  arus-assets
  arus-geo
  arus-model-api
  arus-storage
  arus-web
  arus-evaluation
  arus-hydrology
  arus-ingest
  arus-jobs
  arus-ml
  arus-scenarios
  arus-cli
  arus-forecasting
  arus-hydraulics
  arus-optimisation
  arus-api
  arus-worker
)

if [[ "$DRY_RUN" -eq 1 ]]; then
  echo "Dry-run verifying root leaf crates (arus-domain, arus-media)..."
  cargo publish --dry-run -p arus-domain
  cargo publish --dry-run -p arus-media
  echo "Dry-run passed. Run without --dry-run after 'cargo login' to publish all ${#CRATES[@]} crates."
  exit 0
fi

for i in "${!CRATES[@]}"; do
  crate="${CRATES[$i]}"
  echo "=== [$((i + 1))/${#CRATES[@]}] Publishing ${crate} ==="
  cargo publish -p "$crate"
  if [[ $((i + 1)) -lt ${#CRATES[@]} ]]; then
    echo "Waiting ${DELAY_SECS}s for crates.io index update..."
    sleep "$DELAY_SECS"
  fi
done

echo "All ${#CRATES[@]} ARUS crates published successfully."

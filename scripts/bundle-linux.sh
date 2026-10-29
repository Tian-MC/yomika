#!/usr/bin/env bash
# Build once, then package prereleases with format-specific version metadata.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
format=${1:-all}
case "$format" in
  all | rpm) ;;
  *) echo "Usage: $0 [all|rpm]" >&2; exit 2 ;;
esac
npm run tauri -- build --ci --no-bundle
if [[ $format == all ]]; then
  npm run tauri -- bundle --ci --bundles appimage,deb
fi
rpm_config=$(python3 scripts/rpm_config.py)
npm run tauri -- bundle --ci --bundles rpm --config "$rpm_config"

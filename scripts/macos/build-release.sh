#!/usr/bin/env bash
# Build the macOS (Apple Silicon) release bundle: ZeppBridge.app + .dmg.
#
# Usage:
#   scripts/macos/build-release.sh          # local verification build
#   TAURI_SIGNING_PRIVATE_KEY=... \
#   TAURI_SIGNING_PRIVATE_KEY_PASSWORD=... \
#   scripts/macos/build-release.sh          # publishable build (updater-signed)
#
# Without a signing key the updater artifacts are skipped, so a local build
# never fails just because the private key is missing.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

if ! command -v cargo >/dev/null 2>&1; then
  echo "Error: cargo not found. Install Rust first (https://rustup.rs)." >&2
  exit 1
fi
if ! command -v npm >/dev/null 2>&1; then
  echo "Error: npm not found." >&2
  exit 1
fi

echo "==> Frontend build"
npm run build

echo "==> Icon check"
if ! command -v python3 >/dev/null 2>&1 || ! python3 -c 'import PIL' 2>/dev/null; then
  echo "(Skipping icons:verify: it needs python3 + Pillow)"
else
  npm run icons:verify
fi

echo "==> Rust format and tests"
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo test --manifest-path src-tauri/Cargo.toml --locked

BUNDLES="app,dmg"
EXTRA_ARGS=()
if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" && -z "${TAURI_SIGNING_PRIVATE_KEY_PATH:-}" ]]; then
  echo "==> No updater signing key found. Skipping updater artifacts (local check build)."
  EXTRA_ARGS+=(--config '{"bundle":{"createUpdaterArtifacts":false}}')
else
  echo "==> Updater signing key found. Building release artifacts."
fi

echo "==> Tauri bundle (aarch64-apple-darwin)"
npx tauri build --bundles "$BUNDLES" "${EXTRA_ARGS[@]}"

echo
echo "Build done. Artifacts:"
echo "  app: src-tauri/target/release/bundle/macos/ZeppBridge.app"
echo "  dmg: src-tauri/target/release/bundle/dmg/ZeppBridge_*.dmg"

#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC_TAURI_DIR="$ROOT_DIR/src-tauri"
TAURI_CONF="$SRC_TAURI_DIR/tauri.conf.json"
PRODUCT_NAME="$(sed -n 's/.*"productName"[[:space:]]*:[[:space:]]*"\(.*\)".*/\1/p' "$TAURI_CONF" | head -n1)"
VERSION="$(sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\(.*\)".*/\1/p' "$TAURI_CONF" | head -n1)"
ARCH="$(uname -m)"
BUILD_ROOT="$ROOT_DIR/.build-cache.noindex/tauri"
APP_BUNDLE="$BUILD_ROOT/release/bundle/macos/${PRODUCT_NAME}.app"
DMG_DIR="$SRC_TAURI_DIR/target/release/bundle/dmg"
OUTPUT_DMG="$DMG_DIR/${PRODUCT_NAME}_${VERSION}_${ARCH}.dmg"
TMP_DIR="$(mktemp -d /tmp/send2boox-dmg.XXXXXX)"

if [[ -z "$PRODUCT_NAME" || -z "$VERSION" ]]; then
  echo "failed to read productName/version from $TAURI_CONF" >&2
  exit 1
fi

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

cd "$SRC_TAURI_DIR"
mkdir -p "$BUILD_ROOT" "$SRC_TAURI_DIR/target" "$SRC_TAURI_DIR/target/release" "$SRC_TAURI_DIR/target/release/bundle"
touch "$BUILD_ROOT/.metadata_never_index"
touch "$SRC_TAURI_DIR/target/.metadata_never_index"
touch "$SRC_TAURI_DIR/target/release/.metadata_never_index"
touch "$SRC_TAURI_DIR/target/release/bundle/.metadata_never_index"

CARGO_TARGET_DIR="$BUILD_ROOT" cargo tauri build --bundles app

mkdir -p "$DMG_DIR"
rm -f "$OUTPUT_DMG"

hdiutil detach "/Volumes/${PRODUCT_NAME}" >/dev/null 2>&1 || true
hdiutil detach "/Volumes/${PRODUCT_NAME} 1" >/dev/null 2>&1 || true

ditto "$APP_BUNDLE" "$TMP_DIR/${PRODUCT_NAME}.app"
ln -s /Applications "$TMP_DIR/Applications"

hdiutil create \
  -volname "$PRODUCT_NAME" \
  -srcfolder "$TMP_DIR" \
  -ov \
  -format UDZO \
  "$OUTPUT_DMG"

echo "$OUTPUT_DMG"

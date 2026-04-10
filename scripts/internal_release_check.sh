#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
TAURI_DIR="$ROOT_DIR/src-tauri"

echo "[1/4] cargo fmt --check"
cd "$TAURI_DIR"
cargo fmt --all -- --check

echo "[2/4] cargo clippy"
cargo clippy -- -D warnings

echo "[3/4] cargo test"
cargo test

echo "[4/4] cargo build --release"
cargo build --release

if [ -x "$HOME/.cargo/bin/cargo-tauri" ]; then
  echo "[extra] cargo tauri build (app bundle)"
  "$HOME/.cargo/bin/cargo-tauri" build
fi

cat <<'EOF'

Automated checks completed.
Manual smoke checklist (10 minutes):
- Launch app and confirm default page is Recent Notes.
- Single-click tray icon opens Recent Notes.
- Double-click tray icon opens Upload page.
- Menu toggles between Recent Notes and Upload page.
- Close window hides to tray and app remains running.
- Toggle autostart ON/OFF from tray and verify label updates.

EOF

# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog and this project currently follows Semantic Versioning.

## [0.1.0] - 2026-04-10

### Added
- Tauri desktop shell for Send2Boox Recent Notes and Upload pages.
- Tray interactions: open pages, toggle autostart, and graceful quit.
- URL navigation allowlist for `https://send2boox.com` and `https://www.send2boox.com`.
- Release readiness script: `scripts/internal_release_check.sh`.
- Unit tests for URL routing, tray action mapping, and autostart labels.

### Changed
- Enabled bundle configuration for internal release artifacts.
- Added stricter security settings in Tauri config.
- Replaced placeholder app icon files with a generated multi-size icon set.


# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog and this project currently follows Semantic Versioning.

## [0.2.0] - 2026-04-13

### Added
- Rust 原生官网登录回环服务，使用默认浏览器完成二维码授权并回流到桌面端。
- 分模块后端结构：`app`、`auth`、`api`、`dashboard`、`push`、`device`、`state`、`util`。
- 仪表盘原生快照聚合，覆盖用户、存储、阅读指标、设备、推送队列与上传状态。
- neocloud 远程推送队列访问、原生重推/删除、局域网设备识别与互传地址保护。

### Changed
- 仪表盘升级为唯一主界面，移除了官网主页面窗口与相关菜单入口。
- 托盘左键行为保持为显示/隐藏本地仪表盘，窗口关闭默认隐藏到托盘。
- 登录、上传、推送和设备能力全部改为 Rust 原生 API 流程，对齐官网网页接口语义。
- 前端 `dist/` 仅作为本地静态仪表盘，不再承担网页登录桥接职责。

### Removed
- 隐藏 WebView、页面注入 JS、浏览器会话 fetch 和 PouchDB 本地依赖。

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

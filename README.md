# Send2Boox Tauri Client

This desktop client wraps Send2Boox with:

- Main page: `https://send2boox.com/#/note/recentNote`
- Upload page: `https://send2boox.com/#/push/file`
- Single main window that can switch between recent notes/upload page
- System tray menu to open either page
- Tray double-click opens the upload page directly
- Close-to-tray behavior (window close hides app instead of quitting)
- Auto start on login (enabled by default on first run, can be toggled in tray)
- Navigation allowlist: only `https://send2boox.com` and `https://www.send2boox.com`
- Internal-release checks script for quick regression validation

## Run

```bash
cd /Volumes/DataCenter_01/boox-tauri/src-tauri
cargo run
```

## Internal release checks

```bash
cd /Volumes/DataCenter_01/boox-tauri
./scripts/internal_release_check.sh
```

## Tray actions

- `登录并授权`: open login page and complete account sign-in; app caches session via callback
- `打开主页面`: show or create the recent notes window
- `托盘上传（静默）`: open native file picker and upload in background via API/OSS (without opening main page)
- `上传诊断`: check login token/cookie/browser-session auth, `/api/1/users/me`, `/api/1/config/buckets`, `/api/1/config/stss`
- `日历统计: ...`: show extracted stats from `https://send2boox.com/#/calendar`
- `刷新日历统计`: refresh stats in background without opening the page
- `开机自启动: 开/关`: toggle auto start on login
- `退出`: quit the app

## In-app menu

- `页面 -> 最近笔记`
- `页面 -> 上传文件`

## Build artifacts

To produce internal-release APP bundle (macOS), install Tauri CLI first:

```bash
cargo install tauri-cli --version "^1.6"
cd /Volumes/DataCenter_01/boox-tauri/src-tauri
cargo tauri build
```

Default bundle target is `app` (configured for internal gray release).

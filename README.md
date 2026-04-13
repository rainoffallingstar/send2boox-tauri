# Send2Boox Desktop

Send2Boox 桌面端现已重构为“本地仪表盘唯一主界面 + 托盘入口 + Rust 原生 API 服务层”的架构。

## 当前架构

- 主窗口只保留本地仪表盘，不再创建官网主页面 WebView。
- 托盘左键仍然用于显示/隐藏仪表盘。
- 关闭主窗口默认隐藏到托盘，不退出进程。
- 登录授权改为默认浏览器打开本地回环登录页，再回流到桌面端。
- 仪表盘、上传、推送队列、设备列表、阅读指标全部走 Rust 原生接口。
- 已移除隐藏 WebView、页面注入 JS、浏览器会话 fetch、PouchDB 本地依赖。

## 模块划分

- `src-tauri/src/app.rs`: 主窗口、托盘、左键唤起、关闭隐藏、自启动。
- `src-tauri/src/auth.rs`: 默认浏览器登录、本地回环监听、二维码授权回流。
- `src-tauri/src/api.rs`: 官网网页 API Rust 封装、认证头、neocloud 访问、OSS/STS 支撑。
- `src-tauri/src/dashboard.rs`: 仪表盘快照聚合与命令入口。
- `src-tauri/src/push.rs`: 上传、重推、删除、上传进度状态机。
- `src-tauri/src/device.rs`: 设备列表、局域网识别、互传地址规范化。
- `src-tauri/src/state.rs`: 登录态、缓存、上传运行态。

## 登录与授权

- 点击仪表盘或托盘中的“登录并授权”后，桌面端会启动本地回环端口并用默认浏览器打开登录页。
- 浏览器页内使用官方二维码登录接口完成授权，成功后把 token 回流给桌面端。
- 登录完成后自动回到本地仪表盘，不再依赖网页标题、hash、cookie 抓取或隐藏页面桥接。

## 仪表盘能力

- 用户信息、云空间、阅读指标、设备列表、互动文件列表统一从 Rust 聚合快照读取。
- 上传保留 Rust 直传 OSS/STS 方案，支持进度、速度、ETA 和结果提示。
- 推送列表支持原生 `重推`、`删除`，不再依赖网页端本地数据库。
- 设备互传入口只允许合法局域网地址，并通过系统默认浏览器打开。

## 运行与构建

```bash
cd /Volumes/DataCenter_01/boox-tauri/src-tauri
cargo run
```

```bash
cd /Volumes/DataCenter_01/boox-tauri
./scripts/internal_release_check.sh
```

构建桌面包：

```bash
cargo install tauri-cli --version "^1.6"
cd /Volumes/DataCenter_01/boox-tauri/src-tauri
cargo tauri build
```

## 托盘菜单

- `登录并授权`
- `上传文件`
- `刷新仪表盘`
- `开机自启动: 开/关`
- `退出`

## 已验证方向

- 仪表盘是唯一主界面。
- 托盘左键显示/隐藏仍保留。
- 默认浏览器登录链路可触发本地回流。
- 仪表盘命令不再依赖官网主页面窗口存在。

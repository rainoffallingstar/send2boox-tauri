#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use auto_launch::{AutoLaunch, AutoLaunchBuilder};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use chrono::Local;
use hmac::{Hmac, Mac};
use reqwest::blocking::Client;
use reqwest::header::{AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, COOKIE};
use serde_json::{json, Value};
use sha1::Sha1;
use std::{
    collections::HashMap,
    fmt::Display,
    fs::{self, File},
    io::Read,
    net::{IpAddr, Ipv4Addr, UdpSocket},
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
    thread,
    time::{Duration, Instant},
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{
    api::dialog::{blocking::message, FileDialogBuilder},
    CustomMenuItem, Manager, Menu, Submenu, SystemTray, SystemTrayEvent, SystemTrayMenu,
    SystemTrayMenuItem, Window, WindowBuilder, WindowEvent, WindowUrl,
};
use uuid::Uuid;

const MAIN_LABEL: &str = "main";
const DASHBOARD_LABEL: &str = "tray_dashboard";
const MAIN_URL: &str = "https://send2boox.com/#/note/recentNote";
const UPLOAD_URL: &str = "https://send2boox.com/#/push/file";
const LOGIN_URL: &str = "https://send2boox.com/#/login";
const CALENDAR_URL: &str = "https://send2boox.com/#/calendar";
const DASHBOARD_HTML: &str = "dashboard.html";
const TOGGLE_AUTOSTART_ID: &str = "toggle_autostart";
const CALENDAR_STATS_ID: &str = "calendar_stats_status";
const UPLOAD_PROGRESS_ID: &str = "upload_progress_status";
const REFRESH_CALENDAR_ID: &str = "refresh_calendar_stats";
const AUTOSTART_MARKER: &str = "autostart_initialized";
const APP_ALERT_TITLE: &str = "Send2Boox Desktop";
const ALLOWED_HOST_1: &str = "send2boox.com";
const ALLOWED_HOST_2: &str = "www.send2boox.com";
#[cfg(test)]
const UPLOAD_HASH: &str = "#/push/file";
const CALENDAR_HASH: &str = "#/calendar";
const CALENDAR_LABEL: &str = "calendar_stats";
const CALENDAR_STATS_TITLE_PREFIX: &str = "S2B_CAL_STATS::";
const CALENDAR_STATS_QUERY_KEY: &str = "trayStats";
const UPLOAD_DIAG_ID: &str = "upload_diag";
const AUTH_TITLE_PREFIX: &str = "S2B_AUTH_STATE::";
const BROWSER_FETCH_PREFIX: &str = "S2B_FETCH_RESULT::";
const AUTH_STATE_QUERY_KEY: &str = "trayAuth";
const FETCH_RESULT_QUERY_KEY: &str = "trayFetch";
const MAX_SINGLE_UPLOAD_BYTES: u64 = 200 * 1024 * 1024;
const DEFAULT_BUCKET_KEY: &str = "onyx-cloud";
const DASHBOARD_WIDTH: f64 = 430.0;
const DASHBOARD_HEIGHT: f64 = 820.0;
const DASHBOARD_GAP: f64 = -6.0;
const DASHBOARD_PUSH_QUEUE_LIMIT: usize = 30;
const DASHBOARD_CACHE_MAX_AGE_MS: u128 = 5_000;
const AUTH_CACHE_FILE: &str = "auth_cache.json";
const AUTH_TOKEN_CAPTURE_SCRIPT: &str = r#"
(() => {
  try {
    const readToken = () => {
      const keys = ['token', 'access_token', 'TOKEN'];
      for (const key of keys) {
        const raw = localStorage.getItem(key) ?? sessionStorage.getItem(key);
        if (!raw) continue;
        try {
          const parsed = JSON.parse(raw);
          if (typeof parsed === 'string' && parsed.trim()) return parsed.trim();
        } catch (_) {
          if (typeof raw === 'string' && raw.trim()) return raw.trim();
        }
      }
      return '';
    };

    const payload = {
      token: readToken(),
      cookie: document.cookie || ''
    };
    const encoded = btoa(unescape(encodeURIComponent(JSON.stringify(payload))));
    const pushTitle = () => { document.title = 'S2B_AUTH_STATE::' + encoded; };
    const pushHash = () => {
      if (!payload.token && !payload.cookie) return;
      const hash = window.location.hash || '#/';
      const [route, query = ''] = hash.split('?');
      const params = new URLSearchParams(query);
      params.set('trayAuth', encoded);
      const nextHash = route + '?' + params.toString();
      if (window.location.hash !== nextHash) window.location.hash = nextHash;
    };
    pushTitle();
    pushHash();
    let count = 0;
    const timer = setInterval(() => {
      pushTitle();
      pushHash();
      if (count++ > 50) clearInterval(timer);
    }, 100);
  } catch (_) {}
})();
"#;
const CALENDAR_STATS_SCRIPT: &str = r#"
(() => {
  const key = 'trayStats';
  const normalize = (text) => text.replace(/\s+/g, ' ').replace(/[：:]\s*/g, ':').trim();
  const collect = () => {
    const selectors = [
      '.el-statistic__content-value',
      '.statistic-value',
      '.stats-item',
      '.summary-item',
      '.calendar-stat',
      '[class*="stat"]',
      '[class*="summary"]'
    ];
    const tokens = [];
    for (const selector of selectors) {
      for (const node of document.querySelectorAll(selector)) {
        const text = normalize(node.textContent || '');
        if (!text) continue;
        if (!/[0-9]/.test(text)) continue;
        if (text.length > 28) continue;
        tokens.push(text);
      }
    }

    if (tokens.length === 0) {
      const body = normalize(document.body?.innerText || '');
      const quick = body.match(/([^\s]{1,8}[:：]?\s*\d{1,6})/g) || [];
      tokens.push(...quick.map(normalize));
    }

    const unique = [...new Set(tokens)].slice(0, 3);
    return unique.length > 0 ? unique.join(' | ') : '';
  };

  const apply = () => {
    const stats = collect();
    const value = stats || '暂无统计';
    const hash = '#/calendar?' + key + '=' + encodeURIComponent(value);
    if (window.location.hash !== hash) {
      window.location.hash = hash;
    }
    return true;
  };

  if (apply()) return;
  let retries = 0;
  const timer = setInterval(() => {
    if (apply() || retries++ > 20) {
      if (retries > 20) document.title = prefix + '暂无统计';
      clearInterval(timer);
    }
  }, 400);
})();
"#;

#[derive(Default)]
struct RuntimeState {
    upload_in_progress: Mutex<bool>,
    cached_auth_state: Mutex<CachedAuthState>,
    upload_runtime_state: Mutex<UploadRuntimeState>,
    login_authorizing: Mutex<bool>,
    last_tray_anchor: Mutex<Option<(f64, f64, f64, f64)>>,
    dashboard_cache: Mutex<Option<DashboardSnapshot>>,
}

#[derive(Debug, Clone, Default)]
struct CachedAuthState {
    token: Option<String>,
    cookie: Option<String>,
}

#[derive(Debug, Clone)]
struct UploadRuntimeState {
    in_progress: bool,
    status_text: String,
    last_error: Option<String>,
    current_file: Option<String>,
    bytes_sent: Option<u64>,
    bytes_total: Option<u64>,
    progress_percent: Option<f64>,
    speed_bps: Option<f64>,
    eta_seconds: Option<f64>,
    updated_ms: u128,
}

impl Default for UploadRuntimeState {
    fn default() -> Self {
        Self {
            in_progress: false,
            status_text: "上传进度: 空闲".to_string(),
            last_error: None,
            current_file: None,
            bytes_sent: None,
            bytes_total: None,
            progress_percent: None,
            speed_bps: None,
            eta_seconds: None,
            updated_ms: 0,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct PersistedAuthState {
    token: Option<String>,
    cookie: Option<String>,
    updated_ms: Option<u128>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppPage {
    Login,
    Recent,
    Upload,
}

impl AppPage {
    fn title(self) -> &'static str {
        match self {
            Self::Login => "Send2Boox - Login",
            Self::Recent => "Send2Boox - Recent Notes",
            Self::Upload => "Send2Boox - Upload File",
        }
    }

    fn url(self) -> &'static str {
        match self {
            Self::Login => LOGIN_URL,
            Self::Recent => MAIN_URL,
            Self::Upload => UPLOAD_URL,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrayAction {
    OpenLogin,
    OpenMain,
    OpenUpload,
    UploadDiag,
    RefreshCalendarStats,
    ToggleAutostart,
    Quit,
    Ignore,
}

#[derive(serde::Serialize)]
struct AppStatus {
    app: String,
    version: String,
    unix_ms: u128,
}

#[tauri::command]
fn app_status(app: tauri::AppHandle) -> AppStatus {
    let unix_ms = unix_ms_now();

    AppStatus {
        app: app.package_info().name.clone(),
        version: app.package_info().version.to_string(),
        unix_ms,
    }
}

#[tauri::command]
async fn dashboard_snapshot(app: tauri::AppHandle) -> Result<DashboardSnapshot, String> {
    if let Some(snapshot) = get_dashboard_cache(&app, DASHBOARD_CACHE_MAX_AGE_MS) {
        return Ok(snapshot);
    }
    let app_for_task = app.clone();
    let snapshot =
        tauri::async_runtime::spawn_blocking(move || build_dashboard_snapshot(&app_for_task))
            .await
            .map_err(|err| err.to_string())?;
    set_dashboard_cache(&app, snapshot.clone());
    Ok(snapshot)
}

#[tauri::command]
async fn dashboard_refresh(app: tauri::AppHandle) -> Result<DashboardSnapshot, String> {
    let app_for_task = app.clone();
    let snapshot =
        tauri::async_runtime::spawn_blocking(move || build_dashboard_snapshot(&app_for_task))
            .await
            .map_err(|err| err.to_string())?;
    set_dashboard_cache(&app, snapshot.clone());
    Ok(snapshot)
}

#[tauri::command]
fn dashboard_open_main(app: tauri::AppHandle, page: Option<String>) -> Result<(), String> {
    let target = match page.as_deref() {
        Some("upload") => AppPage::Upload,
        Some("login") => AppPage::Login,
        _ => AppPage::Recent,
    };
    hide_dashboard_window(&app);
    open_or_switch_main(&app, target);
    Ok(())
}

#[tauri::command]
fn dashboard_login_authorize(app: tauri::AppHandle) -> Result<(), String> {
    hide_dashboard_window(&app);
    start_login_flow(&app);
    Ok(())
}

#[tauri::command]
fn dashboard_upload_pick_and_send(app: tauri::AppHandle) -> Result<(), String> {
    trigger_upload_from_tray(&app);
    Ok(())
}

#[tauri::command]
async fn dashboard_push_resend(
    app: tauri::AppHandle,
    id: String,
) -> Result<DashboardSnapshot, String> {
    let app_for_task = app.clone();
    let snapshot = tauri::async_runtime::spawn_blocking(move || {
        dashboard_push_resend_inner(&app_for_task, &id)?;
        Ok::<DashboardSnapshot, String>(build_dashboard_snapshot(&app_for_task))
    })
    .await
    .map_err(|err| err.to_string())??;
    set_dashboard_cache(&app, snapshot.clone());
    Ok(snapshot)
}

#[tauri::command]
async fn dashboard_push_delete(
    app: tauri::AppHandle,
    id: String,
) -> Result<DashboardSnapshot, String> {
    let app_for_task = app.clone();
    let id_for_task = id.clone();
    tauri::async_runtime::spawn_blocking(move || dashboard_push_delete_inner(&app_for_task, &id_for_task))
    .await
    .map_err(|err| err.to_string())??;

    if let Some(snapshot) = update_dashboard_cache_after_delete(&app, &id) {
        return Ok(snapshot);
    }

    let app_for_refresh = app.clone();
    let snapshot =
        tauri::async_runtime::spawn_blocking(move || build_dashboard_snapshot(&app_for_refresh))
            .await
            .map_err(|err| err.to_string())?;
    set_dashboard_cache(&app, snapshot.clone());
    Ok(snapshot)
}

#[tauri::command]
fn dashboard_hide(app: tauri::AppHandle) -> Result<(), String> {
    hide_dashboard_window(&app);
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardAuth {
    authorized: bool,
    source: String,
    message: String,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardProfile {
    uid: String,
    nickname: Option<String>,
    avatar_url: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardStorage {
    used: Option<u64>,
    limit: Option<u64>,
    percent: Option<f64>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardDevice {
    id: Option<String>,
    model: Option<String>,
    mac_address: Option<String>,
    ip_address: Option<String>,
    login_status: Option<String>,
    latest_login_time: Option<String>,
    latest_logout_time: Option<String>,
    locked: Option<bool>,
    same_lan: bool,
    lan_ip: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardPushItem {
    id: String,
    rev: Option<String>,
    name: String,
    size: Option<u64>,
    updated_at: Option<i64>,
    format: Option<String>,
    resource_key: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardCalendarMetrics {
    reading_info: Value,
    read_time_week: Value,
    day_read_today: Value,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardUploadState {
    in_progress: bool,
    status_text: String,
    last_error: Option<String>,
    current_file: Option<String>,
    bytes_sent: Option<u64>,
    bytes_total: Option<u64>,
    progress_percent: Option<f64>,
    speed_bps: Option<f64>,
    eta_seconds: Option<f64>,
    updated_ms: u128,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardSnapshot {
    auth: DashboardAuth,
    profile: Option<DashboardProfile>,
    storage: DashboardStorage,
    devices: Vec<DashboardDevice>,
    push_queue: Vec<DashboardPushItem>,
    calendar_metrics: DashboardCalendarMetrics,
    upload: DashboardUploadState,
    fetched_at_ms: u128,
}

fn log_error(context: &str, err: &dyn Display) {
    eprintln!("[send2boox][error] {context}: {err}");
}

fn unix_ms_now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn get_upload_runtime_state(app: &tauri::AppHandle) -> UploadRuntimeState {
    match app.state::<RuntimeState>().upload_runtime_state.lock() {
        Ok(state) => state.clone(),
        Err(err) => {
            eprintln!("[send2boox][warn] 读取上传状态失败: {err}");
            UploadRuntimeState::default()
        }
    }
}

fn update_upload_runtime_state<F>(app: &tauri::AppHandle, mutator: F)
where
    F: FnOnce(&mut UploadRuntimeState),
{
    match app.state::<RuntimeState>().upload_runtime_state.lock() {
        Ok(mut state) => {
            mutator(&mut state);
            state.updated_ms = unix_ms_now();
        }
        Err(err) => eprintln!("[send2boox][warn] 更新上传状态失败: {err}"),
    }
}

fn report_error(app: &tauri::AppHandle, context: &str, err: &dyn Display) {
    log_error(context, err);
    let body = format!("{context}\n{err}");
    message(None::<&tauri::Window>, APP_ALERT_TITLE, body);
    sync_autostart_menu_title(app);
}

fn autostart_menu_title(enabled: bool) -> &'static str {
    if enabled {
        "开机自启动: 开"
    } else {
        "开机自启动: 关"
    }
}

fn escape_js(text: &str) -> String {
    text.replace('\\', "\\\\").replace('\'', "\\'")
}

fn redirect_script(url: &str) -> String {
    let safe_url = escape_js(url);
    format!(
        "if (window.location.href !== '{0}') window.location.replace('{0}');",
        safe_url
    )
}

fn is_allowed_navigation(url: &str) -> bool {
    let parsed = match url.parse::<tauri::Url>() {
        Ok(value) => value,
        Err(_) => return false,
    };

    parsed.scheme() == "https"
        && matches!(
            parsed.host_str(),
            Some(ALLOWED_HOST_1) | Some(ALLOWED_HOST_2)
        )
}

fn tray_action_from_id(id: &str) -> TrayAction {
    match id {
        "open_login" => TrayAction::OpenLogin,
        "open_main" => TrayAction::OpenMain,
        "open_upload" => TrayAction::OpenUpload,
        UPLOAD_DIAG_ID => TrayAction::UploadDiag,
        REFRESH_CALENDAR_ID => TrayAction::RefreshCalendarStats,
        TOGGLE_AUTOSTART_ID => TrayAction::ToggleAutostart,
        "quit" => TrayAction::Quit,
        _ => TrayAction::Ignore,
    }
}

#[cfg(test)]
fn is_upload_url(url: &str) -> bool {
    url.contains(UPLOAD_HASH)
}

fn is_calendar_url(url: &str) -> bool {
    url.contains(CALENDAR_HASH)
}

fn parse_calendar_stats_title(title: &str) -> Option<String> {
    title
        .strip_prefix(CALENDAR_STATS_TITLE_PREFIX)
        .map(|raw| raw.trim())
        .filter(|value| !value.is_empty())
        .map(|value| format!("日历统计: {value}"))
}

fn parse_calendar_stats_from_url(url: &str) -> Option<String> {
    let value = get_hash_query_value(url, CALENDAR_STATS_QUERY_KEY)?;
    if value.trim().is_empty() {
        return None;
    }
    Some(format!("日历统计: {value}"))
}

fn get_hash_query_value(url: &str, key: &str) -> Option<String> {
    let parsed = url.parse::<tauri::Url>().ok()?;
    let fragment = parsed.fragment()?;
    let (_, query) = fragment.split_once('?')?;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if k == key {
            let decoded = urlencoding::decode(v).ok()?.trim().to_string();
            if !decoded.is_empty() {
                return Some(decoded);
            }
        }
    }
    None
}

fn truncate_stats_label(value: &str) -> String {
    let mut out = String::new();
    for (idx, ch) in value.chars().enumerate() {
        if idx >= 30 {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

fn set_calendar_stats_label(app: &tauri::AppHandle, text: &str) {
    let value = truncate_stats_label(text);
    if let Err(err) = app
        .tray_handle()
        .get_item(CALENDAR_STATS_ID)
        .set_title(value)
    {
        log_error("更新日历统计托盘文案失败", &err);
    }
}

fn set_upload_progress_label(app: &tauri::AppHandle, text: &str) {
    let value = truncate_stats_label(text);
    if let Err(err) = app
        .tray_handle()
        .get_item(UPLOAD_PROGRESS_ID)
        .set_title(value)
    {
        log_error("更新上传进度托盘文案失败", &err);
    }
    let text_owned = text.to_string();
    update_upload_runtime_state(app, move |state| {
        state.status_text = text_owned;
    });
}

fn clear_upload_transfer_metrics(app: &tauri::AppHandle) {
    update_upload_runtime_state(app, |state| {
        state.current_file = None;
        state.bytes_sent = None;
        state.bytes_total = None;
        state.progress_percent = None;
        state.speed_bps = None;
        state.eta_seconds = None;
    });
}

fn update_upload_transfer_metrics(
    app: &tauri::AppHandle,
    seq: usize,
    total: usize,
    file_name: &str,
    bytes_sent: u64,
    bytes_total: u64,
    speed_bps: f64,
) {
    let percent = if bytes_total > 0 {
        ((bytes_sent as f64 / bytes_total as f64) * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };
    let eta_seconds = if speed_bps > 0.1 && bytes_total > bytes_sent {
        Some((bytes_total.saturating_sub(bytes_sent)) as f64 / speed_bps)
    } else {
        None
    };
    let short_name = shorten_for_ui(file_name, 26);
    set_upload_progress_label(
        app,
        &format!("上传进度: {seq}/{total} {percent:.1}% {short_name}"),
    );
    update_upload_runtime_state(app, |state| {
        state.current_file = Some(file_name.to_string());
        state.bytes_sent = Some(bytes_sent);
        state.bytes_total = Some(bytes_total);
        state.progress_percent = Some(percent);
        state.speed_bps = Some(speed_bps.max(0.0));
        state.eta_seconds = eta_seconds;
    });
}

fn try_begin_upload_task(app: &tauri::AppHandle) -> bool {
    match app.state::<RuntimeState>().upload_in_progress.lock() {
        Ok(mut in_progress) => {
            if *in_progress {
                return false;
            }
            *in_progress = true;
            update_upload_runtime_state(app, |state| {
                state.in_progress = true;
                state.last_error = None;
                state.current_file = None;
                state.bytes_sent = None;
                state.bytes_total = None;
                state.progress_percent = None;
                state.speed_bps = None;
                state.eta_seconds = None;
            });
            true
        }
        Err(err) => {
            eprintln!("[send2boox][warn] 更新上传任务状态失败: {err}");
            false
        }
    }
}

fn finish_upload_task(app: &tauri::AppHandle) {
    match app.state::<RuntimeState>().upload_in_progress.lock() {
        Ok(mut in_progress) => *in_progress = false,
        Err(err) => eprintln!("[send2boox][warn] 重置上传任务状态失败: {err}"),
    }
    update_upload_runtime_state(app, |state| {
        state.in_progress = false;
        state.speed_bps = None;
        state.eta_seconds = None;
    });
}

#[derive(Debug, Clone, serde::Deserialize)]
struct BucketConfig {
    #[serde(rename = "aliEndpoint")]
    ali_endpoint: Option<String>,
    bucket: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct OssSts {
    #[serde(rename = "AccessKeyId")]
    access_key_id: String,
    #[serde(rename = "AccessKeySecret")]
    access_key_secret: String,
    #[serde(rename = "SecurityToken")]
    security_token: String,
    #[serde(rename = "Expiration")]
    expiration: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct SyncToken {
    #[serde(rename = "cookie_name")]
    cookie_name: Option<String>,
    #[serde(rename = "session_id")]
    session_id: Option<String>,
}

#[derive(Debug, Clone)]
struct UploadAuthContext {
    bearer: Option<String>,
    cookie: Option<String>,
    use_webview_session: bool,
    uid: String,
    storage_limit: Option<u64>,
    storage_used: Option<u64>,
}

fn api_get_json(
    client: &Client,
    url: &str,
    bearer: Option<&str>,
    cookie: Option<&str>,
) -> Result<Value, String> {
    let mut req = client.get(url);
    if let Some(token) = bearer {
        req = req.header(AUTHORIZATION, format!("Bearer {token}"));
    }
    if let Some(cookie_header) = cookie {
        if !cookie_header.trim().is_empty() {
            req = req.header(COOKIE, cookie_header.to_string());
        }
    }
    let response = req.send().map_err(|err| err.to_string())?;
    let status = response.status();
    let body = response.text().map_err(|err| err.to_string())?;
    if !status.is_success() {
        return Err(format!("HTTP {status}: {body}"));
    }

    let value: Value = serde_json::from_str(&body).map_err(|err| err.to_string())?;
    let code = value
        .get("result_code")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    if code != 0 {
        let msg = value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        return Err(format!("result_code={code}: {msg}"));
    }
    Ok(value.get("data").cloned().unwrap_or(Value::Null))
}

fn api_post_json_keep_body(
    client: &Client,
    url: &str,
    bearer: Option<&str>,
    cookie: Option<&str>,
    body: &Value,
) -> Result<Value, String> {
    let mut req = client.post(url).header(CONTENT_TYPE, "application/json");
    if let Some(token) = bearer {
        req = req.header(AUTHORIZATION, format!("Bearer {token}"));
    }
    if let Some(cookie_header) = cookie {
        if !cookie_header.trim().is_empty() {
            req = req.header(COOKIE, cookie_header.to_string());
        }
    }
    let response = req
        .body(body.to_string())
        .send()
        .map_err(|err| err.to_string())?;
    let status = response.status();
    let text = response.text().map_err(|err| err.to_string())?;
    if !status.is_success() {
        return Err(format!("HTTP {status}: {text}"));
    }
    let value: Value = serde_json::from_str(&text).map_err(|err| err.to_string())?;
    let code = value
        .get("result_code")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    if code != 0 {
        let msg = value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        return Err(format!("result_code={code}: {msg}"));
    }
    Ok(value)
}

fn api_post_json(
    client: &Client,
    url: &str,
    bearer: Option<&str>,
    cookie: Option<&str>,
    body: &Value,
) -> Result<Value, String> {
    let mut req = client.post(url).header(CONTENT_TYPE, "application/json");
    if let Some(token) = bearer {
        req = req.header(AUTHORIZATION, format!("Bearer {token}"));
    }
    if let Some(cookie_header) = cookie {
        if !cookie_header.trim().is_empty() {
            req = req.header(COOKIE, cookie_header.to_string());
        }
    }
    let response = req
        .body(body.to_string())
        .send()
        .map_err(|err| err.to_string())?;
    let status = response.status();
    let text = response.text().map_err(|err| err.to_string())?;
    if !status.is_success() {
        return Err(format!("HTTP {status}: {text}"));
    }
    let value: Value = serde_json::from_str(&text).map_err(|err| err.to_string())?;
    let code = value
        .get("result_code")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    if code != 0 {
        let msg = value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        return Err(format!("result_code={code}: {msg}"));
    }
    Ok(value.get("data").cloned().unwrap_or(Value::Null))
}

fn parse_u64_field(value: &Value, key: &str) -> Option<u64> {
    let field = value.get(key)?;
    if let Some(number) = field.as_u64() {
        return Some(number);
    }
    field.as_str().and_then(|raw| raw.parse::<u64>().ok())
}

fn parse_i64_field(value: &Value, key: &str) -> Option<i64> {
    let field = value.get(key)?;
    if let Some(number) = field.as_i64() {
        return Some(number);
    }
    if let Some(number) = field.as_u64() {
        return i64::try_from(number).ok();
    }
    field.as_str().and_then(|raw| raw.parse::<i64>().ok())
}

fn parse_bool_field(value: &Value, key: &str) -> Option<bool> {
    let field = value.get(key)?;
    if let Some(raw) = field.as_bool() {
        return Some(raw);
    }
    if let Some(raw) = field.as_i64() {
        return Some(raw != 0);
    }
    if let Some(raw) = field.as_u64() {
        return Some(raw != 0);
    }
    field.as_str().map(|raw| {
        matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes"
        )
    })
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value.and_then(|item| {
        let trimmed = item.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn auth_cache_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    let mut dir = app.path_resolver().app_data_dir()?;
    if fs::create_dir_all(&dir).is_err() {
        return None;
    }
    dir.push(AUTH_CACHE_FILE);
    Some(dir)
}

fn load_persisted_auth_state(app: &tauri::AppHandle) -> Option<CachedAuthState> {
    let path = auth_cache_path(app)?;
    let raw = fs::read_to_string(path).ok()?;
    let persisted: PersistedAuthState = serde_json::from_str(&raw).ok()?;
    Some(CachedAuthState {
        token: normalize_optional(persisted.token),
        cookie: normalize_optional(persisted.cookie),
    })
}

fn persist_cached_auth_state(app: &tauri::AppHandle, state: &CachedAuthState) {
    let Some(path) = auth_cache_path(app) else {
        return;
    };
    if !has_auth_state(state) {
        if let Err(err) = fs::remove_file(path) {
            if err.kind() != std::io::ErrorKind::NotFound {
                eprintln!("[send2boox][warn] 删除授权缓存失败: {err}");
            }
        }
        return;
    }
    let payload = PersistedAuthState {
        token: state.token.clone(),
        cookie: state.cookie.clone(),
        updated_ms: Some(unix_ms_now()),
    };
    match serde_json::to_string(&payload) {
        Ok(text) => {
            if let Err(err) = fs::write(path, text) {
                eprintln!("[send2boox][warn] 写入授权缓存失败: {err}");
            }
        }
        Err(err) => eprintln!("[send2boox][warn] 序列化授权缓存失败: {err}"),
    }
}

fn hydrate_cached_auth_state(app: &tauri::AppHandle) {
    if has_auth_state(&get_cached_auth_state(app)) {
        return;
    }
    if let Some(state) = load_persisted_auth_state(app) {
        if has_auth_state(&state) {
            set_cached_auth_state(app, state);
        }
    }
}

fn has_auth_state(state: &CachedAuthState) -> bool {
    state.token.is_some() || state.cookie.is_some()
}

fn set_login_authorizing(app: &tauri::AppHandle, value: bool) {
    match app.state::<RuntimeState>().login_authorizing.lock() {
        Ok(mut flag) => {
            *flag = value;
        }
        Err(err) => eprintln!("[send2boox][warn] 更新登录授权状态失败: {err}"),
    }
}

fn complete_login_authorization_if_needed(app: &tauri::AppHandle) {
    let should_return = match app.state::<RuntimeState>().login_authorizing.lock() {
        Ok(mut flag) => {
            if *flag {
                *flag = false;
                true
            } else {
                false
            }
        }
        Err(err) => {
            eprintln!("[send2boox][warn] 读取登录授权状态失败: {err}");
            false
        }
    };
    if !should_return {
        return;
    }

    let app_handle = app.clone();
    if let Err(err) = app.run_on_main_thread(move || {
        if let Some(main) = app_handle.get_window(MAIN_LABEL) {
            if let Err(err) = main.hide() {
                log_error("登录后隐藏主页面失败", &err);
            }
        }
        show_dashboard_window_from_last_anchor(&app_handle);
    }) {
        log_error("登录后恢复仪表盘失败", &err);
    }
}

fn set_last_tray_anchor(
    app: &tauri::AppHandle,
    tray_x: f64,
    tray_y: f64,
    tray_w: f64,
    tray_h: f64,
) {
    match app.state::<RuntimeState>().last_tray_anchor.lock() {
        Ok(mut anchor) => {
            *anchor = Some((tray_x, tray_y, tray_w, tray_h));
        }
        Err(err) => eprintln!("[send2boox][warn] 写入托盘锚点失败: {err}"),
    }
}

fn get_last_tray_anchor(app: &tauri::AppHandle) -> Option<(f64, f64, f64, f64)> {
    match app.state::<RuntimeState>().last_tray_anchor.lock() {
        Ok(anchor) => *anchor,
        Err(err) => {
            eprintln!("[send2boox][warn] 读取托盘锚点失败: {err}");
            None
        }
    }
}

fn get_dashboard_cache(app: &tauri::AppHandle, max_age_ms: u128) -> Option<DashboardSnapshot> {
    let now = unix_ms_now();
    match app.state::<RuntimeState>().dashboard_cache.lock() {
        Ok(cache) => cache.clone().filter(|snap| {
            let age = now.saturating_sub(snap.fetched_at_ms);
            age <= max_age_ms
        }),
        Err(err) => {
            eprintln!("[send2boox][warn] 读取仪表盘缓存失败: {err}");
            None
        }
    }
}

fn set_dashboard_cache(app: &tauri::AppHandle, snapshot: DashboardSnapshot) {
    match app.state::<RuntimeState>().dashboard_cache.lock() {
        Ok(mut cache) => {
            *cache = Some(snapshot);
        }
        Err(err) => eprintln!("[send2boox][warn] 写入仪表盘缓存失败: {err}"),
    }
}

fn update_dashboard_cache_after_delete(
    app: &tauri::AppHandle,
    deleted_id: &str,
) -> Option<DashboardSnapshot> {
    match app.state::<RuntimeState>().dashboard_cache.lock() {
        Ok(mut cache) => {
            let Some(snapshot) = cache.as_mut() else {
                return None;
            };
            snapshot.push_queue.retain(|item| item.id != deleted_id);
            snapshot.upload = current_upload_snapshot(app);
            snapshot.fetched_at_ms = unix_ms_now();
            Some(snapshot.clone())
        }
        Err(err) => {
            eprintln!("[send2boox][warn] 更新仪表盘缓存失败: {err}");
            None
        }
    }
}

fn set_cached_auth_state(app: &tauri::AppHandle, state: CachedAuthState) {
    let is_authorized = has_auth_state(&state);
    let mut changed = false;
    let state_for_persist = state.clone();
    match app.state::<RuntimeState>().cached_auth_state.lock() {
        Ok(mut cached) => {
            changed = cached.token != state.token || cached.cookie != state.cookie;
            *cached = state;
        }
        Err(err) => eprintln!("[send2boox][warn] 写入缓存 token 失败: {err}"),
    }
    if changed {
        persist_cached_auth_state(app, &state_for_persist);
    }
    if changed {
        if let Ok(mut cache) = app.state::<RuntimeState>().dashboard_cache.lock() {
            *cache = None;
        }
    }
    if is_authorized {
        complete_login_authorization_if_needed(app);
    }
}

fn get_cached_auth_state(app: &tauri::AppHandle) -> CachedAuthState {
    match app.state::<RuntimeState>().cached_auth_state.lock() {
        Ok(cached) => cached.clone(),
        Err(err) => {
            eprintln!("[send2boox][warn] 读取缓存 token 失败: {err}");
            CachedAuthState::default()
        }
    }
}

fn decode_title_token(encoded: &str) -> Option<String> {
    if encoded.is_empty() {
        return None;
    }
    let raw = BASE64_STANDARD.decode(encoded.as_bytes()).ok()?;
    let token = String::from_utf8(raw).ok()?;
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

fn decode_title_auth_state(encoded: &str) -> Option<CachedAuthState> {
    if encoded.is_empty() {
        return None;
    }
    let raw = BASE64_STANDARD.decode(encoded.as_bytes()).ok()?;
    let text = String::from_utf8(raw).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    let token = value
        .get("token")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let cookie = value
        .get("cookie")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    Some(CachedAuthState {
        token: normalize_optional(token),
        cookie: normalize_optional(cookie),
    })
}

fn parse_auth_state_from_url(url: &str) -> Option<CachedAuthState> {
    let encoded = get_hash_query_value(url, AUTH_STATE_QUERY_KEY)?;
    decode_title_auth_state(&encoded)
}

fn poll_token_from_window(window: &Window) {
    let app = window.app_handle();
    let label = window.label().to_string();
    thread::spawn(move || {
        for _ in 0..30 {
            thread::sleep(Duration::from_millis(150));
            let Some(target) = app.get_window(&label) else {
                continue;
            };
            let current_url = target.url().to_string();
            if let Some(state) = parse_auth_state_from_url(&current_url) {
                set_cached_auth_state(&app, state);
                return;
            }
            let Ok(title) = target.title() else {
                continue;
            };
            let Some(encoded) = title.strip_prefix(AUTH_TITLE_PREFIX) else {
                if let Some(old_encoded) = title.strip_prefix("S2B_AUTH_TOKEN::") {
                    set_cached_auth_state(
                        &app,
                        CachedAuthState {
                            token: decode_title_token(old_encoded),
                            cookie: None,
                        },
                    );
                    return;
                }
                continue;
            };
            if let Some(state) = decode_title_auth_state(encoded) {
                set_cached_auth_state(&app, state);
            }
            return;
        }
    });
}

fn refresh_cached_auth_token(app: &tauri::AppHandle) {
    if let Some(window) = app.get_window(MAIN_LABEL) {
        if let Err(err) = window.eval(AUTH_TOKEN_CAPTURE_SCRIPT) {
            log_error("执行 token 捕获脚本失败", &err);
            return;
        }
        poll_token_from_window(&window);
    }
}

fn bootstrap_auth_from_main_window(app: &tauri::AppHandle, timeout_ms: u64) {
    if has_auth_state(&get_cached_auth_state(app)) {
        return;
    }
    let _ = ensure_main_window(app, AppPage::Recent, false);
    refresh_cached_auth_token(app);
    let _ = wait_for_cached_auth_state(app, timeout_ms);
}

fn start_main_auth_sync_poller(app: &tauri::AppHandle) {
    let app_handle = app.clone();
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(4));
        if app_handle.get_window(MAIN_LABEL).is_none() {
            continue;
        }
        refresh_cached_auth_token(&app_handle);
    });
}

fn wait_for_cached_auth_state(app: &tauri::AppHandle, timeout_ms: u64) -> CachedAuthState {
    let initial = get_cached_auth_state(app);
    if initial.token.is_some() || initial.cookie.is_some() || timeout_ms == 0 {
        return initial;
    }
    if app.get_window(MAIN_LABEL).is_none() {
        return initial;
    }

    let loops = (timeout_ms / 100).max(1);
    for _ in 0..loops {
        let state = get_cached_auth_state(app);
        if state.token.is_some() || state.cookie.is_some() {
            return state;
        }
        thread::sleep(Duration::from_millis(100));
    }
    CachedAuthState::default()
}

fn parse_api_data_from_text(raw: &str) -> Result<Value, String> {
    let value: Value = serde_json::from_str(raw).map_err(|err| err.to_string())?;
    let code = value
        .get("result_code")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    if code != 0 {
        let msg = value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        return Err(format!("result_code={code}: {msg}"));
    }
    Ok(value.get("data").cloned().unwrap_or(Value::Null))
}

fn browser_fetch_api_data(
    app: &tauri::AppHandle,
    method: &str,
    url: &str,
    body: Option<&Value>,
) -> Result<Value, String> {
    let window = app
        .get_window(MAIN_LABEL)
        .ok_or_else(|| "主页面未初始化，无法访问浏览器登录会话".to_string())?;
    let request_id = Uuid::new_v4().to_string();
    let prefix = format!("{BROWSER_FETCH_PREFIX}{request_id}::");
    let hash_prefix = format!("{request_id}::");
    let body_b64 = body
        .map(|payload| BASE64_STANDARD.encode(payload.to_string()))
        .unwrap_or_default();
    let method_upper = method.to_ascii_uppercase();
    let script = format!(
        r#"
(() => {{
  const reqPrefix = '{prefix}';
  const decodeB64 = (s) => decodeURIComponent(escape(atob(s)));
  (async () => {{
    try {{
      const init = {{
        method: '{method}',
        credentials: 'include',
        headers: {{
          'Content-Type': 'application/json',
          'Accept': 'application/json, text/plain, */*'
        }}
      }};
      const bodyB64 = '{body_b64}';
      if (!['GET', 'HEAD'].includes('{method}')) {{
        const raw = bodyB64 ? decodeB64(bodyB64) : '';
        if (raw && raw !== 'null') init.body = raw;
      }}
      const res = await fetch('{url}', init);
      const text = await res.text();
      const payload = {{ status: res.status, ok: res.ok, body: text }};
      const encoded = btoa(unescape(encodeURIComponent(JSON.stringify(payload))));
      document.title = reqPrefix + encoded;
      const hash = window.location.hash || '#/';
      const [route, query = ''] = hash.split('?');
      const params = new URLSearchParams(query);
      params.set('{fetch_key}', '{hash_prefix}' + encoded);
      const nextHash = route + '?' + params.toString();
      if (window.location.hash !== nextHash) window.location.hash = nextHash;
    }} catch (err) {{
      const payload = {{ status: 0, ok: false, error: String(err) }};
      const encoded = btoa(unescape(encodeURIComponent(JSON.stringify(payload))));
      document.title = reqPrefix + encoded;
      const hash = window.location.hash || '#/';
      const [route, query = ''] = hash.split('?');
      const params = new URLSearchParams(query);
      params.set('{fetch_key}', '{hash_prefix}' + encoded);
      const nextHash = route + '?' + params.toString();
      if (window.location.hash !== nextHash) window.location.hash = nextHash;
    }}
  }})();
}})();
"#,
        prefix = escape_js(&prefix),
        method = escape_js(&method_upper),
        body_b64 = escape_js(&body_b64),
        url = escape_js(url),
        hash_prefix = escape_js(&hash_prefix),
        fetch_key = FETCH_RESULT_QUERY_KEY,
    );

    window
        .eval(&script)
        .map_err(|err| format!("执行浏览器会话请求失败: {err}"))?;

    for _ in 0..100 {
        thread::sleep(Duration::from_millis(100));
        let current_url = window.url().to_string();
        if let Some(hash_value) = get_hash_query_value(&current_url, FETCH_RESULT_QUERY_KEY) {
            if let Some(encoded) = hash_value.strip_prefix(&hash_prefix) {
                let raw = BASE64_STANDARD
                    .decode(encoded.as_bytes())
                    .map_err(|err| err.to_string())?;
                let text = String::from_utf8(raw).map_err(|err| err.to_string())?;
                let payload: Value = serde_json::from_str(&text).map_err(|err| err.to_string())?;
                let status = payload.get("status").and_then(Value::as_i64).unwrap_or(0);
                if status <= 0 {
                    let err_text = payload
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    return Err(format!("浏览器会话请求失败: {err_text}"));
                }
                let body_text = payload
                    .get("body")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if !(200..300).contains(&status) {
                    return Err(format!("浏览器会话请求返回 HTTP {status}: {body_text}"));
                }
                return parse_api_data_from_text(body_text);
            }
        }

        let current = window.title().map_err(|err| err.to_string())?;
        let Some(encoded) = current.strip_prefix(&prefix) else {
            continue;
        };
        let raw = BASE64_STANDARD
            .decode(encoded.as_bytes())
            .map_err(|err| err.to_string())?;
        let text = String::from_utf8(raw).map_err(|err| err.to_string())?;
        let payload: Value = serde_json::from_str(&text).map_err(|err| err.to_string())?;
        let status = payload.get("status").and_then(Value::as_i64).unwrap_or(0);
        if status <= 0 {
            let err_text = payload
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            return Err(format!("浏览器会话请求失败: {err_text}"));
        }
        let body_text = payload
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !(200..300).contains(&status) {
            return Err(format!("浏览器会话请求返回 HTTP {status}: {body_text}"));
        }
        return parse_api_data_from_text(body_text);
    }

    Err("浏览器会话请求超时".to_string())
}

fn browser_fetch_raw(
    app: &tauri::AppHandle,
    method: &str,
    url: &str,
    body_text: Option<&str>,
) -> Result<(i64, String), String> {
    let window = app
        .get_window(MAIN_LABEL)
        .ok_or_else(|| "主页面未初始化，无法访问浏览器登录会话".to_string())?;
    let request_id = Uuid::new_v4().to_string();
    let prefix = format!("{BROWSER_FETCH_PREFIX}{request_id}::");
    let hash_prefix = format!("{request_id}::");
    let body_b64 = body_text
        .map(|payload| BASE64_STANDARD.encode(payload))
        .unwrap_or_default();
    let method_upper = method.to_ascii_uppercase();
    let script = format!(
        r#"
(() => {{
  const reqPrefix = '{prefix}';
  const decodeB64 = (s) => decodeURIComponent(escape(atob(s)));
  (async () => {{
    try {{
      const init = {{
        method: '{method}',
        credentials: 'include',
        headers: {{ 'Content-Type': 'application/json' }}
      }};
      const bodyB64 = '{body_b64}';
      if (!['GET', 'HEAD'].includes('{method}')) {{
        const raw = bodyB64 ? decodeB64(bodyB64) : '';
        if (raw && raw !== 'null') init.body = raw;
      }}
      const res = await fetch('{url}', init);
      const text = await res.text();
      const payload = {{ status: res.status, ok: res.ok, body: text }};
      const encoded = btoa(unescape(encodeURIComponent(JSON.stringify(payload))));
      document.title = reqPrefix + encoded;
      const hash = window.location.hash || '#/';
      const [route, query = ''] = hash.split('?');
      const params = new URLSearchParams(query);
      params.set('{fetch_key}', '{hash_prefix}' + encoded);
      const nextHash = route + '?' + params.toString();
      if (window.location.hash !== nextHash) window.location.hash = nextHash;
    }} catch (err) {{
      const payload = {{ status: 0, ok: false, error: String(err) }};
      const encoded = btoa(unescape(encodeURIComponent(JSON.stringify(payload))));
      document.title = reqPrefix + encoded;
      const hash = window.location.hash || '#/';
      const [route, query = ''] = hash.split('?');
      const params = new URLSearchParams(query);
      params.set('{fetch_key}', '{hash_prefix}' + encoded);
      const nextHash = route + '?' + params.toString();
      if (window.location.hash !== nextHash) window.location.hash = nextHash;
    }}
  }})();
}})();
"#,
        prefix = escape_js(&prefix),
        method = escape_js(&method_upper),
        body_b64 = escape_js(&body_b64),
        url = escape_js(url),
        hash_prefix = escape_js(&hash_prefix),
        fetch_key = FETCH_RESULT_QUERY_KEY,
    );
    window
        .eval(&script)
        .map_err(|err| format!("执行浏览器会话请求失败: {err}"))?;
    for _ in 0..100 {
        thread::sleep(Duration::from_millis(100));
        let current_url = window.url().to_string();
        if let Some(hash_value) = get_hash_query_value(&current_url, FETCH_RESULT_QUERY_KEY) {
            if let Some(encoded) = hash_value.strip_prefix(&hash_prefix) {
                let raw = BASE64_STANDARD
                    .decode(encoded.as_bytes())
                    .map_err(|err| err.to_string())?;
                let text = String::from_utf8(raw).map_err(|err| err.to_string())?;
                let payload: Value = serde_json::from_str(&text).map_err(|err| err.to_string())?;
                let status = payload.get("status").and_then(Value::as_i64).unwrap_or(0);
                if status <= 0 {
                    let err_text = payload
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    return Err(format!("浏览器会话请求失败: {err_text}"));
                }
                let body = payload
                    .get("body")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                return Ok((status, body));
            }
        }
    }
    Err("浏览器会话请求超时".to_string())
}

fn browser_put_push_doc_via_pouchdb(
    app: &tauri::AppHandle,
    db_name: &str,
    doc_json: &str,
) -> Result<(String, String), String> {
    let window = app
        .get_window(MAIN_LABEL)
        .ok_or_else(|| "主页面未初始化，无法访问浏览器登录会话".to_string())?;
    let request_id = Uuid::new_v4().to_string();
    let prefix = format!("{BROWSER_FETCH_PREFIX}{request_id}::");
    let hash_prefix = format!("{request_id}::");
    let doc_b64 = BASE64_STANDARD.encode(doc_json);
    let script = format!(
        r#"
(() => {{
  const reqPrefix = '{prefix}';
  const hashPrefix = '{hash_prefix}';
  const fetchKey = '{fetch_key}';
  const dbName = '{db_name}';
  const docB64 = '{doc_b64}';
  const decodeB64 = (s) => decodeURIComponent(escape(atob(s)));
  const pushResult = (payload) => {{
    const encoded = btoa(unescape(encodeURIComponent(JSON.stringify(payload))));
    document.title = reqPrefix + encoded;
    const hash = window.location.hash || '#/';
    const [route, query = ''] = hash.split('?');
    const params = new URLSearchParams(query);
    params.set(fetchKey, hashPrefix + encoded);
    const nextHash = route + '?' + params.toString();
    if (window.location.hash !== nextHash) window.location.hash = nextHash;
  }};
  (async () => {{
    try {{
      const doc = JSON.parse(decodeB64(docB64));
      try {{
        const syncResp = await fetch('https://send2boox.com/api/1/users/syncToken', {{
          method: 'GET',
          credentials: 'include',
          headers: {{ 'Accept': 'application/json, text/plain, */*' }}
        }});
        const syncText = await syncResp.text();
        const syncObj = JSON.parse(syncText);
        if (syncObj && syncObj.result_code === 0 && syncObj.data && syncObj.data.session_id) {{
          const sid = String(syncObj.data.session_id);
          const cname = String(syncObj.data.cookie_name || 'SyncGatewaySession');
          const pairs = [
            `${{cname}}=${{sid}}; path=/`,
            `${{cname}}=${{sid}}; path=/neocloud`,
            `SyncGatewaySession=${{sid}}; path=/`,
            `SyncGatewaySession=${{sid}}; path=/neocloud`,
            `${{cname}}=${{sid}}; domain=send2boox.com; path=/`,
            `SyncGatewaySession=${{sid}}; domain=send2boox.com; path=/`
          ];
          for (const item of pairs) {{
            try {{ document.cookie = item; }} catch (_) {{}}
          }}
        }}
      }} catch (_) {{}}

      if (!window.PouchDB) throw new Error('PouchDB not found');
      const db = new window.PouchDB(dbName);
      const putRes = await db.put(doc);
      await db.replicate.to('https://send2boox.com/neocloud');
      pushResult({{
        ok: true,
        id: putRes && putRes.id ? putRes.id : doc._id,
        rev: putRes && putRes.rev ? putRes.rev : ''
      }});
    }} catch (err) {{
      pushResult({{ ok: false, error: String(err) }});
    }}
  }})();
}})();
"#,
        prefix = escape_js(&prefix),
        hash_prefix = escape_js(&hash_prefix),
        fetch_key = FETCH_RESULT_QUERY_KEY,
        db_name = escape_js(db_name),
        doc_b64 = escape_js(&doc_b64),
    );
    window
        .eval(&script)
        .map_err(|err| format!("执行浏览器 PouchDB 写入失败: {err}"))?;

    for _ in 0..300 {
        thread::sleep(Duration::from_millis(100));
        let current_url = window.url().to_string();
        if let Some(hash_value) = get_hash_query_value(&current_url, FETCH_RESULT_QUERY_KEY) {
            if let Some(encoded) = hash_value.strip_prefix(&hash_prefix) {
                let raw = BASE64_STANDARD
                    .decode(encoded.as_bytes())
                    .map_err(|err| err.to_string())?;
                let text = String::from_utf8(raw).map_err(|err| err.to_string())?;
                let payload: Value = serde_json::from_str(&text).map_err(|err| err.to_string())?;
                if payload.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                    let id = json_field_to_string(&payload, "id")
                        .ok_or_else(|| format!("PouchDB 返回缺少 id: {}", text))?;
                    let rev = json_field_to_string(&payload, "rev")
                        .ok_or_else(|| format!("PouchDB 返回缺少 rev: {}", text))?;
                    return Ok((id, rev));
                }
                let err_text = payload
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                return Err(format!("浏览器 PouchDB 写入失败: {err_text}"));
            }
        }
    }

    Err("浏览器 PouchDB 写入超时".to_string())
}

fn browser_fetch_push_queue_via_pouchdb(
    app: &tauri::AppHandle,
    uid: &str,
) -> Result<Vec<DashboardPushItem>, String> {
    let window = app
        .get_window(MAIN_LABEL)
        .ok_or_else(|| "主页面未初始化，无法访问浏览器登录会话".to_string())?;
    let request_id = Uuid::new_v4().to_string();
    let prefix = format!("{BROWSER_FETCH_PREFIX}{request_id}::");
    let hash_prefix = format!("{request_id}::");
    let db_name = format!("{uid}-boox-message");
    let script = format!(
        r#"
(() => {{
  const reqPrefix = '{prefix}';
  const hashPrefix = '{hash_prefix}';
  const fetchKey = '{fetch_key}';
  const dbName = '{db_name}';
  const limit = {limit};
  const pushResult = (payload) => {{
    const encoded = btoa(unescape(encodeURIComponent(JSON.stringify(payload))));
    document.title = reqPrefix + encoded;
    const hash = window.location.hash || '#/';
    const [route, query = ''] = hash.split('?');
    const params = new URLSearchParams(query);
    params.set(fetchKey, hashPrefix + encoded);
    const nextHash = route + '?' + params.toString();
    if (window.location.hash !== nextHash) window.location.hash = nextHash;
  }};
  const toNumber = (v) => {{
    const n = Number(v);
    return Number.isFinite(n) ? n : 0;
  }};
  (async () => {{
    try {{
      if (!window.PouchDB) throw new Error('PouchDB not found');
      const db = new window.PouchDB(dbName);
      let docs = [];
      try {{
        if (typeof db.find === 'function') {{
          const found = await db.find({{
            selector: {{ msgType: 2, contentType: 'digital_content' }},
            limit
          }});
          docs = found && Array.isArray(found.docs) ? found.docs : [];
        }}
      }} catch (_) {{}}
      if (!docs.length) {{
        const all = await db.allDocs({{ include_docs: true, descending: true, limit: 200 }});
        docs = (all.rows || []).map((row) => row.doc).filter(Boolean);
      }}
      const looksLikePushDoc = (doc) => {{
        if (!doc) return false;
        if (doc.msgType === 2 && doc.contentType === 'digital_content') return true;
        let content = doc.content;
        if (typeof content === 'string') {{
          try {{ content = JSON.parse(content); }} catch (_) {{}}
        }}
        const formats = Array.isArray(content && content.formats) ? content.formats : [];
        const hasStorage = !!(content && content.storage && formats.length > 0);
        const hasName = !!(doc.name || (content && (content.name || content.title)));
        return hasStorage || hasName;
      }};
      docs = docs
        .filter((doc) => looksLikePushDoc(doc))
        .sort((a, b) => toNumber(b.updatedAt || b.createdAt) - toNumber(a.updatedAt || a.createdAt))
        .slice(0, limit);

      const items = docs.map((doc) => {{
        let content = null;
        try {{
          content = typeof doc.content === 'string' ? JSON.parse(doc.content) : doc.content;
        }} catch (_) {{}}
        const formats = Array.isArray(content && content.formats) ? content.formats : [];
        const format = formats.length ? String(formats[0]) : '';
        const storage = content && content.storage && format ? content.storage[format] : null;
        const oss = storage && storage.oss ? storage.oss : null;
        return {{
          id: String(doc._id || doc.id || (content && content._id) || ''),
          rev: String(doc._rev || ''),
          name: String(doc.name || (content && (content.name || content.title)) || ''),
          size: toNumber(doc.size || (content && content.size) || (oss && oss.size)),
          updated_at: toNumber(doc.updatedAt || (content && content.updatedAt) || doc.createdAt),
          format: format || null,
          resource_key: String((oss && oss.key) || (content && content.resourceKey) || '')
        }};
      }}).filter((item) => item.id || item.name);

      pushResult({{ ok: true, items }});
    }} catch (err) {{
      pushResult({{ ok: false, error: String(err) }});
    }}
  }})();
}})();
"#,
        prefix = escape_js(&prefix),
        hash_prefix = escape_js(&hash_prefix),
        fetch_key = FETCH_RESULT_QUERY_KEY,
        db_name = escape_js(&db_name),
        limit = DASHBOARD_PUSH_QUEUE_LIMIT,
    );

    window
        .eval(&script)
        .map_err(|err| format!("执行浏览器 PouchDB 查询失败: {err}"))?;

    for _ in 0..200 {
        thread::sleep(Duration::from_millis(100));
        let current_url = window.url().to_string();
        if let Some(hash_value) = get_hash_query_value(&current_url, FETCH_RESULT_QUERY_KEY) {
            if let Some(encoded) = hash_value.strip_prefix(&hash_prefix) {
                let raw = BASE64_STANDARD
                    .decode(encoded.as_bytes())
                    .map_err(|err| err.to_string())?;
                let text = String::from_utf8(raw).map_err(|err| err.to_string())?;
                let payload: Value = serde_json::from_str(&text).map_err(|err| err.to_string())?;
                if !payload.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                    let err_text = payload
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    return Err(format!("浏览器 PouchDB 查询失败: {err_text}"));
                }
                let items = payload
                    .get("items")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let mut out = Vec::new();
                for item in items {
                    let id = item
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    let name = item
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    if id.is_empty() && name.is_empty() {
                        continue;
                    }
                    out.push(DashboardPushItem {
                        id,
                        rev: item
                            .get("rev")
                            .and_then(Value::as_str)
                            .filter(|v| !v.trim().is_empty())
                            .map(ToString::to_string),
                        name,
                        size: parse_u64_field(&item, "size"),
                        updated_at: parse_i64_field(&item, "updated_at"),
                        format: item
                            .get("format")
                            .and_then(Value::as_str)
                            .map(ToString::to_string),
                        resource_key: item
                            .get("resource_key")
                            .and_then(Value::as_str)
                            .map(ToString::to_string),
                    });
                }
                return Ok(out);
            }
        }
    }

    Err("浏览器 PouchDB 查询超时".to_string())
}

#[derive(Debug, Clone)]
struct PushDocDetail {
    id: String,
    rev: String,
    name: String,
    resource_key: String,
    format: String,
}

fn browser_fetch_push_detail_via_pouchdb(
    app: &tauri::AppHandle,
    uid: &str,
    doc_id: &str,
) -> Result<PushDocDetail, String> {
    let window = app
        .get_window(MAIN_LABEL)
        .ok_or_else(|| "主页面未初始化，无法访问浏览器登录会话".to_string())?;
    let request_id = Uuid::new_v4().to_string();
    let prefix = format!("{BROWSER_FETCH_PREFIX}{request_id}::");
    let hash_prefix = format!("{request_id}::");
    let db_name = format!("{uid}-boox-message");
    let script = format!(
        r#"
(() => {{
  const reqPrefix = '{prefix}';
  const hashPrefix = '{hash_prefix}';
  const fetchKey = '{fetch_key}';
  const dbName = '{db_name}';
  const docId = '{doc_id}';
  const pushResult = (payload) => {{
    const encoded = btoa(unescape(encodeURIComponent(JSON.stringify(payload))));
    document.title = reqPrefix + encoded;
    const hash = window.location.hash || '#/';
    const [route, query = ''] = hash.split('?');
    const params = new URLSearchParams(query);
    params.set(fetchKey, hashPrefix + encoded);
    const nextHash = route + '?' + params.toString();
    if (window.location.hash !== nextHash) window.location.hash = nextHash;
  }};
  (async () => {{
    try {{
      if (!window.PouchDB) throw new Error('PouchDB not found');
      const db = new window.PouchDB(dbName);
      const doc = await db.get(docId);
      let content = doc.content;
      if (typeof content === 'string') {{
        try {{ content = JSON.parse(content); }} catch (_) {{}}
      }}
      const formats = Array.isArray(content && content.formats) ? content.formats : [];
      const format = formats.length ? String(formats[0]) : '';
      const storage = content && content.storage && format ? content.storage[format] : null;
      const oss = storage && storage.oss ? storage.oss : null;
      const item = {{
        id: String(doc._id || ''),
        rev: String(doc._rev || ''),
        name: String(doc.name || (content && (content.name || content.title)) || ''),
        resource_key: String((oss && oss.key) || (content && content.resourceKey) || ''),
        format: format
      }};
      pushResult({{ ok: true, item }});
    }} catch (err) {{
      pushResult({{ ok: false, error: String(err) }});
    }}
  }})();
}})();
"#,
        prefix = escape_js(&prefix),
        hash_prefix = escape_js(&hash_prefix),
        fetch_key = FETCH_RESULT_QUERY_KEY,
        db_name = escape_js(&db_name),
        doc_id = escape_js(doc_id),
    );

    window
        .eval(&script)
        .map_err(|err| format!("执行浏览器 PouchDB 详情查询失败: {err}"))?;

    for _ in 0..200 {
        thread::sleep(Duration::from_millis(100));
        let current_url = window.url().to_string();
        if let Some(hash_value) = get_hash_query_value(&current_url, FETCH_RESULT_QUERY_KEY) {
            if let Some(encoded) = hash_value.strip_prefix(&hash_prefix) {
                let raw = BASE64_STANDARD
                    .decode(encoded.as_bytes())
                    .map_err(|err| err.to_string())?;
                let text = String::from_utf8(raw).map_err(|err| err.to_string())?;
                let payload: Value = serde_json::from_str(&text).map_err(|err| err.to_string())?;
                if !payload.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                    let err_text = payload
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    return Err(format!("浏览器 PouchDB 详情查询失败: {err_text}"));
                }
                let item = payload
                    .get("item")
                    .ok_or_else(|| "浏览器 PouchDB 详情缺失 item".to_string())?;
                let id = item
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let rev = item
                    .get("rev")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let name = item
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let resource_key = item
                    .get("resource_key")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let format = item
                    .get("format")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if id.is_empty() || rev.is_empty() || resource_key.is_empty() {
                    return Err("推送记录缺少关键字段（id/rev/resource_key）".to_string());
                }
                return Ok(PushDocDetail {
                    id,
                    rev,
                    name,
                    resource_key,
                    format,
                });
            }
        }
    }

    Err("浏览器 PouchDB 详情查询超时".to_string())
}

fn browser_delete_push_doc_via_pouchdb(
    app: &tauri::AppHandle,
    uid: &str,
    doc_id: &str,
) -> Result<(), String> {
    let window = app
        .get_window(MAIN_LABEL)
        .ok_or_else(|| "主页面未初始化，无法访问浏览器登录会话".to_string())?;
    let request_id = Uuid::new_v4().to_string();
    let prefix = format!("{BROWSER_FETCH_PREFIX}{request_id}::");
    let hash_prefix = format!("{request_id}::");
    let db_name = format!("{uid}-boox-message");
    let script = format!(
        r#"
(() => {{
  const reqPrefix = '{prefix}';
  const hashPrefix = '{hash_prefix}';
  const fetchKey = '{fetch_key}';
  const dbName = '{db_name}';
  const docId = '{doc_id}';
  const pushResult = (payload) => {{
    const encoded = btoa(unescape(encodeURIComponent(JSON.stringify(payload))));
    document.title = reqPrefix + encoded;
    const hash = window.location.hash || '#/';
    const [route, query = ''] = hash.split('?');
    const params = new URLSearchParams(query);
    params.set(fetchKey, hashPrefix + encoded);
    const nextHash = route + '?' + params.toString();
    if (window.location.hash !== nextHash) window.location.hash = nextHash;
  }};
  (async () => {{
    try {{
      if (!window.PouchDB) throw new Error('PouchDB not found');
      const db = new window.PouchDB(dbName);
      let doc = null;
      try {{
        doc = await db.get(docId);
      }} catch (e) {{
        if (String(e).includes('404')) {{
          pushResult({{ ok: true }});
          return;
        }}
        throw e;
      }}
      await db.remove(doc);
      await db.replicate.to('https://send2boox.com/neocloud');
      pushResult({{ ok: true }});
    }} catch (err) {{
      pushResult({{ ok: false, error: String(err) }});
    }}
  }})();
}})();
"#,
        prefix = escape_js(&prefix),
        hash_prefix = escape_js(&hash_prefix),
        fetch_key = FETCH_RESULT_QUERY_KEY,
        db_name = escape_js(&db_name),
        doc_id = escape_js(doc_id),
    );

    window
        .eval(&script)
        .map_err(|err| format!("执行浏览器 PouchDB 删除失败: {err}"))?;

    for _ in 0..200 {
        thread::sleep(Duration::from_millis(100));
        let current_url = window.url().to_string();
        if let Some(hash_value) = get_hash_query_value(&current_url, FETCH_RESULT_QUERY_KEY) {
            if let Some(encoded) = hash_value.strip_prefix(&hash_prefix) {
                let raw = BASE64_STANDARD
                    .decode(encoded.as_bytes())
                    .map_err(|err| err.to_string())?;
                let text = String::from_utf8(raw).map_err(|err| err.to_string())?;
                let payload: Value = serde_json::from_str(&text).map_err(|err| err.to_string())?;
                if payload.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                    return Ok(());
                }
                let err_text = payload
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                return Err(format!("浏览器 PouchDB 删除失败: {err_text}"));
            }
        }
    }

    Err("浏览器 PouchDB 删除超时".to_string())
}

fn build_auth_context(
    user: Value,
    bearer: Option<String>,
    cookie: Option<String>,
    use_webview_session: bool,
) -> Result<UploadAuthContext, String> {
    let uid = user
        .get("uid")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "登录信息缺少 uid".to_string())?
        .to_string();

    Ok(UploadAuthContext {
        bearer,
        cookie,
        use_webview_session,
        uid,
        storage_limit: parse_u64_field(&user, "storage_limit"),
        storage_used: parse_u64_field(&user, "storage_used"),
    })
}

fn fetch_auth_context(
    client: &Client,
    app: &tauri::AppHandle,
) -> Result<UploadAuthContext, String> {
    let (context, _) = fetch_auth_context_and_user(client, app)?;
    Ok(context)
}

fn fetch_auth_context_and_user(
    client: &Client,
    app: &tauri::AppHandle,
) -> Result<(UploadAuthContext, Value), String> {
    hydrate_cached_auth_state(app);
    let mut cached = wait_for_cached_auth_state(app, 1_200);
    if !has_auth_state(&cached) {
        bootstrap_auth_from_main_window(app, 3_500);
        cached = wait_for_cached_auth_state(app, 500);
    }
    let bearer = normalize_optional(cached.token);
    let cookie = normalize_optional(cached.cookie);

    if bearer.is_some() || cookie.is_some() {
        let user = api_get_json(
            client,
            "https://send2boox.com/api/1/users/me",
            bearer.as_deref(),
            cookie.as_deref(),
        );
        if let Ok(data) = user.clone() {
            let context = build_auth_context(data.clone(), bearer, cookie, false)?;
            return Ok((context, data));
        }
    }

    let data = browser_fetch_api_data(app, "GET", "https://send2boox.com/api/1/users/me", None)
        .map_err(|err| format!("无法读取登录 token/cookie，且浏览器会话请求失败: {err}"))?;
    let context = build_auth_context(data.clone(), None, None, true)?;
    Ok((context, data))
}

fn today_ymd() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

fn fetch_api_data_for_auth(
    client: &Client,
    app: &tauri::AppHandle,
    auth: &UploadAuthContext,
    url: &str,
) -> Result<Value, String> {
    if auth.use_webview_session {
        browser_fetch_api_data(app, "GET", url, None)
    } else {
        api_get_json(client, url, auth.bearer.as_deref(), auth.cookie.as_deref())
    }
}

fn auth_source_text(auth: &UploadAuthContext) -> String {
    if auth.use_webview_session {
        return "browser_session".to_string();
    }
    let mut sources = Vec::new();
    if auth.bearer.is_some() {
        sources.push("token");
    }
    if auth.cookie.is_some() {
        sources.push("cookie");
    }
    if sources.is_empty() {
        "unknown".to_string()
    } else {
        sources.join("+")
    }
}

fn push_batch_delete_for_auth(
    client: &Client,
    app: &tauri::AppHandle,
    auth: &UploadAuthContext,
    doc_id: &str,
) -> Result<(), String> {
    let body = json!({ "ids": [doc_id] });
    if auth.use_webview_session {
        browser_fetch_api_data(
            app,
            "POST",
            "https://send2boox.com/api/1/push/message/batchDelete",
            Some(&body),
        )?;
        return Ok(());
    }

    let _ = api_post_json_keep_body(
        client,
        "https://send2boox.com/api/1/push/message/batchDelete",
        auth.bearer.as_deref(),
        auth.cookie.as_deref(),
        &body,
    )?;
    Ok(())
}

fn dashboard_push_resend_inner(app: &tauri::AppHandle, doc_id: &str) -> Result<(), String> {
    if doc_id.trim().is_empty() {
        return Err("推送记录 id 不能为空".to_string());
    }
    let _ = ensure_main_window(app, AppPage::Recent, false);
    refresh_cached_auth_token(app);

    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|err| err.to_string())?;
    let (auth, _) = fetch_auth_context_and_user(&client, app)?;
    let detail = browser_fetch_push_detail_via_pouchdb(app, &auth.uid, doc_id)?;
    let file_name = if detail.name.trim().is_empty() {
        detail.id.clone()
    } else {
        detail.name.clone()
    };
    let resource_type = if detail.format.trim().is_empty() {
        Path::new(&file_name)
            .extension()
            .and_then(|ext| ext.to_str())
            .filter(|ext| !ext.trim().is_empty())
            .unwrap_or("bin")
            .to_ascii_lowercase()
    } else {
        detail.format.to_ascii_lowercase()
    };

    let buckets = fetch_buckets(&client)?;
    let bucket_key = if buckets.contains_key(DEFAULT_BUCKET_KEY) {
        DEFAULT_BUCKET_KEY.to_string()
    } else {
        buckets
            .keys()
            .next()
            .cloned()
            .ok_or_else(|| "未获取到可用 bucket 配置".to_string())?
    };
    let _ = save_and_push(
        app,
        &client,
        &auth,
        &bucket_key,
        &detail.resource_key,
        &file_name,
        &resource_type,
        &detail.id,
        &detail.rev,
    )?;
    Ok(())
}

fn dashboard_push_delete_inner(app: &tauri::AppHandle, doc_id: &str) -> Result<(), String> {
    if doc_id.trim().is_empty() {
        return Err("删除记录 id 不能为空".to_string());
    }
    let _ = ensure_main_window(app, AppPage::Recent, false);
    refresh_cached_auth_token(app);

    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|err| err.to_string())?;
    let (auth, _) = fetch_auth_context_and_user(&client, app)?;
    let _ = browser_delete_push_doc_via_pouchdb(app, &auth.uid, doc_id);
    push_batch_delete_for_auth(&client, app, &auth, doc_id)?;
    Ok(())
}

fn fetch_buckets(
    client: &Client,
) -> Result<std::collections::HashMap<String, BucketConfig>, String> {
    let data = api_get_json(
        client,
        "https://send2boox.com/api/1/config/buckets",
        None,
        None,
    )?;
    serde_json::from_value(data).map_err(|err| err.to_string())
}

fn fetch_sts(
    client: &Client,
    bearer: Option<&str>,
    cookie: Option<&str>,
) -> Result<OssSts, String> {
    let data = api_get_json(
        client,
        "https://send2boox.com/api/1/config/stss",
        bearer,
        cookie,
    )?;
    serde_json::from_value(data).map_err(|err| err.to_string())
}

fn fetch_sts_for_auth(
    client: &Client,
    app: &tauri::AppHandle,
    auth: &UploadAuthContext,
) -> Result<OssSts, String> {
    let data = if auth.use_webview_session {
        browser_fetch_api_data(app, "GET", "https://send2boox.com/api/1/config/stss", None)?
    } else {
        api_get_json(
            client,
            "https://send2boox.com/api/1/config/stss",
            auth.bearer.as_deref(),
            auth.cookie.as_deref(),
        )?
    };
    serde_json::from_value(data).map_err(|err| err.to_string())
}

fn fetch_sync_token_for_auth(
    client: &Client,
    app: &tauri::AppHandle,
    auth: &UploadAuthContext,
) -> Result<SyncToken, String> {
    if let Ok(data) = browser_fetch_api_data(
        app,
        "GET",
        "https://send2boox.com/api/1/users/syncToken",
        None,
    ) {
        if let Ok(token) = serde_json::from_value::<SyncToken>(data) {
            return Ok(token);
        }
    }

    let data = api_get_json(
        client,
        "https://send2boox.com/api/1/users/syncToken",
        auth.bearer.as_deref(),
        auth.cookie.as_deref(),
    )?;
    serde_json::from_value(data).map_err(|err| err.to_string())
}

fn seed_sync_cookie_in_window(
    app: &tauri::AppHandle,
    sync_token: &SyncToken,
) -> Result<(), String> {
    let window = app
        .get_window(MAIN_LABEL)
        .ok_or_else(|| "主页面未初始化，无法写入同步 cookie".to_string())?;
    let cookie_name = sync_token
        .cookie_name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("SyncGatewaySession");
    let session_id = sync_token
        .session_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "syncToken 缺少 session_id".to_string())?;
    let script = format!(
        r#"
(() => {{
  const name = '{name}';
  const sid = '{sid}';
  const cookies = [
    `${{name}}=${{sid}}; path=/`,
    `${{name}}=${{sid}}; path=/neocloud`,
    `SyncGatewaySession=${{sid}}; path=/`,
    `SyncGatewaySession=${{sid}}; path=/neocloud`,
    `${{name}}=${{sid}}; domain=send2boox.com; path=/`,
    `SyncGatewaySession=${{sid}}; domain=send2boox.com; path=/`
  ];
  for (const c of cookies) {{
    try {{ document.cookie = c; }} catch (_) {{}}
  }}
}})();
"#,
        name = escape_js(cookie_name),
        sid = escape_js(session_id),
    );
    window.eval(&script).map_err(|err| err.to_string())
}

fn sign_oss(secret: &str, string_to_sign: &str) -> Result<String, String> {
    type HmacSha1 = Hmac<Sha1>;
    let mut mac = HmacSha1::new_from_slice(secret.as_bytes()).map_err(|err| err.to_string())?;
    mac.update(string_to_sign.as_bytes());
    Ok(BASE64_STANDARD.encode(mac.finalize().into_bytes()))
}

fn content_type_for(path: &Path) -> String {
    mime_guess::from_path(path)
        .first_or_octet_stream()
        .essence_str()
        .to_string()
}

fn build_object_key(uid: &str, path: &Path) -> (String, String) {
    let format = path
        .extension()
        .and_then(|ext| ext.to_str())
        .filter(|ext| !ext.trim().is_empty())
        .map(|ext| ext.to_ascii_lowercase())
        .unwrap_or_else(|| "bin".to_string());
    let unique = Uuid::new_v4();
    (format!("{uid}/push/{unique}.{format}"), format)
}

fn build_oss_host(bucket: &BucketConfig) -> Result<(String, String), String> {
    let bucket_name = bucket
        .bucket
        .as_ref()
        .filter(|name| !name.trim().is_empty())
        .cloned()
        .ok_or_else(|| "bucket 配置缺少 bucket 字段".to_string())?;
    let ali_endpoint = bucket
        .ali_endpoint
        .as_ref()
        .filter(|name| !name.trim().is_empty())
        .cloned()
        .ok_or_else(|| "bucket 配置缺少 aliEndpoint 字段".to_string())?;
    Ok((bucket_name.clone(), format!("{bucket_name}.{ali_endpoint}")))
}

struct ProgressReader<R, F>
where
    R: Read,
    F: FnMut(u64, u64, Duration),
{
    inner: R,
    total: u64,
    sent: u64,
    started: Instant,
    last_emit: Instant,
    callback: F,
}

impl<R, F> ProgressReader<R, F>
where
    R: Read,
    F: FnMut(u64, u64, Duration),
{
    fn new(inner: R, total: u64, callback: F) -> Self {
        let now = Instant::now();
        Self {
            inner,
            total,
            sent: 0,
            started: now,
            last_emit: now,
            callback,
        }
    }

    fn emit_if_needed(&mut self, force: bool) {
        let now = Instant::now();
        if force || now.duration_since(self.last_emit) >= Duration::from_millis(220) {
            (self.callback)(self.sent, self.total, now.duration_since(self.started));
            self.last_emit = now;
        }
    }
}

impl<R, F> Read for ProgressReader<R, F>
where
    R: Read,
    F: FnMut(u64, u64, Duration),
{
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n > 0 {
            self.sent = self.sent.saturating_add(n as u64);
            if self.sent > self.total {
                self.sent = self.total;
            }
            self.emit_if_needed(self.sent >= self.total);
        } else {
            self.emit_if_needed(true);
        }
        Ok(n)
    }
}

fn upload_to_oss<F>(
    client: &Client,
    bucket: &BucketConfig,
    sts: &OssSts,
    object_key: &str,
    file_path: &Path,
    on_progress: F,
) -> Result<(), String>
where
    F: FnMut(u64, u64, Duration) + Send + 'static,
{
    let (bucket_name, host) = build_oss_host(bucket)?;
    let canonical_resource = format!("/{bucket_name}/{object_key}");
    let date = httpdate::fmt_http_date(SystemTime::now());
    let content_type = content_type_for(file_path);
    let string_to_sign = format!(
        "PUT\n\n{content_type}\n{date}\nx-oss-security-token:{}\n{canonical_resource}",
        sts.security_token
    );
    let signature = sign_oss(&sts.access_key_secret, &string_to_sign)?;
    let authorization = format!("OSS {}:{}", sts.access_key_id, signature);
    let file = File::open(file_path).map_err(|err| err.to_string())?;
    let file_size = file.metadata().map_err(|err| err.to_string())?.len();
    let reader = ProgressReader::new(file, file_size, on_progress);
    let body = reqwest::blocking::Body::sized(reader, file_size);
    let url = format!("https://{host}/{object_key}");

    let response = client
        .put(&url)
        .header("Date", date)
        .header(CONTENT_TYPE, content_type)
        .header(CONTENT_LENGTH, file_size.to_string())
        .header("x-oss-security-token", sts.security_token.clone())
        .header(AUTHORIZATION, authorization)
        .body(body)
        .send()
        .map_err(|err| err.to_string())?;

    if response.status().is_success() {
        return Ok(());
    }

    let status = response.status();
    let body = response.text().unwrap_or_default();
    Err(format!("OSS 上传失败 ({}): {}", status, body))
}

fn signed_download_url(
    bucket: &BucketConfig,
    sts: &OssSts,
    object_key: &str,
) -> Result<String, String> {
    let (bucket_name, host) = build_oss_host(bucket)?;
    let expires = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| err.to_string())?
        .as_secs()
        + 10_000;
    let canonical_resource = format!("/{bucket_name}/{object_key}");
    let string_to_sign = format!("GET\n\n\n{expires}\n{canonical_resource}");
    let signature = sign_oss(&sts.access_key_secret, &string_to_sign)?;

    Ok(format!(
        "https://{host}/{object_key}?OSSAccessKeyId={}&Expires={expires}&Signature={}&security-token={}",
        urlencoding::encode(&sts.access_key_id),
        urlencoding::encode(&signature),
        urlencoding::encode(&sts.security_token)
    ))
}

#[allow(clippy::too_many_arguments)]
fn save_and_push(
    app: &tauri::AppHandle,
    client: &Client,
    auth: &UploadAuthContext,
    bucket_key: &str,
    object_key: &str,
    file_name: &str,
    resource_type: &str,
    cb_id: &str,
    cb_rev: &str,
) -> Result<Value, String> {
    let body = json!({
        "data": {
            "name": file_name,
            "resourceDisplayName": file_name,
            "resourceKey": object_key,
            "bucket": bucket_key,
            "resourceType": resource_type,
            "title": file_name,
            "parent": Value::Null
        },
        "cbMsg": {
            "id": cb_id,
            "rev": cb_rev
        }
    });

    if auth.use_webview_session {
        browser_fetch_api_data(
            app,
            "POST",
            "https://send2boox.com/api/1/push/saveAndPush",
            Some(&body),
        )
    } else {
        api_post_json(
            client,
            "https://send2boox.com/api/1/push/saveAndPush",
            auth.bearer.as_deref(),
            auth.cookie.as_deref(),
            &body,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn put_push_message_doc(
    app: &tauri::AppHandle,
    client: &Client,
    auth: &UploadAuthContext,
    uid: &str,
    file_name: &str,
    file_size: u64,
    resource_type: &str,
    object_key: &str,
    signed_url: &str,
    sync_token: &SyncToken,
) -> Result<(String, String), String> {
    let cookie_name = sync_token
        .cookie_name
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or("SyncGatewaySession");
    let session_id = sync_token
        .session_id
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| "syncToken 缺少 session_id".to_string())?;
    let message_id = Uuid::new_v4().to_string();
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| err.to_string())?
        .as_millis() as u64;
    let mut storage_bucket = serde_json::Map::new();
    storage_bucket.insert(
        resource_type.to_string(),
        json!({
            "oss": {
                "displayName": file_name,
                "expires": 0,
                "key": object_key,
                "provider": "oss",
                "size": file_size,
                "url": signed_url
            }
        }),
    );

    let push_file_doc = json!({
        "_id": message_id,
        "createdAt": now_ms,
        "distributeChannel": "onyx",
        "formats": [resource_type],
        "guid": message_id,
        "name": file_name,
        "ownerId": uid,
        "size": file_size,
        "md5": "",
        "storage": Value::Object(storage_bucket),
        "title": file_name,
        "updatedAt": now_ms
    });
    let message_doc = json!({
        "contentType": "digital_content",
        "content": push_file_doc.to_string(),
        "msgType": 2,
        "dbId": format!("{uid}-MESSAGE"),
        "user": uid,
        "name": file_name,
        "size": file_size,
        "uniqueId": message_id,
        "createdAt": now_ms,
        "updatedAt": now_ms,
        "_id": message_id
    });

    let db_name = format!("{uid}-boox-message");
    let doc_url = format!(
        "https://send2boox.com/neocloud/{}/{}",
        urlencoding::encode(&db_name),
        urlencoding::encode(&message_id)
    );
    let doc_payload = message_doc.to_string();

    let pouch_err = match browser_put_push_doc_via_pouchdb(app, &db_name, &doc_payload) {
        Ok((id, rev)) => return Ok((id, rev)),
        Err(err) => Some(err),
    };

    if let Err(err) = seed_sync_cookie_in_window(app, sync_token) {
        eprintln!("[send2boox][warn] 预写入同步 cookie 失败: {err}");
    }

    if let Ok((browser_status, browser_text)) =
        browser_fetch_raw(app, "PUT", &doc_url, Some(&doc_payload))
    {
        if (200..300).contains(&browser_status) {
            let value: Value =
                serde_json::from_str(&browser_text).map_err(|err| err.to_string())?;
            let id = json_field_to_string(&value, "id").unwrap_or(message_id.clone());
            let rev = json_field_to_string(&value, "rev")
                .ok_or_else(|| format!("neocloud 返回缺少 rev: {}", browser_text))?;
            return Ok((id, rev));
        }
    }

    let mut cookie_parts = vec![
        format!("{cookie_name}={session_id}"),
        format!("SyncGatewaySession={session_id}"),
    ];
    if let Some(auth_cookie) = auth.cookie.as_deref().filter(|v| !v.trim().is_empty()) {
        cookie_parts.push(auth_cookie.to_string());
    }
    let mut request = client
        .put(&doc_url)
        .header(CONTENT_TYPE, "application/json")
        .header(COOKIE, cookie_parts.join("; "))
        .body(doc_payload.clone());
    if let Some(token) = auth.bearer.as_deref().filter(|v| !v.trim().is_empty()) {
        request = request.header(AUTHORIZATION, format!("Bearer {token}"));
    }
    let response = request.send().map_err(|err| err.to_string())?;
    let status = response.status();
    let text = response.text().map_err(|err| err.to_string())?;
    let mut final_status = status.as_u16() as i64;
    let mut final_text = text.clone();
    let mut fallback_err: Option<String> = None;
    if !status.is_success() {
        let need_browser_fallback =
            status.as_u16() == 403 && text.to_ascii_lowercase().contains("user is not provided");
        if need_browser_fallback {
            match browser_fetch_raw(app, "PUT", &doc_url, Some(&doc_payload)) {
                Ok((browser_status, browser_text)) => {
                    final_status = browser_status;
                    final_text = browser_text;
                }
                Err(err) => {
                    fallback_err = Some(err);
                }
            }
        }
    }
    if !(200..300).contains(&final_status) {
        let mut detail = format!("写入 neocloud 失败 (HTTP {}): {}", final_status, final_text);
        if let Some(err) = fallback_err {
            detail.push_str(&format!(" | 浏览器回退失败: {}", err));
        }
        if let Some(err) = pouch_err {
            detail.push_str(&format!(" | 浏览器PouchDB失败: {}", err));
        }
        return Err(detail);
    }
    let value: Value = serde_json::from_str(&final_text).map_err(|err| err.to_string())?;
    let id = json_field_to_string(&value, "id").unwrap_or(message_id);
    let rev = json_field_to_string(&value, "rev")
        .ok_or_else(|| format!("neocloud 返回缺少 rev: {}", final_text))?;
    Ok((id, rev))
}

fn verify_oss_object_via_signed_url(client: &Client, signed_url: &str) -> Result<bool, String> {
    let response = client
        .get(signed_url)
        .header("Range", "bytes=0-0")
        .send()
        .map_err(|err| err.to_string())?;
    let status = response.status();
    Ok(status.as_u16() == 200 || status.as_u16() == 206)
}

fn json_field_to_string(value: &Value, key: &str) -> Option<String> {
    let field = value.get(key)?;
    if let Some(text) = field.as_str() {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return None;
        }
        return Some(trimmed.to_string());
    }
    if let Some(num) = field.as_u64() {
        return Some(num.to_string());
    }
    if let Some(num) = field.as_i64() {
        return Some(num.to_string());
    }
    None
}

fn json_field_to_bool(value: &Value, key: &str) -> Option<bool> {
    parse_bool_field(value, key)
}

fn value_to_array(value: Value) -> Vec<Value> {
    if let Some(items) = value.as_array() {
        return items.clone();
    }
    if let Some(items) = value.get("rows").and_then(Value::as_array) {
        return items.clone();
    }
    if let Some(items) = value.get("list").and_then(Value::as_array) {
        return items.clone();
    }
    if let Some(items) = value.get("devices").and_then(Value::as_array) {
        return items.clone();
    }
    Vec::new()
}

fn parse_ipv4_from_text(text: &str) -> Option<Ipv4Addr> {
    for token in text.split(|ch: char| !(ch.is_ascii_digit() || ch == '.')) {
        if token.is_empty() {
            continue;
        }
        if let Ok(ip) = token.parse::<Ipv4Addr>() {
            return Some(ip);
        }
    }
    None
}

fn normalize_mac_address(text: &str) -> Option<String> {
    let hex: String = text.chars().filter(|ch| ch.is_ascii_hexdigit()).collect();
    if hex.len() != 12 {
        return None;
    }
    let lower = hex.to_ascii_lowercase();
    let mut out = String::with_capacity(17);
    for (idx, ch) in lower.chars().enumerate() {
        if idx > 0 && idx % 2 == 0 {
            out.push(':');
        }
        out.push(ch);
    }
    Some(out)
}

fn collect_arp_neighbors() -> HashMap<String, String> {
    let output = match Command::new("arp").arg("-an").output() {
        Ok(output) if output.status.success() => output,
        _ => return HashMap::new(),
    };

    let text = String::from_utf8_lossy(&output.stdout);
    let mut neighbors = HashMap::new();
    for line in text.lines() {
        let Some(open_idx) = line.find('(') else {
            continue;
        };
        let after_open = &line[(open_idx + 1)..];
        let Some(close_rel) = after_open.find(')') else {
            continue;
        };
        let ip_raw = &after_open[..close_rel];
        let Some(ip) = parse_ipv4_from_text(ip_raw).map(|value| value.to_string()) else {
            continue;
        };

        let Some(at_idx) = line.find(" at ") else {
            continue;
        };
        let after_at = &line[(at_idx + 4)..];
        let mac_token = after_at.split_whitespace().next().unwrap_or_default();
        let Some(mac) = normalize_mac_address(mac_token) else {
            continue;
        };
        neighbors.insert(mac, ip);
    }
    neighbors
}

fn detect_local_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let addr = socket.local_addr().ok()?;
    match addr.ip() {
        IpAddr::V4(ipv4) => Some(ipv4),
        IpAddr::V6(_) => None,
    }
}

fn in_same_lan_c24(local: Ipv4Addr, other: Ipv4Addr) -> bool {
    let a = local.octets();
    let b = other.octets();
    a[0] == b[0] && a[1] == b[1] && a[2] == b[2]
}

fn extract_device_ip(item: &Value) -> Option<String> {
    [
        "ipAddress",
        "ip",
        "localIp",
        "localIP",
        "lanIp",
        "deviceIp",
        "lastIp",
        "latestIp",
    ]
    .iter()
    .find_map(|key| json_field_to_string(item, key))
}

fn current_upload_snapshot(app: &tauri::AppHandle) -> DashboardUploadState {
    let state = get_upload_runtime_state(app);
    DashboardUploadState {
        in_progress: state.in_progress,
        status_text: state.status_text,
        last_error: state.last_error,
        current_file: state.current_file,
        bytes_sent: state.bytes_sent,
        bytes_total: state.bytes_total,
        progress_percent: state.progress_percent,
        speed_bps: state.speed_bps,
        eta_seconds: state.eta_seconds,
        updated_ms: state.updated_ms,
    }
}

fn unauthorized_dashboard_snapshot(app: &tauri::AppHandle, reason: String) -> DashboardSnapshot {
    DashboardSnapshot {
        auth: DashboardAuth {
            authorized: false,
            source: "none".to_string(),
            message: reason,
        },
        profile: None,
        storage: DashboardStorage {
            used: None,
            limit: None,
            percent: None,
        },
        devices: Vec::new(),
        push_queue: Vec::new(),
        calendar_metrics: DashboardCalendarMetrics {
            reading_info: Value::Null,
            read_time_week: Value::Null,
            day_read_today: Value::Null,
        },
        upload: current_upload_snapshot(app),
        fetched_at_ms: unix_ms_now(),
    }
}

fn build_dashboard_snapshot(app: &tauri::AppHandle) -> DashboardSnapshot {
    if app.get_window(MAIN_LABEL).is_some() {
        refresh_cached_auth_token(app);
    }

    let client = match Client::builder().timeout(Duration::from_secs(20)).build() {
        Ok(client) => client,
        Err(err) => {
            return unauthorized_dashboard_snapshot(app, format!("创建网络客户端失败: {err}"));
        }
    };

    let (auth, user_data) = match fetch_auth_context_and_user(&client, app) {
        Ok(value) => value,
        Err(err) => return unauthorized_dashboard_snapshot(app, err),
    };

    let source = auth_source_text(&auth);
    let used = auth.storage_used;
    let limit = auth.storage_limit;
    let percent = match (used, limit) {
        (Some(used), Some(limit)) if limit > 0 => Some((used as f64 / limit as f64) * 100.0),
        _ => None,
    };

    let devices_value = fetch_api_data_for_auth(
        &client,
        app,
        &auth,
        "https://send2boox.com/api/1/users/getDevice",
    )
    .unwrap_or(Value::Null);
    let local_ipv4 = detect_local_ipv4();
    let arp_neighbors = collect_arp_neighbors();
    let devices = value_to_array(devices_value)
        .into_iter()
        .map(|item| {
            let mac_address = json_field_to_string(&item, "macAddress")
                .or_else(|| json_field_to_string(&item, "mac"));
            let normalized_mac = mac_address
                .as_deref()
                .and_then(normalize_mac_address);
            let lan_ip = normalized_mac
                .as_ref()
                .and_then(|mac| arp_neighbors.get(mac).cloned());
            let ip_address = extract_device_ip(&item);
            let same_lan_by_ip = match (
                local_ipv4,
                ip_address.as_deref().and_then(parse_ipv4_from_text),
            ) {
                (Some(local), Some(remote)) => in_same_lan_c24(local, remote),
                _ => false,
            };

            DashboardDevice {
                id: json_field_to_string(&item, "id")
                    .or_else(|| json_field_to_string(&item, "_id"))
                    .or_else(|| json_field_to_string(&item, "deviceId")),
                model: json_field_to_string(&item, "model")
                    .or_else(|| json_field_to_string(&item, "deviceModel")),
                mac_address,
                ip_address,
                login_status: json_field_to_string(&item, "loginStatus"),
                latest_login_time: json_field_to_string(&item, "latestLoginTime"),
                latest_logout_time: json_field_to_string(&item, "latestLogoutTime"),
                locked: json_field_to_bool(&item, "lock"),
                same_lan: lan_ip.is_some() || same_lan_by_ip,
                lan_ip,
            }
        })
        .collect::<Vec<_>>();

    let push_queue = fetch_push_queue_for_dashboard(app, &auth.uid);
    let today = today_ymd();
    let reading_info = fetch_api_data_for_auth(
        &client,
        app,
        &auth,
        "https://send2boox.com/api/1/statistics/readingInfo",
    )
    .unwrap_or(Value::Null);
    let read_time_week = fetch_api_data_for_auth(
        &client,
        app,
        &auth,
        &format!("https://send2boox.com/api/1/statistics/readTimeInfo?date={today}&range=week"),
    )
    .unwrap_or(Value::Null);
    let day_read_today = fetch_api_data_for_auth(
        &client,
        app,
        &auth,
        &format!(
            "https://send2boox.com/api/1/statistics/dayRead?startTime={today}&endTime={today}"
        ),
    )
    .unwrap_or(Value::Null);

    DashboardSnapshot {
        auth: DashboardAuth {
            authorized: true,
            source,
            message: "已授权".to_string(),
        },
        profile: Some(DashboardProfile {
            uid: auth.uid.clone(),
            nickname: json_field_to_string(&user_data, "nickname")
                .or_else(|| json_field_to_string(&user_data, "name")),
            avatar_url: json_field_to_string(&user_data, "avatarUrl")
                .or_else(|| json_field_to_string(&user_data, "avatar")),
        }),
        storage: DashboardStorage {
            used,
            limit,
            percent,
        },
        devices,
        push_queue,
        calendar_metrics: DashboardCalendarMetrics {
            reading_info,
            read_time_week,
            day_read_today,
        },
        upload: current_upload_snapshot(app),
        fetched_at_ms: unix_ms_now(),
    }
}

fn fetch_push_queue_for_dashboard(app: &tauri::AppHandle, uid: &str) -> Vec<DashboardPushItem> {
    match browser_fetch_push_queue_via_pouchdb(app, uid) {
        Ok(items) if !items.is_empty() => return items,
        Ok(_) => {}
        Err(err) => eprintln!("[send2boox][warn] 仪表盘首次读取互动列表失败: {err}"),
    }

    let _ = ensure_main_window(app, AppPage::Upload, false);
    refresh_cached_auth_token(app);
    thread::sleep(Duration::from_millis(900));
    match browser_fetch_push_queue_via_pouchdb(app, uid) {
        Ok(items) if !items.is_empty() => items,
        Ok(items) => items,
        Err(err) => {
            eprintln!("[send2boox][warn] 仪表盘重试读取互动列表失败: {err}");
            Vec::new()
        }
    }
}

fn shorten_for_ui(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let mut out = String::new();
    for (idx, ch) in text.chars().enumerate() {
        if idx >= max_chars {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

fn validate_storage_quota(auth: &UploadAuthContext, files: &[PathBuf]) -> Result<(), String> {
    let total_size: u64 = files
        .iter()
        .filter_map(|path| fs::metadata(path).ok().map(|meta| meta.len()))
        .sum();

    if let (Some(limit), Some(used)) = (auth.storage_limit, auth.storage_used) {
        let remaining = limit.saturating_sub(used);
        if total_size > remaining {
            return Err(format!(
                "可用空间不足，剩余 {} 字节，待上传 {} 字节",
                remaining, total_size
            ));
        }
    }
    Ok(())
}

fn run_upload_diagnostics(app: tauri::AppHandle) {
    let _ = ensure_main_window(&app, AppPage::Recent, false);
    refresh_cached_auth_token(&app);
    thread::spawn(move || {
        let client = match Client::builder().timeout(Duration::from_secs(20)).build() {
            Ok(client) => client,
            Err(err) => {
                report_error(&app, "创建网络客户端失败", &err);
                return;
            }
        };

        let auth = fetch_auth_context(&client, &app);
        let buckets = fetch_buckets(&client);
        let sts_public = fetch_sts(&client, None, None);
        let sts_authed = match &auth {
            Ok(value) => fetch_sts_for_auth(&client, &app, value),
            Err(_) => Err("认证失败，无法验证带登录态 STS".to_string()),
        };

        let auth_msg = match auth {
            Ok(ref value) => format!(
                "认证: 成功 (uid={})\n来源: token={} cookie={} 浏览器会话={}\n空间: used={:?}, limit={:?}",
                value.uid,
                value.bearer.as_ref().map(|_| "是").unwrap_or("否"),
                value.cookie.as_ref().map(|_| "是").unwrap_or("否"),
                if value.use_webview_session {
                    "是"
                } else {
                    "否"
                },
                value.storage_used,
                value.storage_limit
            ),
            Err(err) => format!("认证: 失败 ({err})"),
        };
        let bucket_msg = match buckets {
            Ok(ref map) => format!("buckets: 成功 ({} 个桶)", map.len()),
            Err(err) => format!("buckets: 失败 ({err})"),
        };
        let sts_msg = match sts_public {
            Ok(ref value) => format!("stss(匿名): 成功 (过期时间 {})", value.expiration),
            Err(err) => format!("stss(匿名): 失败 ({err})"),
        };
        let sts_auth_msg = match sts_authed {
            Ok(ref value) => format!("stss(带登录态): 成功 (过期时间 {})", value.expiration),
            Err(err) => format!("stss(带登录态): 失败 ({err})"),
        };

        let body = format!("上传诊断结果\n\n{auth_msg}\n{bucket_msg}\n{sts_msg}\n{sts_auth_msg}");
        message(None::<&tauri::Window>, APP_ALERT_TITLE, body);
    });
}

fn run_native_upload_task(app: tauri::AppHandle, files: Vec<PathBuf>) {
    thread::spawn(move || {
        let result = (|| -> Result<(usize, usize, Vec<String>), String> {
            set_upload_progress_label(&app, "上传进度: 校验会话...");
            let client = Client::builder()
                .timeout(Duration::from_secs(600))
                .build()
                .map_err(|err| err.to_string())?;
            refresh_cached_auth_token(&app);
            let auth = fetch_auth_context(&client, &app)
                .map_err(|err| format!("当前会话未授权，请先点击“登录并授权”\n\n详情: {err}"))?;
            validate_storage_quota(&auth, &files)?;
            let total = files.len();
            let buckets = fetch_buckets(&client)?;
            let sts = fetch_sts_for_auth(&client, &app, &auth)?;
            let sync_token = fetch_sync_token_for_auth(&client, &app, &auth)?;
            if let Err(err) = seed_sync_cookie_in_window(&app, &sync_token) {
                eprintln!("[send2boox][warn] 预写入同步 cookie 失败: {err}");
            }
            let (bucket_key, bucket_cfg) = buckets
                .get_key_value(DEFAULT_BUCKET_KEY)
                .or_else(|| buckets.iter().next())
                .ok_or_else(|| "未获取到可用 bucket 配置".to_string())?;

            let mut success = 0usize;
            let mut failed = 0usize;
            let mut errors: Vec<String> = Vec::new();
            for (index, file) in files.iter().enumerate() {
                let seq = index + 1;
                set_upload_progress_label(&app, &format!("上传进度: {seq}/{total} 准备中"));
                let metadata = match fs::metadata(file) {
                    Ok(value) => value,
                    Err(err) => {
                        failed += 1;
                        log_error("读取文件元信息失败", &err);
                        errors.push(format!(
                            "{}: 读取文件元信息失败: {}",
                            file.to_string_lossy(),
                            err
                        ));
                        continue;
                    }
                };
                if !metadata.is_file() {
                    failed += 1;
                    errors.push(format!("{}: 不是普通文件", file.to_string_lossy()));
                    continue;
                }
                if metadata.len() > MAX_SINGLE_UPLOAD_BYTES {
                    failed += 1;
                    errors.push(format!("{}: 文件超过 200MB 上限", file.to_string_lossy()));
                    continue;
                }
                let Some(file_name) = file.file_name().and_then(|name| name.to_str()) else {
                    failed += 1;
                    errors.push(format!("{}: 文件名解析失败", file.to_string_lossy()));
                    continue;
                };
                let (object_key, resource_type) = build_object_key(&auth.uid, file);
                let file_size = metadata.len();
                set_upload_progress_label(&app, &format!("上传进度: {seq}/{total} 上传中"));
                update_upload_runtime_state(&app, |state| {
                    state.current_file = Some(file_name.to_string());
                    state.bytes_sent = Some(0);
                    state.bytes_total = Some(file_size);
                    state.progress_percent = Some(0.0);
                    state.speed_bps = Some(0.0);
                    state.eta_seconds = None;
                });
                let app_for_progress = app.clone();
                let file_name_for_progress = file_name.to_string();
                if let Err(err) = upload_to_oss(
                    &client,
                    bucket_cfg,
                    &sts,
                    &object_key,
                    file,
                    move |sent, total_bytes, elapsed| {
                        let speed = if elapsed.as_secs_f64() > 0.001 {
                            sent as f64 / elapsed.as_secs_f64()
                        } else {
                            0.0
                        };
                        update_upload_transfer_metrics(
                            &app_for_progress,
                            seq,
                            total,
                            &file_name_for_progress,
                            sent,
                            total_bytes,
                            speed,
                        );
                    },
                ) {
                    failed += 1;
                    eprintln!("[send2boox][error] OSS 上传失败: {err}");
                    errors.push(format!("{file_name}: OSS 上传失败: {err}"));
                    set_upload_progress_label(&app, &format!("上传进度: {seq}/{total} 失败"));
                    continue;
                }
                update_upload_runtime_state(&app, |state| {
                    state.current_file = Some(file_name.to_string());
                    state.bytes_sent = Some(file_size);
                    state.bytes_total = Some(file_size);
                    state.progress_percent = Some(100.0);
                    state.speed_bps = None;
                    state.eta_seconds = Some(0.0);
                });
                set_upload_progress_label(&app, &format!("上传进度: {seq}/{total} 回执校验中"));
                let signed_url =
                    signed_download_url(bucket_cfg, &sts, &object_key).unwrap_or_default();
                let verify_note = if signed_url.is_empty() {
                    "已跳过校验: 无签名链接".to_string()
                } else {
                    match verify_oss_object_via_signed_url(&client, &signed_url) {
                        Ok(true) => "OSS可读".to_string(),
                        Ok(false) => "OSS读取受限(不影响推送)".to_string(),
                        Err(err) => {
                            eprintln!("[send2boox][warn] 上传后可读校验失败(忽略): {err}");
                            format!("OSS读取受限: {}", shorten_for_ui(&err, 48))
                        }
                    }
                };
                set_upload_progress_label(&app, &format!("上传进度: {seq}/{total} 写入推送队列"));
                let (cb_id, cb_rev) = match put_push_message_doc(
                    &app,
                    &client,
                    &auth,
                    &auth.uid,
                    file_name,
                    file_size,
                    &resource_type,
                    &object_key,
                    &signed_url,
                    &sync_token,
                ) {
                    Ok(value) => value,
                    Err(err) => {
                        failed += 1;
                        eprintln!("[send2boox][error] 写入推送消息失败: {err}");
                        errors.push(format!("{file_name}: 写入推送消息失败: {err}"));
                        set_upload_progress_label(&app, &format!("上传进度: {seq}/{total} 失败"));
                        continue;
                    }
                };
                set_upload_progress_label(&app, &format!("上传进度: {seq}/{total} 推送确认中"));
                let save_data = match save_and_push(
                    &app,
                    &client,
                    &auth,
                    bucket_key,
                    &object_key,
                    file_name,
                    &resource_type,
                    &cb_id,
                    &cb_rev,
                ) {
                    Ok(value) => value,
                    Err(err) => {
                        failed += 1;
                        eprintln!("[send2boox][error] saveAndPush 失败: {err}");
                        errors.push(format!("{file_name}: saveAndPush 失败: {err}"));
                        set_upload_progress_label(&app, &format!("上传进度: {seq}/{total} 失败"));
                        continue;
                    }
                };
                let _ = (save_data, verify_note);
                success += 1;
                set_upload_progress_label(&app, &format!("上传进度: {seq}/{total} 完成"));
            }

            Ok((success, failed, errors))
        })();

        finish_upload_task(&app);
        match result {
            Ok((0, failed, errors)) if failed > 0 => {
                set_upload_progress_label(&app, "上传进度: 全部失败");
                clear_upload_transfer_metrics(&app);
                let details = errors
                    .iter()
                    .take(3)
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join("\n");
                let details_for_state = details.clone();
                update_upload_runtime_state(&app, move |state| {
                    state.last_error = Some(details_for_state);
                });
                message(
                    None::<&tauri::Window>,
                    APP_ALERT_TITLE,
                    format!("托盘上传失败：所有文件均未成功上传。\n\n{details}"),
                );
            }
            Ok((success, failed, errors)) => {
                if failed > 0 {
                    set_upload_progress_label(
                        &app,
                        &format!("上传进度: 成功{success} 失败{failed}"),
                    );
                    let details = errors
                        .iter()
                        .take(2)
                        .map(String::as_str)
                        .collect::<Vec<_>>()
                        .join("\n");
                    let details_for_state = details.clone();
                    update_upload_runtime_state(&app, move |state| {
                        state.last_error = Some(details_for_state);
                    });
                    message(
                        None::<&tauri::Window>,
                        APP_ALERT_TITLE,
                        format!("上传完成：成功 {success}，失败 {failed}\n\n失败示例:\n{details}"),
                    );
                } else {
                    set_upload_progress_label(&app, "上传进度: 全部完成");
                    update_upload_runtime_state(&app, |state| {
                        state.last_error = None;
                    });
                }
            }
            Err(err) => {
                set_upload_progress_label(&app, "上传进度: 失败");
                clear_upload_transfer_metrics(&app);
                let err_for_state = err.clone();
                update_upload_runtime_state(&app, move |state| {
                    state.last_error = Some(err_for_state);
                });
                message(
                    None::<&tauri::Window>,
                    APP_ALERT_TITLE,
                    format!("托盘上传失败：{err}"),
                );
            }
        }
    });
}

fn ensure_main_window(app: &tauri::AppHandle, page: AppPage, visible: bool) -> Option<Window> {
    if let Some(window) = app.get_window(MAIN_LABEL) {
        if let Err(err) = window.eval(&redirect_script(page.url())) {
            report_error(app, "切换页面失败", &err);
        }
        if let Err(err) = window.set_title(page.title()) {
            log_error("更新窗口标题失败", &err);
        }
        if visible {
            if let Err(err) = window.show() {
                log_error("显示主窗口失败", &err);
            }
            if let Err(err) = window.set_focus() {
                log_error("聚焦主窗口失败", &err);
            }
        } else if let Err(err) = window.hide() {
            log_error("隐藏主窗口失败", &err);
        }
        return Some(window);
    }

    let parsed_url = match page.url().parse() {
        Ok(value) => value,
        Err(err) => {
            report_error(app, "页面 URL 非法", &err);
            return None;
        }
    };

    match WindowBuilder::new(app, MAIN_LABEL, WindowUrl::External(parsed_url))
        .title(page.title())
        .inner_size(1280.0, 860.0)
        .resizable(true)
        .visible(visible)
        .on_navigation(|url| is_allowed_navigation(url.as_str()))
        .build()
    {
        Ok(window) => Some(window),
        Err(err) => {
            report_error(app, "创建主窗口失败", &err);
            None
        }
    }
}

fn open_or_switch_main(app: &tauri::AppHandle, page: AppPage) {
    let _ = ensure_main_window(app, page, true);
}

fn ensure_dashboard_window(app: &tauri::AppHandle) -> Option<Window> {
    if let Some(window) = app.get_window(DASHBOARD_LABEL) {
        return Some(window);
    }

    match WindowBuilder::new(app, DASHBOARD_LABEL, WindowUrl::App(DASHBOARD_HTML.into()))
        .title("Send2Boox 控制中心")
        .inner_size(DASHBOARD_WIDTH, DASHBOARD_HEIGHT)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .visible(false)
        .skip_taskbar(true)
        .build()
    {
        Ok(window) => Some(window),
        Err(err) => {
            report_error(app, "创建托盘仪表盘失败", &err);
            None
        }
    }
}

fn hide_dashboard_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_window(DASHBOARD_LABEL) {
        if let Err(err) = window.hide() {
            log_error("隐藏仪表盘失败", &err);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn compute_dashboard_position(
    tray_x: f64,
    tray_y: f64,
    tray_w: f64,
    tray_h: f64,
    monitor_x: f64,
    monitor_y: f64,
    monitor_w: f64,
    monitor_h: f64,
) -> (f64, f64) {
    let mut x = tray_x + (tray_w / 2.0) - (DASHBOARD_WIDTH / 2.0);
    let mut y = tray_y + tray_h + DASHBOARD_GAP;
    let min_x = monitor_x + 8.0;
    let max_x = monitor_x + monitor_w - DASHBOARD_WIDTH - 8.0;
    let min_y = monitor_y;
    let max_y = monitor_y + monitor_h - DASHBOARD_HEIGHT - 8.0;
    if max_x >= min_x {
        x = x.clamp(min_x, max_x);
    }
    if max_y >= min_y {
        y = y.clamp(min_y, max_y);
    }
    (x, y)
}

fn show_dashboard_window_near_tray(
    app: &tauri::AppHandle,
    tray_x: f64,
    tray_y: f64,
    tray_w: f64,
    tray_h: f64,
) {
    let Some(window) = ensure_dashboard_window(app) else {
        return;
    };
    let (mut monitor_x, mut monitor_y, mut monitor_w, mut monitor_h) =
        (0.0_f64, 0.0_f64, 1920.0_f64, 1080.0_f64);
    if let Ok(Some(monitor)) = window.current_monitor() {
        let position = monitor.position();
        let size = monitor.size();
        monitor_x = position.x as f64;
        monitor_y = position.y as f64;
        monitor_w = size.width as f64;
        monitor_h = size.height as f64;
    }

    let (x, y) = compute_dashboard_position(
        tray_x, tray_y, tray_w, tray_h, monitor_x, monitor_y, monitor_w, monitor_h,
    );
    if let Err(err) = window.set_position(tauri::PhysicalPosition::new(x, y)) {
        log_error("设置仪表盘位置失败", &err);
    }
    if let Err(err) = window.show() {
        log_error("显示仪表盘失败", &err);
    }
    if let Err(err) = window.set_focus() {
        log_error("聚焦仪表盘失败", &err);
    }
}

fn show_dashboard_window_default(app: &tauri::AppHandle) {
    let Some(window) = ensure_dashboard_window(app) else {
        return;
    };
    let (mut monitor_x, mut monitor_y, mut monitor_w, _) =
        (0.0_f64, 0.0_f64, 1920.0_f64, 1080.0_f64);
    if let Ok(Some(monitor)) = window.current_monitor() {
        let position = monitor.position();
        let size = monitor.size();
        monitor_x = position.x as f64;
        monitor_y = position.y as f64;
        monitor_w = size.width as f64;
    }
    let x = monitor_x + monitor_w - DASHBOARD_WIDTH - 10.0;
    let y = monitor_y + 26.0;
    if let Err(err) = window.set_position(tauri::PhysicalPosition::new(x, y)) {
        log_error("设置默认仪表盘位置失败", &err);
    }
    if let Err(err) = window.show() {
        log_error("显示仪表盘失败", &err);
    }
    if let Err(err) = window.set_focus() {
        log_error("聚焦仪表盘失败", &err);
    }
}

fn show_dashboard_window_from_last_anchor(app: &tauri::AppHandle) {
    if let Some((tray_x, tray_y, tray_w, tray_h)) = get_last_tray_anchor(app) {
        show_dashboard_window_near_tray(app, tray_x, tray_y, tray_w, tray_h);
    } else {
        show_dashboard_window_default(app);
    }
}

fn toggle_dashboard_window(
    app: &tauri::AppHandle,
    tray_x: f64,
    tray_y: f64,
    tray_w: f64,
    tray_h: f64,
) {
    let Some(window) = ensure_dashboard_window(app) else {
        return;
    };

    match window.is_visible() {
        Ok(true) => {
            hide_dashboard_window(app);
            return;
        }
        Ok(false) => {}
        Err(err) => {
            log_error("读取仪表盘可见状态失败", &err);
        }
    }

    show_dashboard_window_near_tray(app, tray_x, tray_y, tray_w, tray_h);
}

fn start_login_flow(app: &tauri::AppHandle) {
    set_login_authorizing(app, true);
    match app.state::<RuntimeState>().cached_auth_state.lock() {
        Ok(mut cached) => *cached = CachedAuthState::default(),
        Err(err) => eprintln!("[send2boox][warn] 重置登录态失败: {err}"),
    }
    if let Ok(mut cache) = app.state::<RuntimeState>().dashboard_cache.lock() {
        *cache = None;
    }
    open_or_switch_main(app, AppPage::Login);
    let app_handle = app.clone();
    thread::spawn(move || {
        for _ in 0..180 {
            thread::sleep(Duration::from_secs(1));
            let waiting = match app_handle.state::<RuntimeState>().login_authorizing.lock() {
                Ok(flag) => *flag,
                Err(_) => false,
            };
            if !waiting {
                return;
            }
            refresh_cached_auth_token(&app_handle);
            let state = get_cached_auth_state(&app_handle);
            if has_auth_state(&state) {
                complete_login_authorization_if_needed(&app_handle);
                return;
            }
        }
        set_login_authorizing(&app_handle, false);
    });
}

fn trigger_upload_from_tray(app: &tauri::AppHandle) {
    if !try_begin_upload_task(app) {
        message(
            None::<&tauri::Window>,
            APP_ALERT_TITLE,
            "已有上传任务在执行，请稍后重试。",
        );
        return;
    }
    set_upload_progress_label(app, "上传进度: 等待选择文件...");
    hide_dashboard_window(app);
    let app_handle = app.clone();
    let run_result = app.run_on_main_thread(move || {
        let app_for_callback = app_handle.clone();
        FileDialogBuilder::new()
            .set_title("选择要上传到 Send2Boox 的文件")
            .pick_files(move |files| {
                let Some(files) = files else {
                    set_upload_progress_label(&app_for_callback, "上传进度: 已取消");
                    clear_upload_transfer_metrics(&app_for_callback);
                    update_upload_runtime_state(&app_for_callback, |state| {
                        state.last_error = None;
                    });
                    finish_upload_task(&app_for_callback);
                    return;
                };
                if files.is_empty() {
                    set_upload_progress_label(&app_for_callback, "上传进度: 已取消");
                    clear_upload_transfer_metrics(&app_for_callback);
                    update_upload_runtime_state(&app_for_callback, |state| {
                        state.last_error = None;
                    });
                    finish_upload_task(&app_for_callback);
                    return;
                }
                run_native_upload_task(app_for_callback.clone(), files);
            });
    });
    if let Err(err) = run_result {
        finish_upload_task(app);
        set_upload_progress_label(app, "上传进度: 失败");
        clear_upload_transfer_metrics(app);
        message(
            None::<&tauri::Window>,
            APP_ALERT_TITLE,
            format!("无法打开文件选择窗口：{err}"),
        );
    }
}

fn ensure_calendar_stats_window(app: &tauri::AppHandle) -> Option<Window> {
    if let Some(window) = app.get_window(CALENDAR_LABEL) {
        return Some(window);
    }

    let parsed_url = match CALENDAR_URL.parse() {
        Ok(value) => value,
        Err(err) => {
            report_error(app, "日历页面 URL 非法", &err);
            return None;
        }
    };

    match WindowBuilder::new(app, CALENDAR_LABEL, WindowUrl::External(parsed_url))
        .title("Send2Boox - Calendar Stats")
        .visible(false)
        .focused(false)
        .resizable(false)
        .inner_size(600.0, 400.0)
        .skip_taskbar(true)
        .on_navigation(|url| is_allowed_navigation(url.as_str()))
        .build()
    {
        Ok(window) => Some(window),
        Err(err) => {
            report_error(app, "创建日历统计后台窗口失败", &err);
            None
        }
    }
}

fn poll_calendar_stats_title(app: tauri::AppHandle) {
    thread::spawn(move || {
        for _ in 0..25 {
            thread::sleep(Duration::from_millis(400));
            let Some(window) = app.get_window(CALENDAR_LABEL) else {
                continue;
            };
            let url_text = window.url().to_string();
            if let Some(text) = parse_calendar_stats_from_url(&url_text) {
                set_calendar_stats_label(&app, &text);
                return;
            }
            let Ok(title) = window.title() else {
                continue;
            };
            if let Some(text) = parse_calendar_stats_title(&title) {
                set_calendar_stats_label(&app, &text);
                return;
            }
        }
        set_calendar_stats_label(&app, "日历统计: 暂无统计");
    });
}

fn refresh_calendar_stats(app: &tauri::AppHandle) {
    set_calendar_stats_label(app, "日历统计: 刷新中...");
    let Some(window) = ensure_calendar_stats_window(app) else {
        return;
    };
    if let Err(err) = window.eval(&redirect_script(CALENDAR_URL)) {
        report_error(app, "切换到日历页面失败", &err);
        return;
    }
    if let Err(err) = window.eval(CALENDAR_STATS_SCRIPT) {
        report_error(app, "提取日历统计失败", &err);
        return;
    }
    poll_calendar_stats_title(app.clone());
}

fn build_auto_launch(app: &tauri::AppHandle) -> Result<AutoLaunch, String> {
    if !AutoLaunch::is_support() {
        return Err("当前平台不支持开机自启动".to_string());
    }

    let exe_path = std::env::current_exe().map_err(|err| err.to_string())?;
    let app_name = app.package_info().name.clone();
    let exe = exe_path.to_string_lossy().to_string();

    let mut builder = AutoLaunchBuilder::new();
    builder.set_app_name(&app_name).set_app_path(&exe);
    #[cfg(target_os = "macos")]
    builder.set_use_launch_agent(true);

    builder.build().map_err(|err| err.to_string())
}

fn is_auto_launch_enabled(app: &tauri::AppHandle) -> bool {
    match build_auto_launch(app) {
        Ok(auto) => auto.is_enabled().unwrap_or(false),
        Err(err) => {
            eprintln!("[send2boox][warn] 获取开机自启动状态失败: {err}");
            false
        }
    }
}

fn sync_autostart_menu_title(app: &tauri::AppHandle) {
    let title = autostart_menu_title(is_auto_launch_enabled(app));
    if let Err(err) = app
        .tray_handle()
        .get_item(TOGGLE_AUTOSTART_ID)
        .set_title(title)
    {
        log_error("更新托盘菜单文案失败", &err);
    }
}

fn initialize_auto_launch_default(app: &tauri::AppHandle) {
    let Some(config_dir) = app.path_resolver().app_config_dir() else {
        sync_autostart_menu_title(app);
        return;
    };

    let marker_path = config_dir.join(AUTOSTART_MARKER);
    if !marker_path.exists() {
        match build_auto_launch(app) {
            Ok(auto) => {
                if let Err(err) = auto.enable() {
                    report_error(app, "首次启用开机自启动失败", &err);
                }
            }
            Err(err) => {
                eprintln!("[send2boox][warn] 初始化开机自启动失败: {err}");
            }
        }
        if let Err(err) = fs::create_dir_all(&config_dir) {
            log_error("创建配置目录失败", &err);
        }
        if let Err(err) = fs::write(marker_path, b"initialized") {
            log_error("写入自启动初始化标记失败", &err);
        }
    }

    sync_autostart_menu_title(app);
}

fn toggle_auto_launch(app: &tauri::AppHandle) {
    match build_auto_launch(app) {
        Ok(auto) => {
            let enabled = auto.is_enabled().unwrap_or(false);
            let result = if enabled {
                auto.disable()
            } else {
                auto.enable()
            };
            if let Err(err) = result {
                report_error(app, "切换开机自启动失败", &err);
            }
        }
        Err(err) => {
            eprintln!("[send2boox][warn] 构建开机自启动配置失败: {err}");
        }
    }
    sync_autostart_menu_title(app);
}

#[cfg(target_os = "macos")]
fn build_system_tray(menu: SystemTrayMenu) -> SystemTray {
    SystemTray::new()
        .with_menu_on_left_click(false)
        .with_menu(menu)
}

#[cfg(not(target_os = "macos"))]
fn build_system_tray(menu: SystemTrayMenu) -> SystemTray {
    SystemTray::new().with_menu(menu)
}

fn main() {
    let open_login = CustomMenuItem::new("open_login", "登录并授权");
    let open_main = CustomMenuItem::new("open_main", "打开主页面");
    let open_upload = CustomMenuItem::new("open_upload", "托盘上传（静默）");
    let upload_diag = CustomMenuItem::new(UPLOAD_DIAG_ID, "上传诊断");
    let upload_progress = CustomMenuItem::new(UPLOAD_PROGRESS_ID, "上传进度: 空闲");
    let calendar_stats = CustomMenuItem::new(CALENDAR_STATS_ID, "日历统计: 未加载");
    let refresh_calendar_item = CustomMenuItem::new(REFRESH_CALENDAR_ID, "刷新日历统计");
    let toggle_autostart = CustomMenuItem::new(TOGGLE_AUTOSTART_ID, "开机自启动: --");
    let quit = CustomMenuItem::new("quit", "退出");
    let page_recent = CustomMenuItem::new("page_recent", "最近笔记");
    let page_upload = CustomMenuItem::new("page_upload", "上传文件");

    let tray_menu = SystemTrayMenu::new()
        .add_item(open_login)
        .add_item(open_main)
        .add_item(open_upload)
        .add_item(upload_diag)
        .add_native_item(SystemTrayMenuItem::Separator)
        .add_item(upload_progress)
        .add_native_item(SystemTrayMenuItem::Separator)
        .add_item(calendar_stats)
        .add_item(refresh_calendar_item)
        .add_native_item(SystemTrayMenuItem::Separator)
        .add_item(toggle_autostart)
        .add_native_item(SystemTrayMenuItem::Separator)
        .add_item(quit);
    let tray = build_system_tray(tray_menu);

    let app_menu = Menu::new().add_submenu(Submenu::new(
        "页面",
        Menu::new().add_item(page_recent).add_item(page_upload),
    ));

    tauri::Builder::default()
        .manage(RuntimeState::default())
        .invoke_handler(tauri::generate_handler![
            app_status,
            dashboard_snapshot,
            dashboard_refresh,
            dashboard_open_main,
            dashboard_login_authorize,
            dashboard_upload_pick_and_send,
            dashboard_push_resend,
            dashboard_push_delete,
            dashboard_hide
        ])
        .menu(app_menu)
        .system_tray(tray)
        .setup(|app| {
            let app_handle = app.app_handle();
            initialize_auto_launch_default(&app_handle);
            hydrate_cached_auth_state(&app_handle);
            refresh_cached_auth_token(&app_handle);
            start_main_auth_sync_poller(&app_handle);
            Ok(())
        })
        .on_system_tray_event(|app, event| match event {
            SystemTrayEvent::LeftClick { position, size, .. } => {
                set_last_tray_anchor(
                    &app.app_handle(),
                    position.x,
                    position.y,
                    size.width,
                    size.height,
                );
                toggle_dashboard_window(
                    &app.app_handle(),
                    position.x,
                    position.y,
                    size.width,
                    size.height,
                );
            }
            SystemTrayEvent::DoubleClick { .. } => {}
            SystemTrayEvent::MenuItemClick { id, .. } => match tray_action_from_id(id.as_str()) {
                TrayAction::OpenLogin => {
                    start_login_flow(&app.app_handle());
                }
                TrayAction::OpenMain => {
                    hide_dashboard_window(&app.app_handle());
                    open_or_switch_main(&app.app_handle(), AppPage::Recent);
                }
                TrayAction::OpenUpload => {
                    hide_dashboard_window(&app.app_handle());
                    trigger_upload_from_tray(&app.app_handle());
                }
                TrayAction::UploadDiag => {
                    run_upload_diagnostics(app.app_handle());
                }
                TrayAction::RefreshCalendarStats => {
                    refresh_calendar_stats(&app.app_handle());
                }
                TrayAction::ToggleAutostart => {
                    toggle_auto_launch(&app.app_handle());
                }
                TrayAction::Quit => {
                    app.exit(0);
                }
                TrayAction::Ignore => {}
            },
            _ => {}
        })
        .on_page_load(|window, payload| {
            if window.label() == MAIN_LABEL {
                if let Some(state) = parse_auth_state_from_url(payload.url()) {
                    set_cached_auth_state(&window.app_handle(), state);
                }
                refresh_cached_auth_token(&window.app_handle());
                if let Ok(mut cache) = window
                    .app_handle()
                    .state::<RuntimeState>()
                    .dashboard_cache
                    .lock()
                {
                    *cache = None;
                }
            }
            if window.label() == CALENDAR_LABEL && is_calendar_url(payload.url()) {
                if let Some(text) = parse_calendar_stats_from_url(payload.url()) {
                    set_calendar_stats_label(&window.app_handle(), &text);
                } else {
                    if let Err(err) = window.eval(CALENDAR_STATS_SCRIPT) {
                        report_error(&window.app_handle(), "提取日历统计失败", &err);
                    }
                    poll_calendar_stats_title(window.app_handle());
                }
            }
        })
        .on_menu_event(|event| match event.menu_item_id() {
            "page_recent" => {
                hide_dashboard_window(&event.window().app_handle());
                open_or_switch_main(&event.window().app_handle(), AppPage::Recent);
            }
            "page_upload" => {
                hide_dashboard_window(&event.window().app_handle());
                open_or_switch_main(&event.window().app_handle(), AppPage::Upload);
            }
            _ => {}
        })
        .on_window_event(|event| {
            if let WindowEvent::CloseRequested { api, .. } = event.event() {
                api.prevent_close();
                if let Err(err) = event.window().hide() {
                    log_error("隐藏窗口失败", &err);
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_metadata_is_stable() {
        assert_eq!(AppPage::Login.title(), "Send2Boox - Login");
        assert_eq!(AppPage::Recent.title(), "Send2Boox - Recent Notes");
        assert_eq!(AppPage::Upload.title(), "Send2Boox - Upload File");
        assert_eq!(AppPage::Login.url(), LOGIN_URL);
        assert_eq!(AppPage::Recent.url(), MAIN_URL);
        assert_eq!(AppPage::Upload.url(), UPLOAD_URL);
    }

    #[test]
    fn escape_js_escapes_quotes_and_backslashes() {
        let original = r#"https://send2boox.com/#/push/file?name=foo\'bar"#;
        let escaped = escape_js(original);
        assert!(escaped.contains("\\\\"));
        assert!(escaped.contains("\\'"));
    }

    #[test]
    fn redirect_script_contains_location_replace() {
        let script = redirect_script(UPLOAD_URL);
        assert!(script.contains("window.location.replace"));
        assert!(script.contains(UPLOAD_URL));
    }

    #[test]
    fn allows_only_expected_https_hosts() {
        assert!(is_allowed_navigation(
            "https://send2boox.com/#/note/recentNote"
        ));
        assert!(is_allowed_navigation(
            "https://www.send2boox.com/#/push/file"
        ));
        assert!(!is_allowed_navigation("http://send2boox.com/#/push/file"));
        assert!(!is_allowed_navigation("https://example.com/#/push/file"));
        assert!(!is_allowed_navigation("javascript:alert(1)"));
    }

    #[test]
    fn tray_action_mapping_is_correct() {
        assert_eq!(tray_action_from_id("open_login"), TrayAction::OpenLogin);
        assert_eq!(tray_action_from_id("open_main"), TrayAction::OpenMain);
        assert_eq!(tray_action_from_id("open_upload"), TrayAction::OpenUpload);
        assert_eq!(tray_action_from_id(UPLOAD_DIAG_ID), TrayAction::UploadDiag);
        assert_eq!(
            tray_action_from_id(REFRESH_CALENDAR_ID),
            TrayAction::RefreshCalendarStats
        );
        assert_eq!(
            tray_action_from_id(TOGGLE_AUTOSTART_ID),
            TrayAction::ToggleAutostart
        );
        assert_eq!(tray_action_from_id("quit"), TrayAction::Quit);
        assert_eq!(tray_action_from_id("unknown"), TrayAction::Ignore);
    }

    #[test]
    fn upload_url_matching_is_hash_based() {
        assert!(is_upload_url("https://send2boox.com/#/push/file"));
        assert!(is_upload_url("https://send2boox.com/#/push/file?from=tray"));
        assert!(!is_upload_url("https://send2boox.com/#/note/recentNote"));
    }

    #[test]
    fn calendar_url_matching_is_hash_based() {
        assert!(is_calendar_url("https://send2boox.com/#/calendar"));
        assert!(is_calendar_url(
            "https://send2boox.com/#/calendar?view=month"
        ));
        assert!(!is_calendar_url("https://send2boox.com/#/push/file"));
    }

    #[test]
    fn parse_calendar_stats_from_title_prefix() {
        let parsed = parse_calendar_stats_title("S2B_CAL_STATS::今日:3 | 本月:10");
        assert_eq!(parsed.as_deref(), Some("日历统计: 今日:3 | 本月:10"));
        assert!(parse_calendar_stats_title("random title").is_none());
    }

    #[test]
    fn parse_calendar_stats_from_url_hash_query() {
        let input = "https://send2boox.com/#/calendar?trayStats=%E4%BB%8A%E6%97%A5%3A3%20%7C%20%E6%9C%AC%E6%9C%88%3A10";
        let parsed = parse_calendar_stats_from_url(input);
        assert_eq!(parsed.as_deref(), Some("日历统计: 今日:3 | 本月:10"));
        assert!(parse_calendar_stats_from_url("https://send2boox.com/#/calendar").is_none());
    }

    #[test]
    fn autostart_menu_label_reflects_state() {
        assert_eq!(autostart_menu_title(true), "开机自启动: 开");
        assert_eq!(autostart_menu_title(false), "开机自启动: 关");
    }

    #[test]
    fn dashboard_position_is_clamped_within_monitor() {
        let (x, y) = compute_dashboard_position(1900.0, 0.0, 24.0, 24.0, 0.0, 0.0, 1920.0, 1080.0);
        assert!(x >= 0.0);
        assert!(x + DASHBOARD_WIDTH <= 1920.0);
        assert!(y >= 0.0);
        assert!(y + DASHBOARD_HEIGHT <= 1080.0);
    }

    #[test]
    fn dashboard_position_clamps_when_below_overflows() {
        let (_, y) =
            compute_dashboard_position(960.0, 1040.0, 24.0, 24.0, 0.0, 0.0, 1920.0, 1080.0);
        assert!(y <= 1040.0);
        assert!(y + DASHBOARD_HEIGHT <= 1080.0);
    }
}

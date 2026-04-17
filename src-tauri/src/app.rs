use crate::dashboard::{build_dashboard_snapshot, build_reading_metrics_label};
use crate::models::DashboardSnapshot;
use crate::state::{hydrate_auth_state, update_upload_runtime_state, RuntimeState};
use auto_launch::{AutoLaunch, AutoLaunchBuilder};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::{fs, process::Command, sync::OnceLock};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, WebviewUrl, WindowEvent,
};
use tauri_plugin_dialog::DialogExt;

pub const MAIN_LABEL: &str = "dashboard";
const DASHBOARD_HTML: &str = "dashboard.html";
const DASHBOARD_WIDTH: f64 = 1120.0;
const DASHBOARD_HEIGHT: f64 = 820.0;
const DASHBOARD_GAP: f64 = 12.0;
const APP_ALERT_TITLE: &str = "Send2Boox 控制中心";
const TOGGLE_AUTOSTART_ID: &str = "toggle_autostart";
const CALENDAR_STATS_ID: &str = "calendar_stats_status";
const UPLOAD_PROGRESS_ID: &str = "upload_progress_status";
const REFRESH_DASHBOARD_ID: &str = "refresh_dashboard";
const AUTOSTART_MARKER: &str = "autostart_initialized";
const TRAY_ID: &str = "main";

struct TrayMenuHandles {
    upload_progress: MenuItem<tauri::Wry>,
    calendar_stats: MenuItem<tauri::Wry>,
    toggle_autostart: MenuItem<tauri::Wry>,
}

static TRAY_MENU_HANDLES: OnceLock<TrayMenuHandles> = OnceLock::new();

pub fn truncate_menu_title(value: &str) -> String {
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

pub fn set_calendar_stats_label(_app: &tauri::AppHandle, text: &str) {
    let value = truncate_menu_title(text);
    if let Some(handles) = TRAY_MENU_HANDLES.get() {
        let _ = handles.calendar_stats.set_text(value);
    }
}

pub fn set_upload_progress_label(app: &tauri::AppHandle, text: &str) {
    let value = truncate_menu_title(text);
    if let Some(handles) = TRAY_MENU_HANDLES.get() {
        let _ = handles.upload_progress.set_text(value);
    }
    let text_owned = text.to_string();
    update_upload_runtime_state(app, move |state| {
        state.status_text = text_owned;
    });
}

pub fn clear_upload_transfer_metrics(app: &tauri::AppHandle) {
    update_upload_runtime_state(app, |state| {
        state.current_file = None;
        state.bytes_sent = None;
        state.bytes_total = None;
        state.progress_percent = None;
        state.speed_bps = None;
        state.eta_seconds = None;
    });
}

pub fn update_upload_transfer_metrics(
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

    set_upload_progress_label(
        app,
        &format!("上传进度: {seq}/{total} {percent:.1}% {file_name}"),
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

pub fn sync_reading_metrics_menu_from_snapshot(
    app: &tauri::AppHandle,
    snapshot: &DashboardSnapshot,
) {
    set_calendar_stats_label(app, &build_reading_metrics_label(snapshot));
}

pub fn ensure_dashboard_window(app: &tauri::AppHandle) -> Option<tauri::WebviewWindow> {
    if let Some(window) = app.get_webview_window(MAIN_LABEL) {
        return Some(window);
    }

    tauri::WebviewWindowBuilder::new(app, MAIN_LABEL, WebviewUrl::App(DASHBOARD_HTML.into()))
        .title("Send2Boox 控制中心")
        .inner_size(DASHBOARD_WIDTH, DASHBOARD_HEIGHT)
        .resizable(true)
        .visible(false)
        .build()
        .ok()
}

pub fn hide_dashboard_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_LABEL) {
        let _ = window.hide();
    }
}

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
    let min_x = monitor_x + 8.0;
    let max_x = monitor_x + monitor_w - DASHBOARD_WIDTH - 8.0;
    let space_above = tray_y - monitor_y;
    let space_below = (monitor_y + monitor_h) - (tray_y + tray_h);
    let prefer_above = space_above >= DASHBOARD_HEIGHT + DASHBOARD_GAP || space_above > space_below;
    let mut y = if prefer_above {
        tray_y - DASHBOARD_HEIGHT - DASHBOARD_GAP
    } else {
        tray_y + tray_h + DASHBOARD_GAP
    };
    let min_y = monitor_y + 8.0;
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
    let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
    let _ = window.show();
    let _ = window.set_focus();
}

pub fn show_dashboard_window_default(app: &tauri::AppHandle) {
    let Some(window) = ensure_dashboard_window(app) else {
        return;
    };
    let _ = window.show();
    let _ = window.set_focus();
}

pub fn show_dashboard_window_from_last_anchor(app: &tauri::AppHandle) {
    if let Ok(anchor) = app.state::<RuntimeState>().last_tray_anchor.lock() {
        if let Some((tray_x, tray_y, tray_w, tray_h)) = *anchor {
            show_dashboard_window_near_tray(app, tray_x, tray_y, tray_w, tray_h);
            return;
        }
    }
    show_dashboard_window_default(app);
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
        Err(_) => {}
    }
    show_dashboard_window_near_tray(app, tray_x, tray_y, tray_w, tray_h);
}

#[cfg(target_os = "macos")]
pub fn open_external_url(url: &str) -> Result<(), String> {
    let status = Command::new("open")
        .arg(url)
        .status()
        .map_err(|err| err.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("打开失败，退出码: {:?}", status.code()))
    }
}

#[cfg(target_os = "windows")]
pub fn open_external_url(url: &str) -> Result<(), String> {
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let status = Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|err| err.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("打开失败，退出码: {:?}", status.code()))
    }
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
pub fn open_external_url(url: &str) -> Result<(), String> {
    let status = Command::new("xdg-open")
        .arg(url)
        .status()
        .map_err(|err| err.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("打开失败，退出码: {:?}", status.code()))
    }
}

fn autostart_menu_title(enabled: bool) -> &'static str {
    if enabled {
        "开机自启动: 开"
    } else {
        "开机自启动: 关"
    }
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
        Err(_) => false,
    }
}

fn sync_autostart_menu_title(app: &tauri::AppHandle) {
    let title = autostart_menu_title(is_auto_launch_enabled(app));
    if let Some(handles) = TRAY_MENU_HANDLES.get() {
        let _ = handles.toggle_autostart.set_text(title);
    }
}

fn initialize_auto_launch_default(app: &tauri::AppHandle) {
    let Ok(config_dir) = app.path().app_config_dir() else {
        sync_autostart_menu_title(app);
        return;
    };

    let marker_path = config_dir.join(AUTOSTART_MARKER);
    if !marker_path.exists() {
        if let Ok(auto) = build_auto_launch(app) {
            let _ = auto.enable();
        }
        let _ = fs::create_dir_all(&config_dir);
        let _ = fs::write(marker_path, b"initialized");
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
                show_alert(app, format!("切换开机自启动失败：{err}"));
            }
        }
        Err(err) => {
            show_alert(app, format!("构建开机自启动配置失败：{err}"));
        }
    }
    sync_autostart_menu_title(app);
}

fn show_alert(app: &tauri::AppHandle, message: impl Into<String>) {
    app.dialog()
        .message(message.into())
        .title(APP_ALERT_TITLE)
        .show(|_| {});
}

fn build_tray_menu(
    app: &tauri::AppHandle,
) -> Result<(Menu<tauri::Wry>, TrayMenuHandles), tauri::Error> {
    let open_login = MenuItem::with_id(app, "open_login", "登录并授权", true, None::<&str>)?;
    let open_upload = MenuItem::with_id(app, "open_upload", "上传文件", true, None::<&str>)?;
    let refresh_dashboard =
        MenuItem::with_id(app, REFRESH_DASHBOARD_ID, "刷新仪表盘", true, None::<&str>)?;
    let upload_progress = MenuItem::with_id(
        app,
        UPLOAD_PROGRESS_ID,
        "上传进度: 空闲",
        true,
        None::<&str>,
    )?;
    let calendar_stats = MenuItem::with_id(
        app,
        CALENDAR_STATS_ID,
        "阅读统计指标: 未加载",
        true,
        None::<&str>,
    )?;
    let toggle_autostart = MenuItem::with_id(
        app,
        TOGGLE_AUTOSTART_ID,
        "开机自启动: --",
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let sep3 = PredefinedMenuItem::separator(app)?;
    let sep4 = PredefinedMenuItem::separator(app)?;

    let tray_menu = Menu::with_items(
        app,
        &[
            &open_login,
            &open_upload,
            &refresh_dashboard,
            &sep1,
            &upload_progress,
            &sep2,
            &calendar_stats,
            &sep3,
            &toggle_autostart,
            &sep4,
            &quit,
        ],
    )?;

    Ok((
        tray_menu,
        TrayMenuHandles {
            upload_progress,
            calendar_stats,
            toggle_autostart,
        },
    ))
}

fn build_app_menu(app: &tauri::AppHandle) -> Result<Menu<tauri::Wry>, tauri::Error> {
    let show_dashboard =
        MenuItem::with_id(app, "show_dashboard", "显示主界面", true, None::<&str>)?;
    let window_submenu = Submenu::with_items(app, "窗口", true, &[&show_dashboard])?;
    Menu::with_items(app, &[&window_submenu])
}

fn initialize_tray(app: &tauri::AppHandle) -> Result<(), String> {
    let (tray_menu, handles) = build_tray_menu(app).map_err(|err| err.to_string())?;
    let _ = TRAY_MENU_HANDLES.set(handles);

    let tray = if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_menu(Some(tray_menu))
            .map_err(|err| err.to_string())?;
        let _ = tray.set_show_menu_on_left_click(false);
        tray
    } else {
        TrayIconBuilder::with_id(TRAY_ID)
            .menu(&tray_menu)
            .icon_as_template(false)
            .show_menu_on_left_click(false)
            .build(app)
            .map_err(|err| err.to_string())?
    };

    if let Some(icon) = app.default_window_icon().cloned() {
        let _ = tray.set_icon(Some(icon));
    }
    let _ = tray.set_icon_as_template(false);

    Ok(())
}

fn refresh_snapshot_in_background(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let app_for_task = app.clone();
        let snapshot =
            tauri::async_runtime::spawn_blocking(move || build_dashboard_snapshot(&app_for_task))
                .await;
        if let Ok(snapshot) = snapshot {
            crate::state::set_dashboard_cache(&app, snapshot.clone());
            sync_reading_metrics_menu_from_snapshot(&app, &snapshot);
        }
    });
}

pub fn run() {
    tauri::Builder::default()
        .manage(RuntimeState::default())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            crate::diagnostics::app_diagnostics,
            crate::dashboard::app_status,
            crate::dashboard::dashboard_snapshot,
            crate::dashboard::dashboard_refresh,
            crate::dashboard::dashboard_login_authorize,
            crate::push::dashboard_upload_pick_and_send,
            crate::push::dashboard_open_transfer_host,
            crate::push::dashboard_push_resend,
            crate::push::dashboard_push_delete,
            crate::dashboard::dashboard_hide,
            crate::zotero::zotero_status,
            crate::zotero::zotero_detect_config,
            crate::zotero::zotero_pick_profile_dir,
            crate::zotero::zotero_pick_data_dir,
            crate::zotero::zotero_save_and_validate,
            crate::zotero::zotero_list_recent_items,
            crate::zotero::zotero_push_attachment
        ])
        .setup(|app| {
            let app_handle = app.app_handle();
            crate::diagnostics::init(&app_handle);
            crate::diagnostics::info(
                "app",
                format!("应用启动 version={}", app_handle.package_info().version),
            );
            let app_menu = build_app_menu(&app_handle).map_err(|err| err.to_string())?;
            let _ = app_handle.set_menu(app_menu);
            initialize_tray(&app_handle)?;
            initialize_auto_launch_default(&app_handle);
            hydrate_auth_state(&app_handle);
            ensure_dashboard_window(&app_handle);
            Ok(())
        })
        .on_tray_icon_event(|app, event| {
            if let TrayIconEvent::Click {
                position,
                rect,
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let tray_size = rect.size.to_logical::<f64>(1.0);
                if let Ok(mut anchor) = app.state::<RuntimeState>().last_tray_anchor.lock() {
                    *anchor = Some((position.x, position.y, tray_size.width, tray_size.height));
                }
                toggle_dashboard_window(
                    app,
                    position.x,
                    position.y,
                    tray_size.width,
                    tray_size.height,
                );
            }
        })
        .on_menu_event(|app, event| {
            let id = event.id();
            if id == "show_dashboard" {
                show_dashboard_window_from_last_anchor(app);
                return;
            }
            if id == "open_login" {
                let _ = crate::auth::start_login_flow(app);
            } else if id == "open_upload" {
                let _ = crate::push::dashboard_upload_pick_and_send(app.clone());
            } else if id == REFRESH_DASHBOARD_ID {
                refresh_snapshot_in_background(app.clone());
            } else if id == TOGGLE_AUTOSTART_ID {
                toggle_auto_launch(app);
            } else if id == "quit" {
                app.exit(0);
            }
        })
        .on_window_event(|window, event| {
            if window.label() == MAIN_LABEL {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    hide_dashboard_window(&window.app_handle());
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
    fn dashboard_position_prefers_above_bottom_tray() {
        let (_, y) =
            compute_dashboard_position(1820.0, 1040.0, 24.0, 24.0, 0.0, 0.0, 1920.0, 1080.0);
        assert!(y < 1040.0);
    }
}

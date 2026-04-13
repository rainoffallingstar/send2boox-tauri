use crate::models::{AuthState, DashboardSnapshot, PersistedAuthState, UploadRuntimeState};
use crate::util::{normalize_optional, unix_ms_now};
use std::{fs, path::PathBuf, sync::Mutex};
use tauri::Manager;

const AUTH_CACHE_FILE: &str = "auth_cache.json";

#[derive(Debug, Clone)]
pub struct LoginPortalState {
    pub _state: String,
    pub _port: u16,
    pub _started_ms: u128,
}

#[derive(Default)]
pub struct RuntimeState {
    pub upload_in_progress: Mutex<bool>,
    pub auth_state: Mutex<AuthState>,
    pub upload_runtime_state: Mutex<UploadRuntimeState>,
    pub dashboard_cache: Mutex<Option<DashboardSnapshot>>,
    pub last_tray_anchor: Mutex<Option<(f64, f64, f64, f64)>>,
    pub login_portal: Mutex<Option<LoginPortalState>>,
}

fn auth_cache_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    let mut dir = app.path().app_data_dir().ok()?;
    if fs::create_dir_all(&dir).is_err() {
        return None;
    }
    dir.push(AUTH_CACHE_FILE);
    Some(dir)
}

pub fn hydrate_auth_state(app: &tauri::AppHandle) {
    let path = match auth_cache_path(app) {
        Some(path) => path,
        None => return,
    };
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(_) => return,
    };
    let persisted: PersistedAuthState = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(_) => return,
    };
    if let Ok(mut state) = app.state::<RuntimeState>().auth_state.lock() {
        state.token = normalize_optional(persisted.token);
        state.updated_ms = persisted.updated_ms;
    }
}

pub fn get_auth_state(app: &tauri::AppHandle) -> AuthState {
    match app.state::<RuntimeState>().auth_state.lock() {
        Ok(state) => state.clone(),
        Err(_) => AuthState::default(),
    }
}

pub fn set_auth_state(app: &tauri::AppHandle, token: Option<String>) {
    let normalized = normalize_optional(token);
    let updated_ms = Some(unix_ms_now());
    if let Ok(mut state) = app.state::<RuntimeState>().auth_state.lock() {
        state.token = normalized.clone();
        state.updated_ms = updated_ms;
    }

    if let Ok(mut cache) = app.state::<RuntimeState>().dashboard_cache.lock() {
        *cache = None;
    }

    if let Some(path) = auth_cache_path(app) {
        if let Some(token_value) = normalized {
            let payload = PersistedAuthState {
                token: Some(token_value),
                updated_ms,
            };
            if let Ok(text) = serde_json::to_string(&payload) {
                let _ = fs::write(path, text);
            }
        } else {
            let _ = fs::remove_file(path);
        }
    }
}

pub fn get_dashboard_cache(app: &tauri::AppHandle, max_age_ms: u128) -> Option<DashboardSnapshot> {
    let now = unix_ms_now();
    match app.state::<RuntimeState>().dashboard_cache.lock() {
        Ok(cache) => cache.clone().filter(|snap| {
            let age = now.saturating_sub(snap.fetched_at_ms);
            age <= max_age_ms
        }),
        Err(_) => None,
    }
}

pub fn set_dashboard_cache(app: &tauri::AppHandle, snapshot: DashboardSnapshot) {
    if let Ok(mut cache) = app.state::<RuntimeState>().dashboard_cache.lock() {
        *cache = Some(snapshot);
    }
}

pub fn update_dashboard_cache_after_delete(
    app: &tauri::AppHandle,
    deleted_id: &str,
) -> Option<DashboardSnapshot> {
    match app.state::<RuntimeState>().dashboard_cache.lock() {
        Ok(mut cache) => {
            let snapshot = cache.as_mut()?;
            snapshot.push_queue.retain(|item| item.id != deleted_id);
            snapshot.fetched_at_ms = unix_ms_now();
            Some(snapshot.clone())
        }
        Err(_) => None,
    }
}

pub fn get_upload_runtime_state(app: &tauri::AppHandle) -> UploadRuntimeState {
    match app.state::<RuntimeState>().upload_runtime_state.lock() {
        Ok(state) => state.clone(),
        Err(_) => UploadRuntimeState::default(),
    }
}

pub fn update_upload_runtime_state<F>(app: &tauri::AppHandle, mutator: F)
where
    F: FnOnce(&mut UploadRuntimeState),
{
    if let Ok(mut state) = app.state::<RuntimeState>().upload_runtime_state.lock() {
        mutator(&mut state);
        state.updated_ms = unix_ms_now();
    }
}

pub fn try_begin_upload_task(app: &tauri::AppHandle) -> bool {
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
        Err(_) => false,
    }
}

pub fn finish_upload_task(app: &tauri::AppHandle) {
    if let Ok(mut in_progress) = app.state::<RuntimeState>().upload_in_progress.lock() {
        *in_progress = false;
    }
    update_upload_runtime_state(app, |state| {
        state.in_progress = false;
        state.speed_bps = None;
        state.eta_seconds = None;
    });
}

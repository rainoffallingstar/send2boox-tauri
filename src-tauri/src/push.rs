use crate::api::{
    batch_delete_push_message, create_client, fetch_auth_context_and_user, fetch_default_bucket,
    fetch_push_doc, fetch_push_doc_value, fetch_sts_for_auth, put_existing_push_message_doc,
    put_push_message_doc, save_and_push,
};
use crate::app::{
    clear_upload_transfer_metrics, hide_dashboard_window, open_external_url,
    set_upload_progress_label, update_upload_transfer_metrics,
};
use crate::dashboard::build_dashboard_snapshot;
use crate::device::{is_local_transfer_host, normalize_transfer_host_url};
use crate::models::{BucketConfig, OssSts, UploadAuthContext};
use crate::state::{
    finish_upload_task, set_dashboard_cache, try_begin_upload_task,
    update_dashboard_cache_after_delete, update_upload_runtime_state,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use reqwest::blocking::Client;
use reqwest::header::{AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE};
use serde_json::{json, Value};
use sha1::Sha1;
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri_plugin_dialog::{DialogExt, FilePath};
use uuid::Uuid;

const APP_ALERT_TITLE: &str = "Send2Boox Desktop";
const MAX_SINGLE_UPLOAD_BYTES: u64 = 200 * 1024 * 1024;
const MIN_STS_REMAINING_SECS_FOR_SIGNED_URL: i64 = 120;

fn show_alert(app: &tauri::AppHandle, message: impl Into<String>) {
    app.dialog()
        .message(message.into())
        .title(APP_ALERT_TITLE)
        .show(|_| {});
}

fn dialog_paths_to_std(paths: Vec<FilePath>) -> Vec<PathBuf> {
    paths
        .into_iter()
        .filter_map(|path| path.into_path().ok())
        .collect()
}

struct PreparedUploadFile {
    original_path: PathBuf,
    upload_path: PathBuf,
}

impl PreparedUploadFile {
    fn file_name(&self) -> Result<&str, String> {
        self.upload_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("{}: 文件名解析失败", self.upload_path.to_string_lossy()))
    }

    fn source_label(&self) -> String {
        self.original_path.to_string_lossy().to_string()
    }

    fn cleanup(&self) {}
}

fn prepare_upload_file(path: &Path) -> Result<PreparedUploadFile, String> {
    Ok(PreparedUploadFile {
        original_path: path.to_path_buf(),
        upload_path: path.to_path_buf(),
    })
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

fn validate_storage_quota_for_size(
    auth: &UploadAuthContext,
    uploaded_size: u64,
    next_size: u64,
) -> Result<(), String> {
    if let (Some(limit), Some(used)) = (auth.storage_limit, auth.storage_used) {
        let remaining = limit.saturating_sub(used);
        let planned = uploaded_size.saturating_add(next_size);
        if planned > remaining {
            return Err(format!(
                "可用空间不足，剩余 {} 字节，当前任务需 {} 字节",
                remaining, planned
            ));
        }
    }
    Ok(())
}

fn content_type_for(path: &Path) -> String {
    mime_guess::from_path(path)
        .first_or_octet_stream()
        .essence_str()
        .to_string()
}

fn build_oss_host(bucket: &BucketConfig) -> Result<(String, String), String> {
    let bucket_name = bucket
        .bucket
        .clone()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "bucket 缺少 bucket 字段".to_string())?;
    let endpoint = bucket
        .ali_endpoint
        .clone()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "bucket 缺少 aliEndpoint".to_string())?;
    let normalized = endpoint
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_matches('/');
    Ok((bucket_name.clone(), format!("{bucket_name}.{normalized}")))
}

fn sign_oss(secret: &str, string_to_sign: &str) -> Result<String, String> {
    let mut mac = Hmac::<Sha1>::new_from_slice(secret.as_bytes()).map_err(|err| err.to_string())?;
    mac.update(string_to_sign.as_bytes());
    Ok(BASE64_STANDARD.encode(mac.finalize().into_bytes()))
}

fn build_object_key(uid: &str, path: &Path) -> (String, String) {
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("bin")
        .to_ascii_lowercase();
    let object_key = format!("{uid}/push/{}.{}", Uuid::new_v4(), ext);
    (object_key, ext)
}

fn read_upload_bytes_with_progress<F>(
    file_path: &Path,
    mut on_progress: F,
) -> Result<Vec<u8>, String>
where
    F: FnMut(u64, u64, Duration),
{
    let mut file = File::open(file_path).map_err(|err| err.to_string())?;
    let file_size = file.metadata().map_err(|err| err.to_string())?.len();
    let mut bytes = Vec::with_capacity(file_size.min(8 * 1024 * 1024) as usize);
    let mut buffer = [0_u8; 256 * 1024];
    let mut sent = 0_u64;
    let mut last_emit = Instant::now();
    let started = Instant::now();

    loop {
        let read = file.read(&mut buffer).map_err(|err| err.to_string())?;
        if read == 0 {
            on_progress(sent, file_size, started.elapsed());
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
        sent = sent.saturating_add(read as u64).min(file_size);
        let now = Instant::now();
        if now.duration_since(last_emit) >= Duration::from_millis(120) || sent >= file_size {
            last_emit = now;
            on_progress(sent, file_size, now.duration_since(started));
        }
    }

    Ok(bytes)
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
    let file_size = fs::metadata(file_path)
        .map_err(|err| err.to_string())?
        .len();
    let body = read_upload_bytes_with_progress(file_path, on_progress)?;
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
        Ok(())
    } else {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        Err(format!("OSS 上传失败 ({status}): {body}"))
    }
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
    let response_disposition = "attachment";
    let mut subresources = vec![
        (
            "response-content-disposition",
            response_disposition.to_string(),
        ),
        ("security-token", sts.security_token.clone()),
    ];
    subresources.sort_by(|a, b| a.0.cmp(b.0));
    let canonical_subresources = subresources
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&");
    let canonical_resource = format!("/{bucket_name}/{object_key}?{canonical_subresources}");
    let string_to_sign = format!("GET\n\n\n{expires}\n{canonical_resource}");
    let signature = sign_oss(&sts.access_key_secret, &string_to_sign)?;
    let signed_query = [
        ("OSSAccessKeyId", sts.access_key_id.clone()),
        ("Expires", expires.to_string()),
        ("Signature", signature),
        (
            "response-content-disposition",
            response_disposition.to_string(),
        ),
        ("security-token", sts.security_token.clone()),
    ];
    let encoded_pairs = signed_query
        .into_iter()
        .map(|(key, value)| {
            format!(
                "{}={}",
                urlencoding::encode(key),
                urlencoding::encode(&value)
            )
        })
        .collect::<Vec<_>>();
    Ok(format!(
        "https://{host}/{object_key}?{}",
        encoded_pairs.join("&")
    ))
}

fn diagnostic_excerpt(text: &str, max_chars: usize) -> String {
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

fn verify_oss_object_via_signed_url(client: &Client, signed_url: &str) -> Result<(), String> {
    let response = client
        .get(signed_url)
        .header("Range", "bytes=0-0")
        .send()
        .map_err(|err| err.to_string())?;
    let status = response.status();
    if status.as_u16() == 200 || status.as_u16() == 206 {
        return Ok(());
    }
    let body = response.text().unwrap_or_default();
    Err(format!("HTTP {status}: {}", diagnostic_excerpt(&body, 220)))
}

fn sts_remaining_seconds(sts: &OssSts) -> Option<i64> {
    let expiration = DateTime::parse_from_rfc3339(&sts.expiration).ok()?;
    Some(expiration.timestamp() - Utc::now().timestamp())
}

fn build_verified_signed_download_url(
    client: &Client,
    auth: &UploadAuthContext,
    bucket: &BucketConfig,
    object_key: &str,
) -> Result<String, String> {
    let mut last_err = None;

    for attempt in 0..3 {
        let sts = fetch_sts_for_auth(client, auth)?;
        let remaining_secs = sts_remaining_seconds(&sts);

        if remaining_secs
            .map(|value| value <= MIN_STS_REMAINING_SECS_FOR_SIGNED_URL)
            .unwrap_or(false)
        {
            let err = format!(
                "STS 剩余有效期过短: {}s",
                remaining_secs.unwrap_or_default()
            );
            last_err = Some(err);
        } else {
            let signed_url = signed_download_url(bucket, &sts, object_key)?;
            match verify_oss_object_via_signed_url(client, &signed_url) {
                Ok(()) => return Ok(signed_url),
                Err(err) => last_err = Some(err),
            }
        }

        if attempt < 2 {
            thread::sleep(Duration::from_millis(400 * (attempt as u64 + 1)));
        }
    }

    Err(last_err.unwrap_or_else(|| "生成可用的签名下载链接失败".to_string()))
}

fn wait_for_remote_push_doc_ready(
    client: &Client,
    auth: &UploadAuthContext,
    doc_id: &str,
    expected_rev: &str,
) -> Result<(), String> {
    let mut last_err = None;
    for attempt in 0..8 {
        match fetch_push_doc(client, auth, doc_id) {
            Ok(detail) if detail.rev == expected_rev => return Ok(()),
            Ok(detail) => {
                last_err = Some(format!(
                    "推送消息 rev 尚未稳定，期望 {expected_rev}，实际 {}",
                    detail.rev
                ));
            }
            Err(err) => last_err = Some(err),
        }
        if attempt < 7 {
            thread::sleep(Duration::from_millis(250 * (attempt as u64 + 1)));
        }
    }
    Err(last_err.unwrap_or_else(|| "推送消息未能及时同步到 neocloud".to_string()))
}

#[cfg(test)]
fn update_push_doc_for_resend(doc: &mut Value, signed_url: &str) -> Result<(), String> {
    let now_ms = crate::util::unix_ms_now() as u64;
    update_push_doc_for_resend_at(doc, signed_url, now_ms)?;
    Ok(())
}

fn update_push_doc_for_resend_at(
    doc: &mut Value,
    signed_url: &str,
    now_ms: u64,
) -> Result<(), String> {
    let mut content = match doc.get("content") {
        Some(Value::String(text)) => serde_json::from_str::<Value>(text)
            .map_err(|err| format!("推送记录 content 解析失败: {err}"))?,
        Some(Value::Object(_)) => doc.get("content").cloned().unwrap_or(Value::Null),
        _ => return Err("推送记录缺少 content".to_string()),
    };
    let format = content
        .get("formats")
        .and_then(Value::as_array)
        .and_then(|formats| formats.first())
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "推送记录缺少 formats[0]".to_string())?
        .to_string();
    let storage = content
        .get_mut("storage")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "推送记录缺少 storage".to_string())?;
    let oss = storage
        .get_mut(&format)
        .and_then(Value::as_object_mut)
        .and_then(|bucket| bucket.get_mut("oss"))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "推送记录缺少 storage.<format>.oss".to_string())?;

    oss.insert("url".to_string(), Value::String(signed_url.to_string()));
    if let Some(content_obj) = content.as_object_mut() {
        content_obj.insert("updatedAt".to_string(), json!(now_ms));
    } else {
        return Err("推送记录 content 不是对象".to_string());
    }

    let doc_obj = doc
        .as_object_mut()
        .ok_or_else(|| "推送记录不是对象".to_string())?;
    doc_obj.insert("updatedAt".to_string(), json!(now_ms));
    doc_obj.insert("check".to_string(), Value::Bool(false));
    doc_obj.insert("content".to_string(), Value::String(content.to_string()));
    Ok(())
}

fn resend_doc_matches_expected(doc: &Value, expected_updated_at: u64) -> bool {
    let top_updated = doc
        .get("updatedAt")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let content_updated = match doc.get("content") {
        Some(Value::String(text)) => serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|content| content.get("updatedAt").and_then(Value::as_u64))
            .unwrap_or_default(),
        Some(Value::Object(content)) => content
            .get("updatedAt")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        _ => 0,
    };

    top_updated >= expected_updated_at && content_updated >= expected_updated_at
}

fn wait_for_remote_push_doc_resend_ready(
    client: &Client,
    auth: &UploadAuthContext,
    doc_id: &str,
    expected_updated_at: u64,
) -> Result<bool, String> {
    let mut last_err = None;
    for attempt in 0..8 {
        match fetch_push_doc_value(client, auth, doc_id) {
            Ok(doc) if resend_doc_matches_expected(&doc, expected_updated_at) => return Ok(true),
            Ok(doc) => {
                let top = doc
                    .get("updatedAt")
                    .and_then(Value::as_u64)
                    .unwrap_or_default();
                last_err = Some(format!(
                    "推送消息 updatedAt 尚未稳定，期望至少 {expected_updated_at}，实际 {top}"
                ));
            }
            Err(err) => last_err = Some(err),
        }
        if attempt < 7 {
            thread::sleep(Duration::from_millis(400 * (attempt as u64 + 1)));
        }
    }
    Err(last_err.unwrap_or_else(|| "推送消息未能及时同步到 neocloud".to_string()))
}

fn run_native_upload_task(app: tauri::AppHandle, files: Vec<PathBuf>) {
    thread::spawn(move || {
        let result = (|| -> Result<(usize, usize, Vec<String>, Vec<String>), String> {
            set_upload_progress_label(&app, "上传进度: 校验会话...");
            let client = create_client(600)?;
            let (auth, _) = fetch_auth_context_and_user(&app, &client)
                .map_err(|err| format!("当前会话未授权，请先点击“登录并授权”\n\n详情: {err}"))?;
            validate_storage_quota(&auth, &files)?;
            let total = files.len();
            let (bucket_key, bucket_cfg) = fetch_default_bucket(&client)?;
            let sts = fetch_sts_for_auth(&client, &auth)?;

            let mut success = 0usize;
            let mut failed = 0usize;
            let mut uploaded_size = 0u64;
            let mut errors = Vec::new();
            let warnings = Vec::new();

            for (index, file) in files.iter().enumerate() {
                let seq = index + 1;
                set_upload_progress_label(&app, &format!("上传进度: {seq}/{total} 准备中"));
                let prepared = match prepare_upload_file(file) {
                    Ok(value) => value,
                    Err(err) => {
                        failed += 1;
                        errors.push(err);
                        continue;
                    }
                };
                let metadata = match fs::metadata(&prepared.upload_path) {
                    Ok(value) => value,
                    Err(err) => {
                        failed += 1;
                        errors.push(format!(
                            "{}: 读取文件元信息失败: {}",
                            prepared.source_label(),
                            err
                        ));
                        prepared.cleanup();
                        continue;
                    }
                };
                if !metadata.is_file() {
                    failed += 1;
                    errors.push(format!("{}: 不是普通文件", prepared.source_label()));
                    prepared.cleanup();
                    continue;
                }
                if metadata.len() > MAX_SINGLE_UPLOAD_BYTES {
                    failed += 1;
                    errors.push(format!("{}: 文件超过 200MB 上限", prepared.source_label()));
                    prepared.cleanup();
                    continue;
                }
                if let Err(err) =
                    validate_storage_quota_for_size(&auth, uploaded_size, metadata.len())
                {
                    failed += 1;
                    errors.push(format!("{}: {}", prepared.source_label(), err));
                    prepared.cleanup();
                    continue;
                }
                let file_name = match prepared.file_name() {
                    Ok(value) => value.to_string(),
                    Err(err) => {
                        failed += 1;
                        errors.push(err);
                        prepared.cleanup();
                        continue;
                    }
                };
                let (object_key, resource_type) =
                    build_object_key(&auth.uid, &prepared.upload_path);
                let file_size = metadata.len();
                set_upload_progress_label(&app, &format!("上传进度: {seq}/{total} 上传中"));
                update_upload_runtime_state(&app, |state| {
                    state.current_file = Some(file_name.clone());
                    state.bytes_sent = Some(0);
                    state.bytes_total = Some(file_size);
                    state.progress_percent = Some(0.0);
                    state.speed_bps = Some(0.0);
                    state.eta_seconds = None;
                });
                let app_for_progress = app.clone();
                let file_name_for_progress = file_name.clone();
                if let Err(err) = upload_to_oss(
                    &client,
                    &bucket_cfg,
                    &sts,
                    &object_key,
                    &prepared.upload_path,
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
                    crate::diagnostics::warn(
                        "push.run_native_upload_task",
                        format!("file_name={file_name} stage=upload_to_oss err={err}"),
                    );
                    errors.push(format!("{file_name}: OSS 上传失败: {err}"));
                    prepared.cleanup();
                    continue;
                }
                uploaded_size = uploaded_size.saturating_add(file_size);

                let signed_url = match build_verified_signed_download_url(
                    &client,
                    &auth,
                    &bucket_cfg,
                    &object_key,
                ) {
                    Ok(value) => value,
                    Err(err) => {
                        failed += 1;
                        crate::diagnostics::warn(
                            "push.run_native_upload_task",
                            format!(
                                "file_name={file_name} stage=build_verified_signed_download_url err={err}"
                            ),
                        );
                        errors.push(format!("{file_name}: 下载签名生成失败: {err}"));
                        prepared.cleanup();
                        continue;
                    }
                };
                set_upload_progress_label(&app, &format!("上传进度: {seq}/{total} 写入推送队列"));
                let (cb_id, cb_rev) = match put_push_message_doc(
                    &client,
                    &auth,
                    &file_name,
                    file_size,
                    &resource_type,
                    &object_key,
                    &signed_url,
                ) {
                    Ok(value) => value,
                    Err(err) => {
                        failed += 1;
                        crate::diagnostics::warn(
                            "push.run_native_upload_task",
                            format!("file_name={file_name} stage=put_push_message_doc err={err}"),
                        );
                        errors.push(format!("{file_name}: 写入推送消息失败: {err}"));
                        prepared.cleanup();
                        continue;
                    }
                };
                if let Err(err) = wait_for_remote_push_doc_ready(&client, &auth, &cb_id, &cb_rev) {
                    failed += 1;
                    crate::diagnostics::warn(
                        "push.run_native_upload_task",
                        format!(
                            "file_name={file_name} stage=wait_for_remote_push_doc_ready doc_id={cb_id} err={err}"
                        ),
                    );
                    errors.push(format!("{file_name}: 推送消息同步确认失败: {err}"));
                    prepared.cleanup();
                    continue;
                }
                set_upload_progress_label(&app, &format!("上传进度: {seq}/{total} 推送确认中"));
                if let Err(err) = save_and_push(
                    &client,
                    &auth,
                    &bucket_key,
                    &object_key,
                    &file_name,
                    &resource_type,
                    &cb_id,
                    &cb_rev,
                ) {
                    failed += 1;
                    crate::diagnostics::warn(
                        "push.run_native_upload_task",
                        format!(
                            "file_name={file_name} stage=save_and_push doc_id={cb_id} err={err}"
                        ),
                    );
                    errors.push(format!("{file_name}: saveAndPush 失败: {err}"));
                    prepared.cleanup();
                    continue;
                }
                success += 1;
                set_upload_progress_label(&app, &format!("上传进度: {seq}/{total} 完成"));
                prepared.cleanup();
            }

            Ok((success, failed, errors, warnings))
        })();

        finish_upload_task(&app);
        match result {
            Ok((0, failed, errors, _)) if failed > 0 => {
                set_upload_progress_label(&app, "上传进度: 全部失败");
                clear_upload_transfer_metrics(&app);
                let details = errors
                    .iter()
                    .take(2)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n");
                let details_for_state = details.clone();
                update_upload_runtime_state(&app, move |state| {
                    state.last_error = Some(details_for_state.clone());
                });
                show_alert(
                    &app,
                    format!("上传失败：所有文件均未成功上传。\n\n{details}"),
                );
            }
            Ok((success, failed, errors, warnings)) => {
                if failed > 0 {
                    set_upload_progress_label(
                        &app,
                        &format!("上传进度: 成功{success} 失败{failed}"),
                    );
                    let details = errors
                        .iter()
                        .chain(warnings.iter())
                        .take(2)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("\n");
                    let details_for_state = details.clone();
                    update_upload_runtime_state(&app, move |state| {
                        state.last_error = Some(details_for_state.clone());
                    });
                    show_alert(
                        &app,
                        format!("上传完成：成功 {success}，失败 {failed}\n\n处理提示:\n{details}"),
                    );
                } else {
                    set_upload_progress_label(&app, "上传进度: 全部完成");
                    update_upload_runtime_state(&app, |state| {
                        state.last_error = None;
                    });
                }
                let snapshot = build_dashboard_snapshot(&app);
                set_dashboard_cache(&app, snapshot.clone());
                crate::app::sync_reading_metrics_menu_from_snapshot(&app, &snapshot);
            }
            Err(err) => {
                set_upload_progress_label(&app, "上传进度: 失败");
                clear_upload_transfer_metrics(&app);
                let err_for_state = err.clone();
                update_upload_runtime_state(&app, move |state| {
                    state.last_error = Some(err_for_state.clone());
                });
                show_alert(&app, format!("上传失败：{err}"));
            }
        }
    });
}

pub fn trigger_upload_from_tray(app: &tauri::AppHandle) {
    if !try_begin_upload_task(app) {
        show_alert(app, "已有上传任务在执行，请稍后重试。");
        return;
    }

    set_upload_progress_label(app, "上传进度: 等待选择文件...");
    hide_dashboard_window(app);
    let app_handle = app.clone();
    let run_result = app.run_on_main_thread(move || {
        app_handle.dialog().file().pick_files(move |files| {
            let app_for_callback = app_handle.clone();
            let Some(paths) = files else {
                set_upload_progress_label(&app_for_callback, "上传进度: 已取消");
                clear_upload_transfer_metrics(&app_for_callback);
                finish_upload_task(&app_for_callback);
                return;
            };
            let paths = dialog_paths_to_std(paths);
            if paths.is_empty() {
                set_upload_progress_label(&app_for_callback, "上传进度: 已取消");
                clear_upload_transfer_metrics(&app_for_callback);
                finish_upload_task(&app_for_callback);
                return;
            }
            run_native_upload_task(app_for_callback, paths);
        });
    });

    if let Err(err) = run_result {
        finish_upload_task(app);
        set_upload_progress_label(app, "上传进度: 失败");
        clear_upload_transfer_metrics(app);
        show_alert(app, format!("无法打开文件选择窗口：{err}"));
    }
}

fn dashboard_push_resend_inner(app: &tauri::AppHandle, doc_id: &str) -> Result<(), String> {
    if doc_id.trim().is_empty() {
        return Err("推送记录 id 不能为空".to_string());
    }
    let client = create_client(90)?;
    let (auth, _) = fetch_auth_context_and_user(app, &client)?;
    let detail = fetch_push_doc(&client, &auth, doc_id)?;
    let (_, bucket_cfg) = fetch_default_bucket(&client)?;
    let signed_url =
        build_verified_signed_download_url(&client, &auth, &bucket_cfg, &detail.resource_key)?;
    let mut last_err = None;

    for attempt in 0..3 {
        let mut raw_doc = fetch_push_doc_value(&client, &auth, doc_id)?;
        let expected_updated_at = crate::util::unix_ms_now() as u64;
        update_push_doc_for_resend_at(&mut raw_doc, &signed_url, expected_updated_at)?;

        match put_existing_push_message_doc(&client, &auth, &raw_doc) {
            Ok(_) => {
                let _ = wait_for_remote_push_doc_resend_ready(
                    &client,
                    &auth,
                    doc_id,
                    expected_updated_at,
                );
                return Ok(());
            }
            Err(err) => {
                let lowered = err.to_ascii_lowercase();
                let retryable = lowered.contains("timeout")
                    || lowered.contains("gateway")
                    || lowered.contains("504")
                    || lowered.contains("503")
                    || lowered.contains("502")
                    || lowered.contains("temporarily unavailable")
                    || lowered.contains("connection reset");
                crate::diagnostics::warn(
                    "push.dashboard_push_resend_inner",
                    format!(
                        "doc_id={doc_id} attempt={} retryable={} err={err}",
                        attempt + 1,
                        retryable
                    ),
                );
                if retryable {
                    if wait_for_remote_push_doc_resend_ready(
                        &client,
                        &auth,
                        doc_id,
                        expected_updated_at,
                    )
                    .unwrap_or(false)
                    {
                        return Ok(());
                    }
                }
                if !retryable || attempt == 2 {
                    crate::diagnostics::error(
                        "push.dashboard_push_resend_inner",
                        format!("fail doc_id={doc_id} attempt={} err={err}", attempt + 1),
                    );
                    return Err(err);
                }
                last_err = Some(err);
                thread::sleep(Duration::from_millis(1_500 * (attempt + 1) as u64));
            }
        }
    }

    Err(last_err.unwrap_or_else(|| "重推失败".to_string()))
}

fn dashboard_push_delete_inner(app: &tauri::AppHandle, doc_id: &str) -> Result<(), String> {
    if doc_id.trim().is_empty() {
        return Err("删除记录 id 不能为空".to_string());
    }
    let client = create_client(60)?;
    let (auth, _) = fetch_auth_context_and_user(app, &client)?;
    let _ = crate::api::delete_push_doc(&client, &auth, doc_id);
    batch_delete_push_message(&client, &auth, doc_id)?;
    Ok(())
}

#[tauri::command]
pub fn dashboard_upload_pick_and_send(app: tauri::AppHandle) -> Result<(), String> {
    trigger_upload_from_tray(&app);
    Ok(())
}

#[tauri::command]
pub async fn dashboard_push_resend(
    app: tauri::AppHandle,
    id: String,
) -> Result<crate::models::DashboardSnapshot, String> {
    let app_for_task = app.clone();
    let snapshot = tauri::async_runtime::spawn_blocking(move || {
        dashboard_push_resend_inner(&app_for_task, &id)?;
        Ok::<crate::models::DashboardSnapshot, String>(build_dashboard_snapshot(&app_for_task))
    })
    .await
    .map_err(|err| err.to_string())??;
    set_dashboard_cache(&app, snapshot.clone());
    crate::app::sync_reading_metrics_menu_from_snapshot(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
pub async fn dashboard_push_delete(
    app: tauri::AppHandle,
    id: String,
) -> Result<crate::models::DashboardSnapshot, String> {
    let app_for_task = app.clone();
    let id_for_task = id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        dashboard_push_delete_inner(&app_for_task, &id_for_task)
    })
    .await
    .map_err(|err| err.to_string())??;

    if let Some(snapshot) = update_dashboard_cache_after_delete(&app, &id) {
        crate::app::sync_reading_metrics_menu_from_snapshot(&app, &snapshot);
        return Ok(snapshot);
    }
    let app_for_refresh = app.clone();
    let snapshot =
        tauri::async_runtime::spawn_blocking(move || build_dashboard_snapshot(&app_for_refresh))
            .await
            .map_err(|err| err.to_string())?;
    set_dashboard_cache(&app, snapshot.clone());
    crate::app::sync_reading_metrics_menu_from_snapshot(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
pub fn dashboard_open_transfer_host(host: String) -> Result<(), String> {
    let normalized =
        normalize_transfer_host_url(&host).ok_or_else(|| "设备互传地址无效".to_string())?;
    let parsed = tauri::Url::parse(&normalized).map_err(|err| err.to_string())?;
    let host_name = parsed
        .host_str()
        .ok_or_else(|| "设备互传地址缺少主机名".to_string())?;
    if !is_local_transfer_host(host_name) {
        return Err("仅允许打开局域网 BOOX 设备地址".to_string());
    }
    open_external_url(&normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn build_object_key_matches_web_upload_prefix() {
        let (object_key, ext) = build_object_key("user-123", Path::new("/tmp/demo.azw3"));
        assert_eq!(ext, "azw3");
        assert!(object_key.starts_with("user-123/push/"));
        assert!(object_key.ends_with(".azw3"));
        assert!(!object_key.contains("/demo.azw3"));
    }

    #[test]
    fn build_object_key_falls_back_to_bin_extension() {
        let (object_key, ext) = build_object_key("user-123", Path::new("/tmp/upload"));
        assert_eq!(ext, "bin");
        assert!(object_key.starts_with("user-123/push/"));
        assert!(object_key.ends_with(".bin"));
    }

    #[test]
    fn update_push_doc_for_resend_refreshes_top_level_and_nested_fields() {
        let mut doc = json!({
            "_id": "doc-1",
            "_rev": "1-a",
            "contentType": "digital_content",
            "updatedAt": 1,
            "content": "{\"formats\":[\"azw3\"],\"storage\":{\"azw3\":{\"oss\":{\"url\":\"old\"}}},\"updatedAt\":1}"
        });

        update_push_doc_for_resend(&mut doc, "https://example.com/new").expect("updated");

        assert_eq!(doc.get("check").and_then(Value::as_bool), Some(false));
        assert_eq!(doc.get("content").and_then(Value::as_str).is_some(), true);
        let content: Value =
            serde_json::from_str(doc.get("content").and_then(Value::as_str).unwrap()).unwrap();
        assert_eq!(
            content["storage"]["azw3"]["oss"]["url"].as_str(),
            Some("https://example.com/new")
        );
        assert!(doc["updatedAt"].as_u64().unwrap() >= 1);
        assert!(content["updatedAt"].as_u64().unwrap() >= 1);
    }

    #[test]
    fn signed_download_url_includes_security_token_in_signature_and_query() {
        let bucket = BucketConfig {
            ali_endpoint: Some("oss-cn-shenzhen.aliyuncs.com".to_string()),
            bucket: Some("onyx-cloud".to_string()),
            region: Some("oss-cn-shenzhen".to_string()),
        };
        let sts = OssSts {
            access_key_id: "test-id".to_string(),
            access_key_secret: "test-secret".to_string(),
            security_token: "test-token".to_string(),
            expiration: "2099-01-01T00:00:00Z".to_string(),
        };

        let url = signed_download_url(&bucket, &sts, "uid/push/demo.epub").expect("signed url");

        assert!(url.contains("OSSAccessKeyId=test-id"));
        assert!(url.contains("response-content-disposition=attachment"));
        assert!(url.contains("security-token=test-token"));
        assert!(url.contains("Signature="));
        assert!(url.contains("response-content-disposition=attachment&security-token=test-token"));
    }
}

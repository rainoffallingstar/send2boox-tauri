function createIpcInvoke() {
  return (cmd, args = {}) =>
    new Promise((resolve, reject) => {
      const cb = `_${Math.random().toString(36).slice(2)}`;
      const err = `_${Math.random().toString(36).slice(2)}`;
      Object.defineProperty(window, cb, {
        value: (result) => {
          try {
            resolve(result);
          } finally {
            Reflect.deleteProperty(window, cb);
            Reflect.deleteProperty(window, err);
          }
        },
        configurable: true
      });
      Object.defineProperty(window, err, {
        value: (error) => {
          try {
            reject(error);
          } finally {
            Reflect.deleteProperty(window, cb);
            Reflect.deleteProperty(window, err);
          }
        },
        configurable: true
      });
      window.__TAURI_IPC__({
        cmd,
        callback: cb,
        error: err,
        payload: args
      });
    });
}

function resolveInvoke() {
  if (window.__TAURI__?.core?.invoke) {
    return window.__TAURI__.core.invoke;
  }
  if (window.__TAURI__?.invoke) {
    return window.__TAURI__.invoke;
  }
  if (window.__TAURI__?.tauri?.invoke) {
    return window.__TAURI__.tauri.invoke;
  }
  if (typeof window.__TAURI_INVOKE__ === "function") {
    return window.__TAURI_INVOKE__;
  }
  if (typeof window.__TAURI_INTERNALS__?.invoke === "function") {
    return window.__TAURI_INTERNALS__.invoke;
  }
  if (typeof window.__TAURI_IPC__ === "function") {
    return createIpcInvoke();
  }
  return null;
}

const state = {
  timer: null,
  syncTimer: null,
  snapshot: null,
  loading: false,
  pendingForce: false,
  authTimer: null,
  authTimerTicks: 0,
  refreshMs: 60000,
  uploadStatusOverride: "",
  uploadStatusOverrideUntil: 0
};

function invokeWithTimeout(command, args = {}, timeoutMs = 12000) {
  const invoke = resolveInvoke();
  if (!invoke) {
    return Promise.reject(new Error("当前环境不支持 Tauri invoke"));
  }
  return Promise.race([
    invoke(command, args),
    new Promise((_, reject) => {
      setTimeout(() => reject(new Error(`命令超时: ${command}`)), timeoutMs);
    })
  ]);
}

function bytesToText(value) {
  const n = Number(value || 0);
  if (!Number.isFinite(n) || n <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let idx = 0;
  let cur = n;
  while (cur >= 1024 && idx < units.length - 1) {
    cur /= 1024;
    idx += 1;
  }
  return `${cur.toFixed(idx > 1 ? 2 : 0)} ${units[idx]}`;
}

function durationText(ms) {
  const total = Number(ms || 0);
  if (!Number.isFinite(total) || total <= 0) return "-";
  const seconds = Math.floor(total / 1000);
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

function etaText(seconds) {
  const s = Number(seconds);
  if (!Number.isFinite(s) || s < 0) return "-";
  const total = Math.floor(s);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const sec = total % 60;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m ${sec}s`;
  return `${sec}s`;
}

function speedText(speedBps) {
  const n = Number(speedBps);
  if (!Number.isFinite(n) || n <= 0) return "-/s";
  return `${bytesToText(n)}/s`;
}

function numberText(v) {
  const n = Number(v);
  if (!Number.isFinite(n)) return "-";
  return String(n);
}

function toDateText(ts) {
  const n = Number(ts || 0);
  if (!Number.isFinite(n) || n <= 0) return "-";
  const d = new Date(n);
  return d.toLocaleString();
}

function timeAgoText(ts) {
  const n = Number(ts || 0);
  if (!Number.isFinite(n) || n <= 0) return "刚刚";
  const now = Date.now();
  const diff = Math.max(0, Math.floor((now - n) / 1000));
  if (diff < 5) return "刚刚";
  if (diff < 60) return `${diff}s 前`;
  if (diff < 3600) return `${Math.floor(diff / 60)}m 前`;
  return `${Math.floor(diff / 3600)}h 前`;
}

function safeNum(v) {
  const n = Number(v);
  return Number.isFinite(n) ? n : 0;
}

function computeTodayReadCount(dayReadToday) {
  if (Array.isArray(dayReadToday)) return dayReadToday.length;
  if (Array.isArray(dayReadToday?.list)) return dayReadToday.list.length;
  if (Array.isArray(dayReadToday?.rows)) return dayReadToday.rows.length;
  return safeNum(dayReadToday?.read || dayReadToday?.count || dayReadToday?.total || 0);
}

function computeWeekTotalTime(readTimeWeek) {
  return (
    readTimeWeek?.now?.totalTime ??
    readTimeWeek?.totalTime ??
    readTimeWeek?.weekTotalTime ??
    0
  );
}

function setText(id, value) {
  const el = document.getElementById(id);
  if (el) el.textContent = value;
}

function setUploadStatusOverride(value, holdMs = 0) {
  state.uploadStatusOverride = value || "";
  state.uploadStatusOverrideUntil = holdMs > 0 ? Date.now() + holdMs : 0;
  if (value) {
    setText("upload-status", value);
  }
}

function clearUploadStatusOverride() {
  state.uploadStatusOverride = "";
  state.uploadStatusOverrideUntil = 0;
}

function getVisibleUploadStatus(snapshotStatus) {
  const now = Date.now();
  const overrideActive =
    !!state.uploadStatusOverride && now < Number(state.uploadStatusOverrideUntil || 0);
  if (!overrideActive) {
    clearUploadStatusOverride();
    return snapshotStatus;
  }
  if (snapshotStatus === "上传进度: 空闲" || snapshotStatus === "上传进度: 等待选择文件...") {
    return state.uploadStatusOverride;
  }
  clearUploadStatusOverride();
  return snapshotStatus;
}

function scheduleSnapshotRefresh(delays, force = true) {
  (Array.isArray(delays) ? delays : []).forEach((delay) => {
    const ms = Number(delay);
    if (!Number.isFinite(ms) || ms < 0) return;
    setTimeout(() => loadSnapshot(force), ms);
  });
}

async function refreshCard(cardEl) {
  if (!cardEl) {
    await loadSnapshot(true);
    return;
  }
  cardEl.classList.add("is-refreshing");
  try {
    await loadSnapshot(true);
  } finally {
    setTimeout(() => cardEl.classList.remove("is-refreshing"), 200);
  }
}

function normalizeRefreshMinutes(value) {
  const n = Number(value);
  if (!Number.isFinite(n) || n <= 0) return 1;
  return Math.min(1440, n);
}

function formatRefreshMinutes(minutes) {
  const rounded = Math.round(minutes * 10) / 10;
  if (Number.isInteger(rounded)) return String(rounded);
  return String(rounded.toFixed(1));
}

function applyRefreshIntervalMinutes(minutesInput) {
  const minutes = normalizeRefreshMinutes(minutesInput);
  state.refreshMs = Math.max(1000, Math.round(minutes * 60 * 1000));
  localStorage.setItem("s2b_refresh_interval_minutes", formatRefreshMinutes(minutes));
  const input = document.getElementById("refresh-interval-minutes");
  if (input) {
    input.value = formatRefreshMinutes(minutes);
  }
  startAutoRefresh();
}

function getInitialRefreshMinutes() {
  const minutes = localStorage.getItem("s2b_refresh_interval_minutes");
  if (minutes != null) return normalizeRefreshMinutes(minutes);
  const legacyMs = Number(localStorage.getItem("s2b_refresh_interval_ms"));
  if (Number.isFinite(legacyMs) && legacyMs > 0) {
    return normalizeRefreshMinutes(legacyMs / 60000);
  }
  return 1;
}

function removePushItemLocally(id) {
  if (!state.snapshot?.push_queue) return;
  state.snapshot.push_queue = state.snapshot.push_queue.filter((item) => item.id !== id);
  renderPushList(state.snapshot.push_queue);
}

function renderLanDeviceButtons(devices) {
  const host = document.getElementById("lan-device-list");
  if (!host) return;
  host.innerHTML = "";

  const sameLanDevices = (devices || []).filter((item) => !!item?.same_lan);
  if (sameLanDevices.length === 0) {
    const empty = document.createElement("button");
    empty.type = "button";
    empty.className = "lan-device-btn empty";
    empty.disabled = true;
    empty.textContent = "未发现同局域网 BOOX 设备";
    host.appendChild(empty);
    return;
  }

  sameLanDevices.forEach((item) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "lan-device-btn";
    const model = item.model || "BOOX 设备";
    const transferHost = item.transfer_host || "";
    const ip = item.lan_ip || item.ip_address || "";
    const inferred =
      item.same_lan_reason === "single_device_online_fallback" ||
      item.same_lan_reason === "share_socket_single_fallback";
    const hostText = transferHost ? transferHost.replace(/^https?:\/\//, "") : ip;
    const titleText = hostText ? `${model} · ${hostText}` : model;
    button.textContent = inferred ? `${titleText} · 推断` : titleText;
    button.dataset.model = model;
    button.dataset.deviceId = item.id || "";
    button.dataset.mac = item.mac_address || "";
    button.dataset.ip = ip;
    button.dataset.host = transferHost;
    button.dataset.status = item.login_status || "";
    button.dataset.reason = item.same_lan_reason || "";
    host.appendChild(button);
  });
}

function renderPushList(items) {
  const list = document.getElementById("push-list");
  if (!list) return;
  list.innerHTML = "";
  if (!items || items.length === 0) {
    const li = document.createElement("li");
    li.className = "push-item";
    li.innerHTML = `<p class="push-item-name">暂无互动文件</p>`;
    list.appendChild(li);
    return;
  }
  items.forEach((item) => {
    const li = document.createElement("li");
    li.className = "push-item";
    li.dataset.pushId = item.id || "";
    const meta = `${bytesToText(item.size)} · ${toDateText(item.updated_at)}`;
    li.innerHTML = `
      <p class="push-item-name">${item.name || "(未命名文件)"}</p>
      <p class="push-item-meta">${meta}</p>
      <div class="push-item-actions">
        <button class="push-action" data-action="resend">推送</button>
        <button class="push-action danger" data-action="delete">删除</button>
      </div>
    `;
    list.appendChild(li);
  });
}

function renderSnapshot(snapshot) {
  state.snapshot = snapshot;
  const auth = snapshot?.auth || {};
  const profile = snapshot?.profile || {};
  const upload = snapshot?.upload || {};
  const storage = snapshot?.storage || {};
  const devices = snapshot?.devices || [];
  const readingInfo = snapshot?.calendar_metrics?.reading_info || {};
  const readTimeWeek = snapshot?.calendar_metrics?.read_time_week || {};
  const dayReadToday = snapshot?.calendar_metrics?.day_read_today || {};

  const authEl = document.getElementById("auth-status");
  if (authEl) {
    authEl.textContent = auth.authorized
      ? `已授权 · ${auth.source || "unknown"}`
      : `未授权 · ${auth.message || "请先登录"}`;
    authEl.className = auth.authorized ? "muted state-success" : "muted state-danger";
  }
  const authChip = document.getElementById("auth-chip");
  if (authChip) {
    authChip.textContent = auth.authorized ? "已授权" : "待授权";
    authChip.className = auth.authorized ? "chip chip-success" : "chip chip-danger";
  }
  setText("sync-time", `更新于 ${timeAgoText(snapshot?.fetched_at_ms)}`);

  setText("profile-uid", profile.uid ? `uid: ${profile.uid}` : "");
  setText("profile-name", profile.nickname || "未获取到用户名");
  const sameLanCount = devices.filter((item) => !!item?.same_lan).length;
  setText(
    "device-summary",
    devices.length
      ? `设备数: ${devices.length} · 同局域网: ${sameLanCount}`
      : "未获取到设备信息"
  );
  renderLanDeviceButtons(devices);

  const used = storage.used;
  const limit = storage.limit;
  const storagePercent = Number(storage.percent || 0);
  setText(
    "storage-text",
    used != null || limit != null
      ? `${bytesToText(used)} / ${bytesToText(limit)}`
      : "未知"
  );
  const bar = document.getElementById("storage-bar");
  if (bar) {
    bar.style.width = `${Math.max(0, Math.min(100, storagePercent)).toFixed(2)}%`;
  }

  setText("metric-today-read", numberText(computeTodayReadCount(dayReadToday)));
  setText("metric-week-time", durationText(computeWeekTotalTime(readTimeWeek)));
  setText("metric-total-read", numberText(readingInfo.read));
  setText("metric-total-finished", numberText(readingInfo.finished));

  const uploadText = getVisibleUploadStatus(upload.status_text || "上传进度: 空闲");
  setText("upload-status", uploadText);
  const upPercent = Number(upload.progress_percent);
  const uploadPercent = Number.isFinite(upPercent) ? Math.max(0, Math.min(100, upPercent)) : 0;
  const uploadBar = document.getElementById("upload-progress-bar");
  if (uploadBar) {
    uploadBar.style.width = `${uploadPercent.toFixed(1)}%`;
  }
  const bytesSent = upload.bytes_sent;
  const bytesTotal = upload.bytes_total;
  const metricsText = `${uploadPercent.toFixed(1)}% · ${speedText(upload.speed_bps)} · 剩余 ${etaText(upload.eta_seconds)}${
    bytesTotal ? ` · ${bytesToText(bytesSent || 0)}/${bytesToText(bytesTotal)}` : ""
  }`;
  setText("upload-metrics", metricsText);
  renderPushList(snapshot?.push_queue || []);

  const loginBtn = document.getElementById("login-btn");
  if (loginBtn) {
    loginBtn.disabled = !!auth.authorized;
    loginBtn.textContent = auth.authorized ? "已授权" : "浏览器登录";
  }

  if (auth.authorized && state.authTimer) {
    clearInterval(state.authTimer);
    state.authTimer = null;
    state.authTimerTicks = 0;
  }
}

async function loadSnapshot(force = false) {
  const invoke = resolveInvoke();
  if (state.loading) {
    state.pendingForce = state.pendingForce || force;
    return;
  }
  if (!invoke) {
    setText("auth-status", "当前环境不支持 Tauri invoke");
    return;
  }
  state.loading = true;
  try {
    const command = force ? "dashboard_refresh" : "dashboard_snapshot";
    const snapshot = await invokeWithTimeout(command);
    renderSnapshot(snapshot);
  } catch (err) {
    setText("auth-status", `加载失败: ${String(err)}`);
  } finally {
    state.loading = false;
    if (state.pendingForce) {
      state.pendingForce = false;
      setTimeout(() => loadSnapshot(true), 50);
    }
  }
}

function bindActions() {
  document.querySelectorAll("[data-card-refresh]").forEach((btn) => {
    btn.addEventListener("click", async (event) => {
      const target = event.currentTarget;
      if (!(target instanceof HTMLElement)) return;
      const card = target.closest(".card");
      target.setAttribute("disabled", "true");
      try {
        await refreshCard(card);
      } finally {
        target.removeAttribute("disabled");
      }
    });
  });

  document.getElementById("refresh-btn")?.addEventListener("click", async () => {
    await loadSnapshot(true);
  });

  document.getElementById("refresh-apply-btn")?.addEventListener("click", () => {
    const input = document.getElementById("refresh-interval-minutes");
    if (!(input instanceof HTMLInputElement)) return;
    applyRefreshIntervalMinutes(input.value);
  });

  document
    .getElementById("refresh-interval-minutes")
    ?.addEventListener("keydown", (event) => {
      if (!(event instanceof KeyboardEvent) || event.key !== "Enter") return;
      const input = event.target;
      if (!(input instanceof HTMLInputElement)) return;
      applyRefreshIntervalMinutes(input.value);
    });

  document.getElementById("refresh-interval-minutes")?.addEventListener("blur", (event) => {
    const target = event.target;
    if (!(target instanceof HTMLInputElement)) return;
    applyRefreshIntervalMinutes(target.value);
  });

  document.getElementById("login-btn")?.addEventListener("click", async () => {
    setText("auth-status", "默认浏览器已打开登录方式选择页，请选择微信扫码或 BOOX 助手扫码完成授权");
    await invokeWithTimeout("dashboard_login_authorize", {}, 6000);
    if (state.authTimer) {
      clearInterval(state.authTimer);
    }
    state.authTimerTicks = 0;
    state.authTimer = setInterval(() => {
      state.authTimerTicks += 1;
      loadSnapshot(true);
      if (state.authTimerTicks >= 90 && state.authTimer) {
        clearInterval(state.authTimer);
        state.authTimer = null;
      }
    }, 1200);
  });

  document.getElementById("upload-btn")?.addEventListener("click", async (event) => {
    const target = event.currentTarget;
    if (!(target instanceof HTMLButtonElement)) return;
    if (target.disabled) return;

    const originalText = target.textContent || "上传文件";
    target.disabled = true;
    target.textContent = "打开中...";
    setUploadStatusOverride("上传进度: 正在打开文件选择窗口，请检查系统弹窗", 12000);

    try {
      await invokeWithTimeout("dashboard_upload_pick_and_send", {}, 10000);
      setUploadStatusOverride(
        "上传进度: 文件选择窗口已唤起，请在系统弹窗中选择文件；若为 mobi/azw3 将自动转换为 EPUB",
        16000
      );
      scheduleSnapshotRefresh([250, 900, 1800, 3600, 7000, 12000, 18000], true);
    } catch (err) {
      clearUploadStatusOverride();
      setText("upload-status", `上传入口失败: ${String(err)}`);
      scheduleSnapshotRefresh([300], true);
    } finally {
      setTimeout(() => {
        target.disabled = false;
        target.textContent = originalText;
      }, 1200);
    }
  });

  document.getElementById("push-list")?.addEventListener("click", async (event) => {
    const target = event.target?.closest("button[data-action]");
    if (!(target instanceof HTMLButtonElement)) return;
    if (target.disabled) return;
    const action = target.dataset.action;
    if (!action) return;
    const row = target.closest(".push-item");
    if (!(row instanceof HTMLElement)) return;
    const id = row.dataset.pushId;
    if (!id) return;

    const originalText = target.textContent || (action === "resend" ? "推送" : "删除");
    target.disabled = true;
    target.textContent = action === "resend" ? "推送中..." : "删除中...";
    try {
      if (action === "resend") {
        setUploadStatusOverride("上传进度: 正在重新推送文件到设备，请稍候...", 12000);
        const snapshot = await invokeWithTimeout("dashboard_push_resend", { id }, 20000);
        if (snapshot) renderSnapshot(snapshot);
        setUploadStatusOverride("上传进度: 已提交重新推送，正在刷新状态...", 6000);
        scheduleSnapshotRefresh([500, 1800, 4000], true);
      } else if (action === "delete") {
        const ok = window.confirm("确定删除这条推送记录吗？");
        if (!ok) return;
        removePushItemLocally(id);
        setUploadStatusOverride("上传进度: 正在删除推送记录...", 6000);
        const snapshot = await invokeWithTimeout("dashboard_push_delete", { id }, 12000);
        if (snapshot) renderSnapshot(snapshot);
        setUploadStatusOverride("上传进度: 推送记录已删除", 3000);
      }
    } catch (err) {
      clearUploadStatusOverride();
      setText("upload-status", `操作失败: ${String(err)}`);
      if (action === "delete") {
        setTimeout(() => loadSnapshot(true), 120);
      }
    } finally {
      target.disabled = false;
      target.textContent = originalText;
      if (action === "resend") {
        setTimeout(() => loadSnapshot(false), 400);
      }
    }
  });

  document.getElementById("lan-device-list")?.addEventListener("click", async (event) => {
    const invoke = resolveInvoke();
    const target = event.target;
    if (!(target instanceof HTMLButtonElement)) return;
    if (target.disabled) return;
    const model = target.dataset.model || "BOOX 设备";
    const status = target.dataset.status || "-";
    const ip = target.dataset.ip || "-";
    const host = target.dataset.host || "-";
    const mac = target.dataset.mac || "-";
    const deviceId = target.dataset.deviceId || "-";
    const reasonRaw = target.dataset.reason || "-";
    const reason =
      reasonRaw === "share_socket_mac"
        ? "Share WebSocket(MAC匹配)"
        : reasonRaw === "share_socket_single_fallback"
          ? "Share WebSocket(单设备回退)"
          : reasonRaw === "single_device_online_fallback"
        ? "单设备在线推断"
        : reasonRaw === "mac_arp"
          ? "MAC-ARP命中"
          : reasonRaw === "same_subnet"
            ? "同网段IP命中"
            : reasonRaw;
    if (host !== "-" && invoke) {
      try {
        await invokeWithTimeout("dashboard_open_transfer_host", { host }, 6000);
        return;
      } catch (err) {
        setText("upload-status", `打开设备地址失败: ${String(err)}`);
      }
    }
    window.alert(
      `设备: ${model}\n状态: ${status}\n互传地址: ${host}\n局域网IP: ${ip}\nMAC: ${mac}\n设备ID: ${deviceId}\n识别来源: ${reason}`
    );
  });
}

function startAutoRefresh() {
  if (state.timer) clearInterval(state.timer);
  state.timer = setInterval(() => {
    loadSnapshot(false);
  }, state.refreshMs);
  if (state.syncTimer) clearInterval(state.syncTimer);
  state.syncTimer = setInterval(() => {
    if (state.snapshot?.fetched_at_ms) {
      setText("sync-time", `更新于 ${timeAgoText(state.snapshot.fetched_at_ms)}`);
    }
  }, 1000);
}

document.addEventListener("DOMContentLoaded", async () => {
  applyRefreshIntervalMinutes(getInitialRefreshMinutes());
  bindActions();
  setTimeout(() => loadSnapshot(true), 120);
  window.addEventListener("focus", () => loadSnapshot(true));
  window.addEventListener("beforeunload", () => {
    if (state.timer) {
      clearInterval(state.timer);
      state.timer = null;
    }
    if (state.syncTimer) {
      clearInterval(state.syncTimer);
      state.syncTimer = null;
    }
  });
});

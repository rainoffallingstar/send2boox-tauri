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
  if (window.__TAURI__?.core?.invoke) return window.__TAURI__.core.invoke;
  if (window.__TAURI__?.invoke) return window.__TAURI__.invoke;
  if (window.__TAURI__?.tauri?.invoke) return window.__TAURI__.tauri.invoke;
  if (typeof window.__TAURI_INVOKE__ === "function") return window.__TAURI_INVOKE__;
  if (typeof window.__TAURI_INTERNALS__?.invoke === "function") return window.__TAURI_INTERNALS__.invoke;
  if (typeof window.__TAURI_IPC__ === "function") return createIpcInvoke();
  return null;
}

const VIEW_META = {
  overview: {
    kicker: "概览",
    title: "控制中心",
    subtitle: "统一查看授权状态、上传进度、设备摘要与阅读摘要。",
    badge: "Workspace"
  },
  push: {
    kicker: "互动文件",
    title: "推送与上传",
    subtitle: "管理互动文件、查看上传状态，并直接重推到设备。",
    badge: "Queue"
  },
  devices: {
    kicker: "设备",
    title: "设备与互传",
    subtitle: "查看在线设备、同局域网设备，并打开互传地址。",
    badge: "Devices"
  },
  reading: {
    kicker: "阅读",
    title: "阅读指标",
    subtitle: "聚合今日阅读、本周时长和累计阅读完成情况。",
    badge: "Reading"
  },
  zotero: {
    kicker: "Zotero",
    title: "Zotero 附件工作流",
    subtitle: "查看最近文献附件、按需推送，并在缺失本地文件时走 WebDAV 拉取链路。",
    badge: "Zotero"
  },
  calibre: {
    kicker: "Calibre",
    title: "Calibre 书库工作流",
    subtitle: "直接读取 metadata.db，展示最近书籍并按数据库标题推送到 BOOX。",
    badge: "Calibre"
  }
};

function createZoteroForm() {
  return {
    profileDir: "",
    dataDir: "",
    webdavUrl: "",
    webdavUsername: "",
    webdavPassword: ""
  };
}

function createCalibreForm() {
  return {
    libraryDirsText: ""
  };
}

const LIBRARY_PAGE_SIZE = 50;

const state = {
  timer: null,
  syncTimer: null,
  zoteroSearchTimer: null,
  calibreSearchTimer: null,
  snapshot: null,
  loading: false,
  pendingForce: false,
  authTimer: null,
  authTimerTicks: 0,
  refreshMs: 60000,
  uploadStatusOverride: "",
  uploadStatusOverrideUntil: 0,
  activeView: "overview",
  zotero: {
    status: null,
    items: [],
    page: 0,
    hasMore: false,
    loadingStatus: false,
    loadingItems: false,
    pushingAttachmentId: null,
    phase: "idle",
    phaseError: "",
    form: createZoteroForm(),
    detected: null,
    filterText: ""
  },
  calibre: {
    status: null,
    books: [],
    page: 0,
    hasMore: false,
    loadingStatus: false,
    loadingBooks: false,
    pushingFormatKey: null,
    showBrokenOnly: false,
    scrollTop: 0,
    activeBookKey: "",
    phase: "idle",
    phaseError: "",
    form: createCalibreForm(),
    filterText: ""
  }
};

function $(id) {
  return document.getElementById(id);
}

function contentRoot() {
  return $("content-root");
}

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

function escapeHtml(value) {
  return String(value ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
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
  return new Date(n).toLocaleString();
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

function zoteroStateText(raw) {
  if (raw === "connected") return "已连接";
  if (raw === "pending") return "待补全";
  if (raw === "failed") return "失败";
  if (raw === "detecting") return "检测中";
  if (raw === "validating") return "验证中";
  return "未检测";
}

function missingFieldText(key) {
  if (key === "profile_dir") return "缺少 profile 目录";
  if (key === "data_dir") return "缺少 dataDir 或 zotero.sqlite";
  if (key === "webdav_protocol") return "当前协议不是 WebDAV";
  if (key === "webdav_url") return "缺少 WebDAV 地址";
  if (key === "webdav_username") return "缺少 WebDAV 用户名";
  if (key === "webdav_password") return "缺少 WebDAV 密码";
  return key || "-";
}

function formatZoteroStateNote(status) {
  if (!status) return "尚未检测 Zotero 配置。";
  if (state.zotero.phase === "detecting") return "正在自动检测 Zotero profile、dataDir 和 WebDAV 配置。";
  if (state.zotero.phase === "validating") return "正在验证 WebDAV 凭据并保存到系统安全存储。";
  if (state.zotero.phaseError) return state.zotero.phaseError;
  const missing = Array.isArray(status.missing_fields) ? status.missing_fields : [];
  if (missing.length > 0) {
    return `待补全: ${missing.map(missingFieldText).join("，")}`;
  }
  if (status.last_error) return status.last_error;
  if (status.state === "connected") return "Zotero 本地库和 WebDAV 凭据都已就绪，可直接进入附件工作流。";
  return "可以先自动检测，再补全缺失配置。";
}

function syncZoteroFormFromStatus(status, options = {}) {
  const preservePassword = options.preservePassword !== false;
  const summary = status?.summary || {};
  const next = createZoteroForm();
  next.profileDir = summary.profile_dir || "";
  next.dataDir = summary.data_dir || "";
  next.webdavUrl = summary.webdav_url || "";
  next.webdavUsername = summary.webdav_username || "";
  if (preservePassword) {
    next.webdavPassword = state.zotero.form.webdavPassword || "";
  }
  state.zotero.form = next;
}

function buildZoteroStatusRows(status) {
  const summary = status?.summary || {};
  const rows = [
    ["状态", zoteroStateText(state.zotero.phase === "detecting" ? "detecting" : state.zotero.phase === "validating" ? "validating" : status?.state)],
    ["Profile", summary.profile_dir || "未检测到"],
    ["Data Dir", summary.data_dir || "未检测到"],
    ["数据库", summary.database_exists ? summary.database_path || "已找到" : "未找到 zotero.sqlite"],
    ["协议", summary.protocol || "未检测到"],
    ["WebDAV 地址", summary.webdav_url || "未检测到"],
    ["WebDAV 用户名", summary.webdav_username || "未检测到"],
    ["密码", summary.password_saved ? "已保存在系统安全存储" : "尚未保存"],
    ["下载模式", summary.download_mode_personal || "未知"]
  ];
  return rows
    .map(
      ([label, value]) => `
        <div class="account-row">
          <p class="account-key">${escapeHtml(label)}</p>
          <p class="account-value">${escapeHtml(String(value || "-"))}</p>
        </div>
      `
    )
    .join("");
}

function buildZoteroStatusChips(status) {
  const summary = status?.summary || {};
  const chips = [
    `状态: ${zoteroStateText(state.zotero.phase === "detecting" ? "detecting" : state.zotero.phase === "validating" ? "validating" : status?.state)}`,
    `数据库: ${summary.database_exists ? "已找到" : "未找到"}`,
    `协议: ${summary.protocol || "未检测到"}`,
    `密码: ${summary.password_saved ? "已保存" : "待补全"}`
  ];
  return chips
    .map((text) => `<span class="status-chip pending zotero-chip">${escapeHtml(text)}</span>`)
    .join("");
}

function detectionSourceText(key) {
  const detected = state.zotero.detected || {};
  if (key === "profileDir") return detected.profile_source || "";
  if (key === "dataDir") return detected.data_dir_source || "";
  if (key === "webdavUrl") return detected.webdav_url_source || "";
  if (key === "webdavUsername") return detected.webdav_username_source || "";
  return "";
}

function fieldStatusText(key, status) {
  const summary = status?.summary || {};
  if (key === "profileDir") return summary.profile_dir ? "已检测到" : "未检测到";
  if (key === "dataDir") return summary.database_exists ? "数据库已就绪" : "请补全 dataDir";
  if (key === "webdavUrl") return summary.webdav_url ? "已检测到" : "待补全";
  if (key === "webdavUsername") return summary.webdav_username ? "已检测到" : "待补全";
  if (key === "webdavPassword") return summary.password_saved ? "系统安全存储中已有密码" : "输入后保存到系统安全存储";
  return "";
}

function buildEditableField({ label, field, type = "text", value, placeholder, actionHtml = "", status }) {
  const source = detectionSourceText(field);
  const fieldStatus = fieldStatusText(field, status);
  return `
    <label class="zotero-field">
      <span class="zotero-field-head">
        <span>${escapeHtml(label)}</span>
        <span class="zotero-field-meta">${escapeHtml(source || fieldStatus || "")}</span>
      </span>
      <div class="zotero-inline-field">
        <input data-zotero-field="${escapeHtml(field)}" type="${escapeHtml(type)}" value="${escapeHtml(value)}" placeholder="${escapeHtml(placeholder)}" />
        ${actionHtml}
      </div>
    </label>
  `;
}

function buildZoteroConnectionCard(options = {}) {
  const embedded = options.embedded === true;
  const status = state.zotero.status;
  const form = state.zotero.form;
  const note = formatZoteroStateNote(status);
  const connected = status?.state === "connected";

  return `
    <section class="panel-card soft zotero-connection-card">
      <div class="panel-header">
        <div>
          <h3>Zotero 连接与补全</h3>
          <p>${embedded ? "检测结果直接落到当前字段里，可在这里确认、补全并保存。" : "检测结果直接写入当前字段，你可以原地确认和编辑。"}</p>
        </div>
        <div class="section-actions">
          <button class="button button-tertiary button-xs" data-action="zotero-detect" type="button" ${state.zotero.loadingStatus ? "disabled" : ""}>自动检测</button>
          <button class="button button-tertiary button-xs" data-action="zotero-redetect" type="button" ${state.zotero.loadingStatus ? "disabled" : ""}>重新检测</button>
        </div>
      </div>
      <div class="zotero-panel-body">
        <p class="zotero-note">${escapeHtml(note)}</p>
        <div class="zotero-status-strip">${buildZoteroStatusChips(status)}</div>
        <div class="zotero-form">
          ${buildEditableField({
            label: "Profile 目录",
            field: "profileDir",
            value: form.profileDir,
            placeholder: "自动检测或手动选择 profile 目录",
            status,
            actionHtml: `<button class="button button-tertiary button-xs" data-action="zotero-pick-profile" type="button">选择</button>`
          })}
          ${buildEditableField({
            label: "Data Dir",
            field: "dataDir",
            value: form.dataDir,
            placeholder: "例如 /Volumes/.../zotero",
            status,
            actionHtml: `<button class="button button-tertiary button-xs" data-action="zotero-pick-data" type="button">选择</button>`
          })}
          ${buildEditableField({
            label: "WebDAV 地址",
            field: "webdavUrl",
            value: form.webdavUrl,
            placeholder: "例如 https://example.com/webdav",
            status
          })}
          ${buildEditableField({
            label: "WebDAV 用户名",
            field: "webdavUsername",
            value: form.webdavUsername,
            placeholder: "用于附件同步的用户名",
            status
          })}
          ${buildEditableField({
            label: "WebDAV 密码",
            field: "webdavPassword",
            type: "password",
            value: form.webdavPassword,
            placeholder: status?.summary?.password_saved ? "留空则继续使用已保存密码" : "输入后保存到系统安全存储",
            status
          })}
          <div class="section-actions">
            <button class="button button-primary" data-action="zotero-save" type="button" ${state.zotero.phase === "validating" ? "disabled" : ""}>保存并验证</button>
            ${connected ? `<button class="button button-secondary" data-view-target="zotero" type="button">打开附件工作流</button>` : ""}
          </div>
        </div>
      </div>
    </section>
  `;
}

function calibreStateText(raw) {
  if (raw === "connected") return "已连接";
  if (raw === "pending") return "待补全";
  if (raw === "failed") return "失败";
  if (raw === "detecting") return "检测中";
  if (raw === "validating") return "保存中";
  return "未检测";
}

function calibreMissingFieldText(key) {
  if (key === "library_dirs") return "缺少书库目录";
  if (key === "database_path") return "未找到 metadata.db";
  return key || "-";
}

function formatCalibreStateNote(status) {
  if (!status) return "尚未检测 Calibre 书库。";
  if (state.calibre.phase === "detecting") return "正在检测默认 Calibre 书库目录。";
  if (state.calibre.phase === "validating") return "正在保存并校验 Calibre 书库目录。";
  if (state.calibre.phaseError) return state.calibre.phaseError;
  const missing = Array.isArray(status.missing_fields) ? status.missing_fields : [];
  if (missing.length > 0) {
    if (Number(status?.summary?.ready_library_count || 0) > 0) {
      return `部分书库可用: 已就绪 ${Number(status?.summary?.ready_library_count || 0)} 个，待修复 ${missing.map(calibreMissingFieldText).join("，")}`;
    }
    return `待补全: ${missing.map(calibreMissingFieldText).join("，")}`;
  }
  if (status.last_error) return status.last_error;
  if (status.state === "connected") {
    return "Calibre 书库已就绪，推送时会优先使用 metadata.db 里的书名，而不是拼音文件名。";
  }
  return "可以先自动检测，找不到时再手动选择书库目录。";
}

function syncCalibreFormFromStatus(status) {
  const summary = status?.summary || {};
  state.calibre.form = {
    libraryDirsText: Array.isArray(summary.library_dirs) ? summary.library_dirs.join("\n") : ""
  };
}

function parseCalibreLibraryDirs(text) {
  return String(text || "")
    .split(/\r?\n/)
    .map((value) => value.trim())
    .filter(Boolean);
}

function buildCalibreStatusChips(status) {
  const summary = status?.summary || {};
  const phaseState = state.calibre.phase === "detecting"
    ? "detecting"
    : state.calibre.phase === "validating"
      ? "validating"
      : status?.state;
  const chips = [
    `状态: ${calibreStateText(phaseState)}`,
    `书库: ${Number(summary.total_library_count || 0)} 个`,
    `数据库: ${Number(summary.ready_library_count || 0)}/${Number(summary.total_library_count || 0)} 已就绪`
  ];
  return chips
    .map((text) => `<span class="status-chip pending zotero-chip">${escapeHtml(text)}</span>`)
    .join("");
}

function buildCalibreConfiguredLibraryList(status) {
  const summary = status?.summary || {};
  const dirs = Array.isArray(summary.library_dirs) ? summary.library_dirs : [];
  const dbs = Array.isArray(summary.database_paths) ? summary.database_paths : [];
  const readyDirs = new Set(Array.isArray(summary.ready_library_dirs) ? summary.ready_library_dirs : []);
  if (dirs.length === 0) return "";
  const rows = dirs
    .map((dir, index) => {
      const dbPath = dbs[index] || "";
      const ready = readyDirs.has(dir);
      return { dir, dbPath, ready };
    })
    .filter((row) => !state.calibre.showBrokenOnly || !row.ready);
  return `
    <div class="calibre-library-list">
      ${rows.length > 0 ? rows.map((row) => {
        return `
          <div class="calibre-library-row ${row.ready ? "is-ready" : "is-broken"}">
            <div class="calibre-library-main">
              <p class="push-name">${escapeHtml(row.dir)}</p>
              <p class="push-meta">${escapeHtml(row.ready ? (row.dbPath || "metadata.db 已就绪") : "未找到 metadata.db")}</p>
            </div>
            <div class="row-actions">
              <span class="zotero-count-pill ${row.ready ? "is-ready" : ""}">${escapeHtml(row.ready ? "已就绪" : "待修复")}</span>
              <button
                class="button button-tertiary button-xs"
                data-action="calibre-remove-library"
                data-library-dir="${escapeHtml(row.dir)}"
                type="button"
                ${state.calibre.phase === "validating" ? "disabled" : ""}
              >移除</button>
            </div>
          </div>
        `;
      }).join("") : `
        <div class="calibre-library-empty">
          <p class="inline-note">当前没有异常书库，所有已配置目录都已就绪。</p>
        </div>
      `}
    </div>
  `;
}

function buildCalibreConnectionCard(options = {}) {
  const embedded = options.embedded === true;
  const status = state.calibre.status;
  const form = state.calibre.form;
  const note = formatCalibreStateNote(status);
  const readyCount = Number(status?.summary?.ready_library_count || 0);
  const totalCount = Number(status?.summary?.total_library_count || 0);
  const brokenCount = Math.max(0, totalCount - readyCount);
  const connected = readyCount > 0;

  return `
    <section class="panel-card soft zotero-connection-card">
      <div class="panel-header">
        <div>
          <h3>Calibre 书库连接</h3>
          <p>${embedded ? "读取本地 metadata.db 并用数据库标题发起推送，避免中文书名被拼音文件名替代。" : "连接书库后，可直接浏览最近书籍和可推送格式。"}</p>
        </div>
        <div class="section-actions">
          <button class="button button-tertiary button-xs" data-action="calibre-detect" type="button" ${state.calibre.loadingStatus ? "disabled" : ""}>自动检测</button>
          <button class="button button-tertiary button-xs" data-action="calibre-refresh-libraries" type="button" ${state.calibre.loadingStatus ? "disabled" : ""}>检查全部</button>
          <button class="button button-tertiary button-xs" data-action="calibre-pick-library" type="button">选择目录</button>
        </div>
      </div>
      <div class="zotero-panel-body">
        <p class="zotero-note">${escapeHtml(note)}</p>
        <div class="zotero-status-strip">${buildCalibreStatusChips(status)}</div>
        <div class="zotero-form">
          <label class="zotero-field">
            <span class="zotero-field-head">
              <span>书库目录列表</span>
              <span class="zotero-field-meta">${escapeHtml(totalCount > 0 ? `已配置 ${totalCount} 个，已就绪 ${readyCount} 个` : "一行一个目录，需包含 metadata.db")}</span>
            </span>
            <textarea
              data-calibre-field="libraryDirsText"
              class="zotero-textarea"
              rows="4"
              placeholder="例如 /Users/you/Calibre Library&#10;/Volumes/SSD/Books/Calibre Library"
            >${escapeHtml(form.libraryDirsText || "")}</textarea>
          </label>
          ${totalCount > 0 ? `
            <div class="section-actions">
              <button
                class="button ${state.calibre.showBrokenOnly ? "button-secondary" : "button-tertiary"} button-xs"
                data-action="calibre-toggle-broken-only"
                type="button"
              >${escapeHtml(state.calibre.showBrokenOnly ? `显示全部 (${totalCount})` : `只看异常书库 (${brokenCount})`)}</button>
            </div>
          ` : ""}
          ${buildCalibreConfiguredLibraryList(status)}
          <div class="section-actions">
            <button class="button button-tertiary" data-action="calibre-pick-library" type="button">追加目录</button>
            <button class="button button-primary" data-action="calibre-save" type="button" ${state.calibre.phase === "validating" ? "disabled" : ""}>保存</button>
            ${connected ? `<button class="button button-secondary" data-view-target="calibre" type="button">打开书库工作流</button>` : ""}
          </div>
        </div>
      </div>
    </section>
  `;
}

function buildZoteroAttachmentActions(attachment) {
  if (Number(attachment.link_mode) !== 0) {
    return `<button class="button button-tertiary button-xs" type="button" disabled>暂不支持</button>`;
  }
  const busy = String(state.zotero.pushingAttachmentId || "") === String(attachment.attachment_item_id);
  return `
    <button
      class="button button-primary button-xs"
      data-action="zotero-push-attachment"
      data-attachment-id="${escapeHtml(String(attachment.attachment_item_id))}"
      type="button"
      ${attachment.can_push_directly || attachment.can_download_from_webdav ? "" : "disabled"}
      ${busy ? "disabled" : ""}
    >${escapeHtml(busy ? "推送中..." : "推送")}</button>
  `;
}

function zoteroAttachmentModeText(linkMode) {
  if (Number(linkMode) === 0) return "stored attachment";
  if (Number(linkMode) === 1) return "link to file";
  if (Number(linkMode) === 2) return "imported url";
  if (Number(linkMode) === 3) return "linked url";
  return `link mode ${String(linkMode ?? "-")}`;
}

function zoteroAttachmentRouteText(attachment) {
  if (Number(attachment.link_mode) !== 0) return "当前不是 stored attachment，第一版不支持直接推送。";
  if (attachment.can_push_directly) return "本地附件已就绪，会直接走现有上传链路。";
  if (attachment.can_download_from_webdav) return "本地缺失，推送时会先从 WebDAV 拉取临时文件。";
  return "附件不可直接推送，请先在 Zotero 中确认本地文件或同步状态。";
}

function zoteroSearchText(item) {
  const attachments = Array.isArray(item?.attachments) ? item.attachments : [];
  return [
    item?.title,
    item?.author_summary,
    item?.year,
    item?.date_modified,
    ...attachments.flatMap((attachment) => [
      attachment?.file_name,
      attachment?.attachment_key,
      attachment?.content_type,
      attachment?.status_label
    ])
  ]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();
}

function getZoteroItemsWithAttachments(items) {
  const list = Array.isArray(items) ? items : [];
  return list.filter((item) => Array.isArray(item?.attachments) && item.attachments.length > 0);
}

function getFilteredZoteroItems(items, filterText) {
  const list = getZoteroItemsWithAttachments(items);
  const needle = String(filterText || "").trim().toLowerCase();
  if (!needle) return list;
  return list.filter((item) => zoteroSearchText(item).includes(needle));
}

function buildZoteroWorkflowStats(items) {
  const list = getZoteroItemsWithAttachments(items);
  const attachmentCount = list.reduce((sum, item) => sum + (Array.isArray(item.attachments) ? item.attachments.length : 0), 0);
  const pushableCount = list.reduce(
    (sum, item) =>
      sum +
      (Array.isArray(item.attachments)
        ? item.attachments.filter((attachment) => attachment.can_push_directly || attachment.can_download_from_webdav).length
        : 0),
    0
  );
  const localReadyCount = list.reduce(
    (sum, item) =>
      sum + (Array.isArray(item.attachments) ? item.attachments.filter((attachment) => attachment.can_push_directly).length : 0),
    0
  );
  return `
    <div class="zotero-workflow-stats">
      <span class="zotero-count-pill">${escapeHtml(`${list.length} 条文献`)}</span>
      <span class="zotero-count-pill">${escapeHtml(`${attachmentCount} 个附件`)}</span>
      <span class="zotero-count-pill is-ready">${escapeHtml(`可推送 ${pushableCount}`)}</span>
      <span class="zotero-count-pill">${escapeHtml(`本地就绪 ${localReadyCount}`)}</span>
    </div>
  `;
}

function buildWorkflowPagination(kind, page, hasMore, loading) {
  const prevDisabled = loading || page <= 0;
  const nextDisabled = loading || !hasMore;
  return `
    <div class="workflow-pagination">
      <p class="inline-note">第 ${escapeHtml(String(page + 1))} 页 · 每页 ${escapeHtml(String(LIBRARY_PAGE_SIZE))} 条</p>
      <div class="section-actions">
        <button
          class="button button-tertiary button-xs"
          data-action="${escapeHtml(`${kind}-page-prev`)}"
          type="button"
          ${prevDisabled ? "disabled" : ""}
        >上一页</button>
        <button
          class="button button-tertiary button-xs"
          data-action="${escapeHtml(`${kind}-page-next`)}"
          type="button"
          ${nextDisabled ? "disabled" : ""}
        >下一页</button>
      </div>
    </div>
  `;
}

function buildZoteroWorkflowItems(items) {
  if (!Array.isArray(items) || items.length === 0) {
    return `
      <div class="empty-card compact">
        <h3>暂无可展示的条目</h3>
        <p class="empty-copy">连接完成后会展示个人库文献及其附件状态。</p>
      </div>
    `;
  }
  return items
    .map((item) => {
      const attachments = Array.isArray(item.attachments) ? item.attachments : [];
      const readyCount = attachments.filter((attachment) => attachment.can_push_directly || attachment.can_download_from_webdav).length;
      const attachmentHtml = attachments.length
        ? attachments
            .map(
              (attachment) => `
                <div class="zotero-attachment-row">
                  <div class="zotero-attachment-main">
                    <p class="push-name">${escapeHtml(attachment.file_name || attachment.attachment_key || "未命名附件")}</p>
                    <p class="push-meta">${escapeHtml([
                      attachment.content_type || "未知类型",
                      zoteroAttachmentModeText(attachment.link_mode),
                      attachment.status_label || "-"
                    ].filter(Boolean).join(" · "))}</p>
                    <p class="inline-note">${escapeHtml(zoteroAttachmentRouteText(attachment))}</p>
                  </div>
                  <div class="row-actions">
                    ${buildZoteroAttachmentActions(attachment)}
                  </div>
                </div>
              `
            )
            .join("")
        : `
          <div class="zotero-attachment-row empty">
            <div class="zotero-attachment-main">
              <p class="push-meta">当前条目没有附件。</p>
            </div>
          </div>
        `;
      return `
        <article class="list-item zotero-item-card">
          <div class="zotero-item-card-head">
            <div class="list-item-main zotero-item-card-main">
              <p class="list-title">${escapeHtml(item.title || "未命名条目")}</p>
              <p class="list-meta">${escapeHtml([item.author_summary, item.year, item.date_modified].filter(Boolean).join(" · ") || "最近修改条目")}</p>
            </div>
            <div class="zotero-item-card-side">
              <span class="zotero-count-pill">${escapeHtml(`${attachments.length} 个附件`)}</span>
              <span class="zotero-count-pill ${readyCount > 0 ? "is-ready" : ""}">${escapeHtml(`可推送 ${readyCount}`)}</span>
            </div>
          </div>
          <div class="zotero-item-card-body">
            ${attachmentHtml}
          </div>
        </article>
      `;
    })
    .join("");
}

function calibreSearchText(book) {
  const formats = Array.isArray(book?.formats) ? book.formats : [];
  return [
    book?.title,
    book?.author_summary,
    book?.published_year,
    book?.library_label,
    book?.library_dir,
    book?.date_modified,
    ...formats.flatMap((format) => [
      format?.format,
      format?.file_name,
      format?.status_label
    ])
  ]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();
}

function getCalibreBooksWithFormats(books) {
  const list = Array.isArray(books) ? books : [];
  return list.filter((book) => Array.isArray(book?.formats) && book.formats.length > 0);
}

function getFilteredCalibreBooks(books, filterText) {
  const list = getCalibreBooksWithFormats(books);
  const needle = String(filterText || "").trim().toLowerCase();
  if (!needle) return list;
  return list.filter((book) => calibreSearchText(book).includes(needle));
}

function buildCalibreWorkflowStats(books) {
  const list = getCalibreBooksWithFormats(books);
  const libraryCount = new Set(list.map((book) => String(book.library_dir || ""))).size;
  const formatCount = list.reduce((sum, book) => sum + (Array.isArray(book.formats) ? book.formats.length : 0), 0);
  const pushableCount = list.reduce(
    (sum, book) =>
      sum + (Array.isArray(book.formats) ? book.formats.filter((format) => format.can_push_directly).length : 0),
    0
  );
  return `
    <div class="zotero-workflow-stats">
      <span class="zotero-count-pill">${escapeHtml(`${libraryCount} 个书库`)}</span>
      <span class="zotero-count-pill">${escapeHtml(`${list.length} 本书`)}</span>
      <span class="zotero-count-pill">${escapeHtml(`${formatCount} 个格式`)}</span>
      <span class="zotero-count-pill is-ready">${escapeHtml(`可推送 ${pushableCount}`)}</span>
    </div>
  `;
}

function buildCalibreFormatActions(format) {
  const formatKey = `${String(format.library_dir || "")}::${String(format.data_id || "")}`;
  const busy = String(state.calibre.pushingFormatKey || "") === formatKey;
  return `
    <button
      class="button button-primary button-xs"
      data-action="calibre-push-format"
      data-library-dir="${escapeHtml(String(format.library_dir || ""))}"
      data-data-id="${escapeHtml(String(format.data_id || ""))}"
      type="button"
      ${format.can_push_directly && !busy ? "" : "disabled"}
    >${escapeHtml(busy ? "推送中..." : "推送")}</button>
  `;
}

function buildCalibreWorkflowBooks(books) {
  if (!Array.isArray(books) || books.length === 0) {
    return `
      <div class="empty-card compact">
        <h3>暂无可展示的书籍</h3>
        <p class="empty-copy">连接完成后，这里会展示带格式文件的 Calibre 书籍。</p>
      </div>
    `;
  }
  return books
    .map((book) => {
      const bookKey = buildCalibreBookKey(book);
      const formats = Array.isArray(book.formats) ? book.formats : [];
      const readyCount = formats.filter((format) => format.can_push_directly).length;
      const formatHtml = formats
        .map(
          (format) => `
            <div class="zotero-attachment-row">
              <div class="zotero-attachment-main">
                <p class="push-name">${escapeHtml(format.file_name || `${book.title || "未命名书籍"}.${String(format.format || "").toLowerCase()}`)}</p>
                <p class="push-meta">${escapeHtml([
                  format.format || "未知格式",
                  format.file_size ? bytesToText(format.file_size) : null,
                  format.status_label || "-"
                ].filter(Boolean).join(" · "))}</p>
                <p class="inline-note">${escapeHtml(format.local_exists ? "将直接读取本地书籍文件，并把 BOOX 侧标题改成 metadata.db 中的书名。" : "未找到本地文件，请先确认 Calibre 书库完整同步。")}</p>
              </div>
              <div class="row-actions">
                ${buildCalibreFormatActions(format)}
              </div>
            </div>
          `
        )
        .join("");
      return `
        <article
          class="list-item zotero-item-card ${state.calibre.activeBookKey === bookKey ? "is-active" : ""}"
          data-calibre-book-key="${escapeHtml(bookKey)}"
        >
          <div class="zotero-item-card-head">
            <div class="list-item-main zotero-item-card-main">
              <p class="list-title">${escapeHtml(book.title || "未命名书籍")}</p>
              <p class="list-meta">${escapeHtml([book.author_summary, book.published_year, book.library_label, book.date_modified].filter(Boolean).join(" · ") || "最近修改书籍")}</p>
            </div>
            <div class="zotero-item-card-side">
              <span class="zotero-count-pill">${escapeHtml(`${formats.length} 个格式`)}</span>
              <span class="zotero-count-pill ${readyCount > 0 ? "is-ready" : ""}">${escapeHtml(`可推送 ${readyCount}`)}</span>
            </div>
          </div>
          <div class="zotero-item-card-body">
            ${formatHtml}
          </div>
        </article>
      `;
    })
    .join("");
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
  return readTimeWeek?.now?.totalTime ?? readTimeWeek?.totalTime ?? readTimeWeek?.weekTotalTime ?? 0;
}

function setUploadStatusOverride(value, holdMs = 0) {
  state.uploadStatusOverride = value || "";
  state.uploadStatusOverrideUntil = holdMs > 0 ? Date.now() + holdMs : 0;
  updateSidebarUploadStatus();
  if (state.snapshot) {
    renderCurrentView();
  }
}

function clearUploadStatusOverride() {
  state.uploadStatusOverride = "";
  state.uploadStatusOverrideUntil = 0;
}

function getVisibleUploadStatus(snapshotStatus) {
  const now = Date.now();
  const overrideActive = !!state.uploadStatusOverride && now < Number(state.uploadStatusOverrideUntil || 0);
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
  const input = $("refresh-interval-minutes");
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

function getSavedView() {
  const saved = localStorage.getItem("s2b_dashboard_active_view");
  if (saved && VIEW_META[saved]) return saved;
  return "overview";
}

function getInitialCalibreShowBrokenOnly() {
  return localStorage.getItem("s2b_calibre_show_broken_only") === "1";
}

function setCalibreShowBrokenOnly(value) {
  state.calibre.showBrokenOnly = !!value;
  localStorage.setItem("s2b_calibre_show_broken_only", state.calibre.showBrokenOnly ? "1" : "0");
}

function getInitialCalibreFilterText() {
  return localStorage.getItem("s2b_calibre_filter_text") || "";
}

function setCalibreFilterText(value) {
  state.calibre.filterText = String(value || "");
  localStorage.setItem("s2b_calibre_filter_text", state.calibre.filterText);
}

function getInitialCalibreScrollTop() {
  const value = Number(localStorage.getItem("s2b_calibre_scroll_top"));
  return Number.isFinite(value) && value > 0 ? value : 0;
}

function setCalibreScrollTop(value) {
  const next = Math.max(0, Number(value) || 0);
  state.calibre.scrollTop = next;
  localStorage.setItem("s2b_calibre_scroll_top", String(next));
}

function buildCalibreBookKey(book) {
  return `${String(book?.library_dir || "")}::${String(book?.book_id || "")}`;
}

function getInitialCalibreActiveBookKey() {
  return localStorage.getItem("s2b_calibre_active_book_key") || "";
}

function setCalibreActiveBookKey(value) {
  state.calibre.activeBookKey = String(value || "");
  localStorage.setItem("s2b_calibre_active_book_key", state.calibre.activeBookKey);
}

function getCalibreScrollContainer() {
  return document.querySelector('[data-calibre-scroll-container="true"]');
}

function getCalibreActiveBookElement() {
  if (!state.calibre.activeBookKey) return null;
  return document.querySelector(`[data-calibre-book-key="${CSS.escape(state.calibre.activeBookKey)}"]`);
}

function saveCalibreScrollPositionFromDom() {
  const container = getCalibreScrollContainer();
  if (container instanceof HTMLElement) {
    setCalibreScrollTop(container.scrollTop);
  }
}

function restoreCalibreScrollPosition() {
  const targetTop = Math.max(0, Number(state.calibre.scrollTop || 0));
  requestAnimationFrame(() => {
    const container = getCalibreScrollContainer();
    if (container instanceof HTMLElement) {
      container.scrollTop = targetTop;
    }
    const activeBook = getCalibreActiveBookElement();
    if (activeBook instanceof HTMLElement) {
      activeBook.scrollIntoView({ block: "nearest" });
    }
  });
}

function setActiveView(view, options = {}) {
  if (!VIEW_META[view]) return;
  state.activeView = view;
  if (options.persist !== false) {
    localStorage.setItem("s2b_dashboard_active_view", view);
  }
  syncNavState();
  renderToolbar();
  renderCurrentView();
}

function syncNavState() {
  document.querySelectorAll(".nav-item[data-view]").forEach((item) => {
    item.classList.toggle("is-active", item.dataset.view === state.activeView);
  });
}

function authPresentation(auth) {
  const authorized = !!auth?.authorized;
  return {
    authorized,
    chipText: authorized ? "已授权" : "待授权",
    chipClass: authorized ? "status-chip authorized" : "status-chip pending",
    actionText: authorized ? "重新授权" : "浏览器登录"
  };
}

function renderToolbar() {
  const meta = VIEW_META[state.activeView] || VIEW_META.overview;
  const snapshot = state.snapshot;
  let badge = meta.badge;
  if (snapshot) {
    if (state.activeView === "push") {
      badge = `${(snapshot.push_queue || []).length} 项`;
    } else if (state.activeView === "devices") {
      badge = `${(snapshot.devices || []).length} 台`;
    } else if (state.activeView === "reading") {
      badge = `${computeTodayReadCount(snapshot?.calendar_metrics?.day_read_today || {})} 今日`;
    } else if (state.activeView === "zotero") {
      badge = zoteroStateText(state.zotero.status?.state);
    } else if (state.activeView === "calibre") {
      badge = calibreStateText(state.calibre.status?.state);
    } else if (state.activeView === "overview") {
      badge = snapshot?.auth?.authorized ? "Live" : "Preview";
    }
  }
  $("toolbar-section-kicker").textContent = meta.kicker;
  $("toolbar-title").textContent = meta.title;
  $("toolbar-subtitle").textContent = meta.subtitle;
  $("toolbar-badge").textContent = badge;
}

function updateSidebarUploadStatus() {
  const upload = state.snapshot?.upload || {};
  $("sidebar-upload-status").textContent = getVisibleUploadStatus(upload.status_text || "上传进度: 空闲");
}

function renderSidebarAuth(snapshot) {
  const auth = snapshot?.auth || {};
  const profile = snapshot?.profile || {};
  const storage = snapshot?.storage || {};
  const authUi = authPresentation(auth);
  const used = storage.used;
  const limit = storage.limit;
  const storagePercent = Number(storage.percent || 0);

  const chip = $("auth-chip");
  chip.textContent = authUi.chipText;
  chip.className = authUi.chipClass;
  $("sidebar-profile-name").textContent = authUi.authorized
    ? profile.nickname || "已连接账户"
    : "未登录";
  $("sidebar-profile-subtitle").textContent = authUi.authorized
    ? `${profile.uid ? `uid: ${profile.uid}` : "授权已同步"} · ${auth.source || "unknown"}`
    : auth.message || "请使用浏览器完成授权";

  const loginBtn = $("sidebar-login-btn");
  loginBtn.disabled = false;
  loginBtn.textContent = authUi.actionText;

  $("storage-text").textContent =
    used != null || limit != null ? `${bytesToText(used)} / ${bytesToText(limit)}` : "未知";
  $("storage-bar").style.width = `${Math.max(0, Math.min(100, storagePercent)).toFixed(2)}%`;
  $("sync-time").textContent = `更新于 ${timeAgoText(snapshot?.fetched_at_ms)}`;
  updateSidebarUploadStatus();
}

function renderErrorView(message) {
  const root = contentRoot();
  if (!root) return;
  root.innerHTML = `
    <section class="view">
      <div class="empty-card">
        <h3>无法加载主界面</h3>
        <p class="empty-copy">${escapeHtml(message)}</p>
      </div>
    </section>
  `;
}

function buildPushItems(items, options = {}) {
  const list = Array.isArray(items) ? items : [];
  const limit = Number.isFinite(options.limit) ? options.limit : list.length;
  const visible = list.slice(0, limit);
  const emptyText = options.emptyText || "暂无互动文件";
  if (visible.length === 0) {
    return `
      <li class="list-item">
        <div class="list-item-main">
          <p class="list-title">${escapeHtml(emptyText)}</p>
          <p class="list-meta">当有上传成功的互动文件后，会出现在这里。</p>
        </div>
      </li>
    `;
  }
  return visible
    .map((item) => {
      const meta = `${bytesToText(item.size)} · ${toDateText(item.updated_at)}`;
      return `
        <li class="list-item" data-push-id="${escapeHtml(item.id || "")}">
          <div class="list-item-main">
            <p class="list-title">${escapeHtml(item.name || "(未命名文件)")}</p>
            <p class="list-meta">${escapeHtml(meta)}</p>
          </div>
          <div class="row-actions">
            <button class="button button-tertiary button-xs" data-action="resend" type="button">推送</button>
            <button class="button button-danger button-xs" data-action="delete" type="button">删除</button>
          </div>
        </li>
      `;
    })
    .join("");
}

function buildLanDeviceButtons(devices, options = {}) {
  const list = (devices || []).filter((item) => !!item?.same_lan);
  const limit = Number.isFinite(options.limit) ? options.limit : list.length;
  const visible = list.slice(0, limit);
  if (visible.length === 0) {
    return `
      <button class="lan-device-button empty" type="button" disabled>
        <span class="lan-device-title">未发现同局域网 BOOX 设备</span>
        <span class="lan-device-subtitle">当设备在线且位于同网段时，会在这里出现快捷入口。</span>
      </button>
    `;
  }
  return visible
    .map((item) => {
      const model = item.model || "BOOX 设备";
      const transferHost = item.transfer_host || "";
      const ip = item.lan_ip || item.ip_address || "";
      const hostText = transferHost ? transferHost.replace(/^https?:\/\//, "") : ip;
      const note = item.same_lan_reason ? `识别来源: ${item.same_lan_reason}` : "可直接打开互传地址";
      return `
        <button
          class="lan-device-button"
          type="button"
          data-action="open-transfer"
          data-host="${escapeHtml(transferHost)}"
          data-model="${escapeHtml(model)}"
          data-ip="${escapeHtml(ip)}"
          data-device-id="${escapeHtml(item.id || "")}"
          data-mac="${escapeHtml(item.mac_address || "")}"
          data-status="${escapeHtml(item.login_status || "")}"
          data-reason="${escapeHtml(item.same_lan_reason || "")}"
        >
          <span class="lan-device-title">${escapeHtml(hostText ? `${model} · ${hostText}` : model)}</span>
          <span class="lan-device-subtitle">${escapeHtml(note)}</span>
        </button>
      `;
    })
    .join("");
}

function buildAllDeviceItems(devices) {
  const list = Array.isArray(devices) ? devices : [];
  if (list.length === 0) {
    return `
      <li class="device-item">
        <div class="device-item-main">
          <p class="device-title">暂无设备信息</p>
          <p class="device-meta">登录后桌面端会同步当前账户下的设备状态。</p>
        </div>
      </li>
    `;
  }
  return list
    .map((item) => {
      const model = item.model || "BOOX 设备";
      const ip = item.lan_ip || item.ip_address || "-";
      const status = item.login_status || "未知";
      const lastSeen = item.latest_login_time || item.latest_logout_time || "-";
      const meta = `状态: ${status} · IP: ${ip} · 最近时间: ${lastSeen}`;
      const action = item.transfer_host
        ? `
          <button
            class="button button-secondary button-xs"
            data-action="open-transfer"
            type="button"
            data-host="${escapeHtml(item.transfer_host)}"
            data-model="${escapeHtml(model)}"
            data-ip="${escapeHtml(ip)}"
            data-device-id="${escapeHtml(item.id || "")}"
            data-mac="${escapeHtml(item.mac_address || "")}"
            data-status="${escapeHtml(status)}"
            data-reason="${escapeHtml(item.same_lan_reason || "")}"
          >打开互传</button>
        `
        : "";
      return `
        <li class="device-item">
          <div class="device-item-main">
            <p class="device-title">${escapeHtml(model)}</p>
            <p class="device-meta">${escapeHtml(meta)}</p>
          </div>
          <div class="row-actions">${action}</div>
        </li>
      `;
    })
    .join("");
}

function renderOverview(snapshot) {
  const auth = snapshot?.auth || {};
  const profile = snapshot?.profile || {};
  const upload = snapshot?.upload || {};
  const devices = snapshot?.devices || [];
  const readingInfo = snapshot?.calendar_metrics?.reading_info || {};
  const readTimeWeek = snapshot?.calendar_metrics?.read_time_week || {};
  const dayReadToday = snapshot?.calendar_metrics?.day_read_today || {};
  const uploadPercent = Number.isFinite(Number(upload.progress_percent))
    ? Math.max(0, Math.min(100, Number(upload.progress_percent)))
    : 0;
  const visibleUploadStatus = getVisibleUploadStatus(upload.status_text || "上传进度: 空闲");
  const sameLanCount = devices.filter((item) => !!item?.same_lan).length;

  return `
    <section class="view">
      <div class="summary-grid">
        <div class="summary-card">
          <p class="summary-label">上传状态</p>
          <p class="summary-value">${escapeHtml(visibleUploadStatus)}</p>
          <p class="summary-note">${escapeHtml(
            `${uploadPercent.toFixed(1)}% · ${speedText(upload.speed_bps)} · 剩余 ${etaText(upload.eta_seconds)}`
          )}</p>
        </div>
        <div class="summary-card">
          <p class="summary-label">设备摘要</p>
          <p class="summary-value">${escapeHtml(`${devices.length} 台设备`)}</p>
          <p class="summary-note">${escapeHtml(`同局域网 ${sameLanCount} 台 · 最近同步 ${timeAgoText(snapshot?.fetched_at_ms)}`)}</p>
        </div>
        <div class="summary-card">
          <p class="summary-label">阅读摘要</p>
          <p class="summary-value">${escapeHtml(`${computeTodayReadCount(dayReadToday)} 本`)}</p>
          <p class="summary-note">${escapeHtml(`本周 ${durationText(computeWeekTotalTime(readTimeWeek))} · 累计完成 ${numberText(readingInfo.finished)}`)}</p>
        </div>
      </div>

      <div class="overview-grid">
        <div class="stack">
          <section class="panel-card">
            <div class="panel-header">
              <div>
                <h3>上传与推送</h3>
                <p>在桌面端发起上传，并查看最新的上传进度和互动文件状态。</p>
              </div>
              <div class="section-actions">
                <button class="button button-tertiary button-xs" data-action="refresh-view" type="button">刷新</button>
              </div>
            </div>
            <div class="upload-progress-block">
              <p class="upload-status-text">${escapeHtml(visibleUploadStatus)}</p>
              <div class="progress-track"><div class="progress-fill" style="width:${uploadPercent.toFixed(1)}%"></div></div>
              <p class="upload-metrics-text">${escapeHtml(
                `${uploadPercent.toFixed(1)}% · ${speedText(upload.speed_bps)} · 剩余 ${etaText(upload.eta_seconds)}${
                  upload.bytes_total ? ` · ${bytesToText(upload.bytes_sent || 0)}/${bytesToText(upload.bytes_total)}` : ""
                }`
              )}</p>
            </div>
          </section>

          ${buildCalibreConnectionCard({ embedded: true })}
        </div>

        <div class="stack">
          <section class="panel-card">
            <div class="panel-header">
              <div>
                <h3>同局域网设备</h3>
                <p>优先展示可直接打开互传地址的设备快捷入口。</p>
              </div>
              <div class="section-actions">
                <button class="button button-tertiary button-xs" data-view-target="devices" type="button">设备页</button>
              </div>
            </div>
            <div class="lan-devices">${buildLanDeviceButtons(devices, { limit: 4 })}</div>
          </section>

          <section class="panel-card soft">
            <div class="panel-header">
              <div>
                <h3>账户摘要</h3>
                <p>当前授权状态和主要账号信息。</p>
              </div>
            </div>
            <div class="account-list">
              <div class="account-row">
                <p class="account-key">授权状态</p>
                <p class="account-value ${auth.authorized ? "success-text" : "danger-text"}">${escapeHtml(
                  auth.authorized ? `已授权 · ${auth.source || "unknown"}` : `未授权 · ${auth.message || "请先登录"}`
                )}</p>
              </div>
              <div class="account-row">
                <p class="account-key">昵称</p>
                <p class="account-value">${escapeHtml(profile.nickname || "未获取到用户名")}</p>
              </div>
              <div class="account-row">
                <p class="account-key">UID</p>
                <p class="account-value">${escapeHtml(profile.uid || "-")}</p>
              </div>
            </div>
          </section>

          ${buildZoteroConnectionCard({ embedded: true })}
        </div>
      </div>
    </section>
  `;
}

function renderPushView(snapshot) {
  const upload = snapshot?.upload || {};
  const pushQueue = snapshot?.push_queue || [];
  const uploadPercent = Number.isFinite(Number(upload.progress_percent))
    ? Math.max(0, Math.min(100, Number(upload.progress_percent)))
    : 0;
  const visibleUploadStatus = getVisibleUploadStatus(upload.status_text || "上传进度: 空闲");

  return `
    <section class="view">
      <section class="panel-card">
        <div class="panel-header">
          <div>
            <h3>上传队列</h3>
            <p>从桌面端发起上传，并在这里查看进度、速率和剩余时间。</p>
          </div>
          <div class="section-actions">
            <button class="button button-tertiary button-xs" data-action="refresh-view" type="button">刷新</button>
          </div>
        </div>
        <div class="upload-progress-block">
          <p class="upload-status-text">${escapeHtml(visibleUploadStatus)}</p>
          <div class="progress-track"><div class="progress-fill" style="width:${uploadPercent.toFixed(1)}%"></div></div>
          <p class="upload-metrics-text">${escapeHtml(
            `${uploadPercent.toFixed(1)}% · ${speedText(upload.speed_bps)} · 剩余 ${etaText(upload.eta_seconds)}${
              upload.bytes_total ? ` · ${bytesToText(upload.bytes_sent || 0)}/${bytesToText(upload.bytes_total)}` : ""
            }`
          )}</p>
        </div>
      </section>

      <section class="panel-card soft">
        <div class="panel-header">
          <div>
            <h3>互动文件列表</h3>
            <p>使用行内操作直接重推到设备或删除云端推送记录。</p>
          </div>
        </div>
        <ul class="push-list">${buildPushItems(pushQueue, { emptyText: "暂无互动文件" })}</ul>
      </section>
    </section>
  `;
}

function renderDevicesView(snapshot) {
  const devices = snapshot?.devices || [];
  const sameLanCount = devices.filter((item) => !!item?.same_lan).length;

  return `
    <section class="view">
      <div class="devices-grid">
        <div class="stack">
          <section class="panel-card">
            <div class="panel-header">
              <div>
                <h3>同局域网设备</h3>
                <p>点击即可调用系统默认浏览器打开 BOOX 互传地址。</p>
              </div>
              <div class="section-actions">
                <button class="button button-tertiary button-xs" data-action="refresh-view" type="button">刷新</button>
              </div>
            </div>
            <div class="lan-devices">${buildLanDeviceButtons(devices)}</div>
          </section>
        </div>

        <div class="stack">
          <section class="panel-card soft">
            <div class="panel-header">
              <div>
                <h3>设备摘要</h3>
                <p>汇总当前账户下的设备数量与局域网可达情况。</p>
              </div>
            </div>
            <div class="account-list">
              <div class="account-row">
                <p class="account-key">设备总数</p>
                <p class="account-value">${escapeHtml(String(devices.length))}</p>
              </div>
              <div class="account-row">
                <p class="account-key">同局域网</p>
                <p class="account-value">${escapeHtml(String(sameLanCount))}</p>
              </div>
            </div>
          </section>
        </div>
      </div>

      <section class="panel-card">
        <div class="panel-header">
          <div>
            <h3>所有设备</h3>
            <p>包含登录状态、IP 与最近登录时间；若存在互传地址可直接打开。</p>
          </div>
        </div>
        <ul class="device-list">${buildAllDeviceItems(devices)}</ul>
      </section>
    </section>
  `;
}

function renderReadingView(snapshot) {
  const readingInfo = snapshot?.calendar_metrics?.reading_info || {};
  const readTimeWeek = snapshot?.calendar_metrics?.read_time_week || {};
  const dayReadToday = snapshot?.calendar_metrics?.day_read_today || {};

  return `
    <section class="view">
      <div class="reading-grid">
        <div class="stack">
          <section class="panel-card">
            <div class="panel-header">
              <div>
                <h3>核心指标</h3>
                <p>以今日阅读和本周时长为主，保留累计阅读与完成量。</p>
              </div>
              <div class="section-actions">
                <button class="button button-tertiary button-xs" data-action="refresh-view" type="button">刷新</button>
              </div>
            </div>
            <div class="metrics-grid">
              <div class="metric-card">
                <p class="metric-label">今日阅读数</p>
                <p class="metric-value">${escapeHtml(numberText(computeTodayReadCount(dayReadToday)))}</p>
              </div>
              <div class="metric-card">
                <p class="metric-label">本周时长</p>
                <p class="metric-value">${escapeHtml(durationText(computeWeekTotalTime(readTimeWeek)))}</p>
              </div>
              <div class="metric-card">
                <p class="metric-label">累计阅读</p>
                <p class="metric-value">${escapeHtml(numberText(readingInfo.read))}</p>
              </div>
              <div class="metric-card">
                <p class="metric-label">累计完成</p>
                <p class="metric-value">${escapeHtml(numberText(readingInfo.finished))}</p>
              </div>
            </div>
          </section>
        </div>

        <div class="stack">
          <section class="panel-card soft">
            <div class="panel-header">
              <div>
                <h3>阅读摘要</h3>
                <p>维持当前数据语义，不引入新的计算逻辑。</p>
              </div>
            </div>
            <div class="account-list">
              <div class="account-row">
                <p class="account-key">今日阅读</p>
                <p class="account-value">${escapeHtml(`${computeTodayReadCount(dayReadToday)} 本`)}</p>
              </div>
              <div class="account-row">
                <p class="account-key">本周时长</p>
                <p class="account-value">${escapeHtml(durationText(computeWeekTotalTime(readTimeWeek)))}</p>
              </div>
              <div class="account-row">
                <p class="account-key">阅读总量</p>
                <p class="account-value">${escapeHtml(numberText(readingInfo.read))}</p>
              </div>
            </div>
          </section>
        </div>
      </div>
    </section>
  `;
}

function renderAccountView(snapshot) {
  const auth = snapshot?.auth || {};
  const profile = snapshot?.profile || {};
  const storage = snapshot?.storage || {};
  const authUi = authPresentation(auth);

  return `
    <section class="view">
      <div class="account-grid">
        <div class="stack">
          <section class="panel-card">
            <div class="panel-header">
              <div>
                <h3>授权状态</h3>
                <p>当前授权仍通过默认浏览器桥接，主界面只展示状态与入口。</p>
              </div>
              <div class="section-actions">
                <button class="button button-primary button-xs" data-action="login" type="button">${escapeHtml(authUi.actionText)}</button>
              </div>
            </div>
            <div class="account-list">
              <div class="account-row">
                <p class="account-key">状态</p>
                <p class="account-value ${auth.authorized ? "success-text" : "danger-text"}">${escapeHtml(
                  auth.authorized ? "已授权" : "未授权"
                )}</p>
              </div>
              <div class="account-row">
                <p class="account-key">来源</p>
                <p class="account-value">${escapeHtml(auth.source || "-")}</p>
              </div>
              <div class="account-row">
                <p class="account-key">说明</p>
                <p class="account-value">${escapeHtml(auth.message || "-")}</p>
              </div>
            </div>
          </section>

          <section class="panel-card soft">
            <div class="panel-header">
              <div>
                <h3>账户资料</h3>
                <p>展示当前桌面端已同步到的基础用户信息。</p>
              </div>
            </div>
            <div class="account-list">
              <div class="account-row">
                <p class="account-key">昵称</p>
                <p class="account-value">${escapeHtml(profile.nickname || "未获取到用户名")}</p>
              </div>
              <div class="account-row">
                <p class="account-key">UID</p>
                <p class="account-value">${escapeHtml(profile.uid || "-")}</p>
              </div>
            </div>
          </section>
        </div>

        <div class="stack">
          <section class="panel-card">
            <div class="panel-header">
              <div>
                <h3>云空间</h3>
                <p>沿用当前后端返回的空间占用与上限数据。</p>
              </div>
            </div>
            <div class="upload-progress-block">
              <p class="summary-value">${escapeHtml(
                storage.used != null || storage.limit != null
                  ? `${bytesToText(storage.used)} / ${bytesToText(storage.limit)}`
                  : "未知"
              )}</p>
              <div class="progress-track"><div class="progress-fill" style="width:${Math.max(0, Math.min(100, Number(storage.percent || 0))).toFixed(2)}%"></div></div>
              <p class="upload-metrics-text">${escapeHtml(`使用率 ${Number(storage.percent || 0).toFixed(2)}%`)}</p>
            </div>
          </section>
        </div>
      </div>
    </section>
  `;
}

function renderZoteroView() {
  const status = state.zotero.status;
  const note = formatZoteroStateNote(status);
  const connected = status?.state === "connected";
  const allItems = Array.isArray(state.zotero.items) ? state.zotero.items : [];
  const filteredItems = getFilteredZoteroItems(allItems, state.zotero.filterText);

  return `
    <section class="view zotero-view">
      <div class="zotero-grid">
        <section class="panel-card soft zotero-panel-card zotero-workflow-card">
          <div class="panel-header">
            <div>
              <h3>附件工作流</h3>
              <p>按最近修改时间展示个人库文献与附件，统一使用“推送”动作。</p>
            </div>
            <div class="section-actions">
              <button class="button button-tertiary button-xs" data-action="zotero-refresh-items" type="button" ${connected ? "" : "disabled"} ${state.zotero.loadingItems ? "disabled" : ""}>刷新列表</button>
            </div>
          </div>
          <div class="zotero-panel-body">
            <p class="zotero-note">${escapeHtml(note)}</p>
            <div class="zotero-status-strip">${buildZoteroStatusChips(status)}</div>
            ${connected ? `
              <div class="zotero-workflow-toolbar">
                <div class="zotero-workflow-toolbar-main">
                  ${buildZoteroWorkflowStats(allItems)}
                  <p class="inline-note">这里只展示带附件的文献。第一版仅支持 stored attachments；若本地文件缺失但远端 WebDAV 可用，推送时会按需拉取。</p>
                  ${buildWorkflowPagination("zotero", state.zotero.page, state.zotero.hasMore, state.zotero.loadingItems)}
                </div>
                <label class="zotero-search-field">
                  <span>筛选条目或附件</span>
                  <input
                    data-zotero-search="items"
                    type="search"
                    value="${escapeHtml(state.zotero.filterText || "")}"
                    placeholder="搜索标题、作者、年份、附件名"
                  />
                </label>
              </div>
              <div class="zotero-workflow-list">
                ${state.zotero.loadingItems && allItems.length === 0 ? `
                  <div class="empty-card compact">
                    <h3>正在刷新 Zotero 条目</h3>
                    <p class="empty-copy">本地库读取完成后，这里会更新最近文献与附件状态。</p>
                  </div>
                ` : filteredItems.length > 0 ? buildZoteroWorkflowItems(filteredItems) : `
                  <div class="empty-card compact">
                    <h3>没有可展示的附件条目</h3>
                    <p class="empty-copy">换一个关键词试试，或清空筛选后查看所有带附件的文献。</p>
                  </div>
                `}
              </div>
            ` : `
              <div class="empty-card compact">
                <h3>等待连接完成</h3>
                <p class="empty-copy">请先回到概览页中的“Zotero 连接与补全”，完成检测、补全并验证。</p>
                <div class="section-actions">
                  <button class="button button-secondary" data-view-target="overview" type="button">前往概览补全</button>
                </div>
              </div>
            `}
          </div>
        </section>
      </div>
    </section>
  `;
}

function renderCalibreView() {
  const status = state.calibre.status;
  const note = formatCalibreStateNote(status);
  const connected = Number(status?.summary?.ready_library_count || 0) > 0;
  const allBooks = Array.isArray(state.calibre.books) ? state.calibre.books : [];
  const filteredBooks = getFilteredCalibreBooks(allBooks, state.calibre.filterText);

  return `
    <section class="view zotero-view">
      <div class="zotero-grid">
        <section class="panel-card soft zotero-panel-card zotero-workflow-card">
          <div class="panel-header">
            <div>
              <h3>书籍工作流</h3>
              <p>按最近修改时间展示书籍与格式，推送时优先使用数据库标题。</p>
            </div>
            <div class="section-actions">
              <button class="button button-tertiary button-xs" data-action="calibre-refresh-books" type="button" ${connected ? "" : "disabled"} ${state.calibre.loadingBooks ? "disabled" : ""}>刷新列表</button>
            </div>
          </div>
          <div class="zotero-panel-body" data-calibre-scroll-container="true">
            <p class="zotero-note">${escapeHtml(note)}</p>
            <div class="zotero-status-strip">${buildCalibreStatusChips(status)}</div>
            ${connected ? `
              <div class="zotero-workflow-toolbar">
                <div class="zotero-workflow-toolbar-main">
                  ${buildCalibreWorkflowStats(allBooks)}
                  <p class="inline-note">这里直接读取 Calibre 的 metadata.db。即使库目录和文件名是拼音，推送名称也会改成数据库里的原始书名。</p>
                  ${buildWorkflowPagination("calibre", state.calibre.page, state.calibre.hasMore, state.calibre.loadingBooks)}
                </div>
                <label class="zotero-search-field">
                  <span>筛选书籍或格式</span>
                  <input
                    data-calibre-search="books"
                    type="search"
                    value="${escapeHtml(state.calibre.filterText || "")}"
                    placeholder="搜索标题、作者、年份、格式"
                  />
                </label>
              </div>
              <div class="zotero-workflow-list">
                ${state.calibre.loadingBooks && allBooks.length === 0 ? `
                  <div class="empty-card compact">
                    <h3>正在刷新 Calibre 书籍</h3>
                    <p class="empty-copy">本地书库读取完成后，这里会更新最近书籍与格式状态。</p>
                  </div>
                ` : filteredBooks.length > 0 ? buildCalibreWorkflowBooks(filteredBooks) : `
                  <div class="empty-card compact">
                    <h3>没有匹配的书籍</h3>
                    <p class="empty-copy">试试其他关键词，或清空筛选后查看最近书籍。</p>
                  </div>
                `}
              </div>
            ` : `
              <div class="empty-card compact">
                <h3>等待连接完成</h3>
                <p class="empty-copy">请先在概览页完成 Calibre 书库目录配置。</p>
                <div class="section-actions">
                  <button class="button button-secondary" data-view-target="overview" type="button">前往概览配置</button>
                </div>
              </div>
            `}
          </div>
        </section>
      </div>
    </section>
  `;
}

function renderCurrentView() {
  const root = contentRoot();
  if (!root) return;
  if (state.activeView === "calibre") {
    saveCalibreScrollPositionFromDom();
  }
  const snapshot = state.snapshot;
  if (!snapshot && state.activeView !== "zotero" && state.activeView !== "calibre") {
    root.innerHTML = `
      <section class="view">
        <div class="empty-card">
          <h3>正在准备主界面</h3>
          <p class="empty-copy">桌面端正在加载账户、设备和互动文件快照。</p>
        </div>
      </section>
    `;
    return;
  }

  let html = "";
  if (state.activeView === "push") {
    html = renderPushView(snapshot);
  } else if (state.activeView === "devices") {
    html = renderDevicesView(snapshot);
  } else if (state.activeView === "reading") {
    html = renderReadingView(snapshot);
  } else if (state.activeView === "zotero") {
    html = renderZoteroView();
  } else if (state.activeView === "calibre") {
    html = renderCalibreView();
  } else {
    html = renderOverview(snapshot);
  }
  root.innerHTML = html;
  if (state.activeView === "calibre") {
    restoreCalibreScrollPosition();
  }
}

function renderSnapshot(snapshot) {
  state.snapshot = snapshot;
  renderSidebarAuth(snapshot);
  renderToolbar();
  renderCurrentView();

  if (snapshot?.auth?.authorized && state.authTimer) {
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
    renderErrorView("当前环境不支持 Tauri invoke");
    return;
  }
  state.loading = true;
  try {
    const command = force ? "dashboard_refresh" : "dashboard_snapshot";
    const snapshot = await invokeWithTimeout(command);
    renderSnapshot(snapshot);
  } catch (err) {
    if (!state.snapshot) {
      renderErrorView(`加载失败: ${String(err)}`);
    }
  } finally {
    state.loading = false;
    if (state.pendingForce) {
      state.pendingForce = false;
      setTimeout(() => loadSnapshot(true), 50);
    }
  }
}

function beginLoginAuthorization() {
  $("sidebar-profile-subtitle").textContent = "默认浏览器已打开登录方式选择页，请完成授权后返回桌面端。";
  invokeWithTimeout("dashboard_login_authorize", {}, 6000)
    .then(() => {
      if (state.authTimer) clearInterval(state.authTimer);
      state.authTimerTicks = 0;
      state.authTimer = setInterval(() => {
        state.authTimerTicks += 1;
        loadSnapshot(true);
        if (state.authTimerTicks >= 90 && state.authTimer) {
          clearInterval(state.authTimer);
          state.authTimer = null;
        }
      }, 1200);
    })
    .catch((err) => {
      $("sidebar-profile-subtitle").textContent = `登录入口失败: ${String(err)}`;
    });
}

async function triggerUpload() {
  const uploadButtons = document.querySelectorAll('[data-action="upload"], #sidebar-upload-btn');
  uploadButtons.forEach((button) => {
    if (button instanceof HTMLButtonElement) {
      button.disabled = true;
    }
  });

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
    $("sidebar-upload-status").textContent = `上传入口失败: ${String(err)}`;
    scheduleSnapshotRefresh([300], true);
  } finally {
    setTimeout(() => {
      uploadButtons.forEach((button) => {
        if (button instanceof HTMLButtonElement) {
          button.disabled = false;
        }
      });
    }, 1200);
  }
}

function removePushItemLocally(id) {
  if (!state.snapshot?.push_queue) return;
  state.snapshot.push_queue = state.snapshot.push_queue.filter((item) => item.id !== id);
  renderCurrentView();
}

function transferReasonText(reasonRaw) {
  if (reasonRaw === "share_socket_mac") return "Share WebSocket(MAC匹配)";
  if (reasonRaw === "share_socket_single_fallback") return "Share WebSocket(单设备回退)";
  if (reasonRaw === "single_device_online_fallback") return "单设备在线推断";
  if (reasonRaw === "mac_arp") return "MAC-ARP命中";
  if (reasonRaw === "same_subnet") return "同网段IP命中";
  return reasonRaw || "-";
}

async function openTransferFromButton(button) {
  const host = button.dataset.host || "-";
  const model = button.dataset.model || "BOOX 设备";
  const status = button.dataset.status || "-";
  const ip = button.dataset.ip || "-";
  const mac = button.dataset.mac || "-";
  const deviceId = button.dataset.deviceId || "-";
  const reason = transferReasonText(button.dataset.reason || "-");

  if (host !== "-") {
    try {
      await invokeWithTimeout("dashboard_open_transfer_host", { host }, 6000);
      return;
    } catch (err) {
      $("sidebar-upload-status").textContent = `打开设备地址失败: ${String(err)}`;
    }
  }

  window.alert(
    `设备: ${model}\n状态: ${status}\n互传地址: ${host}\n局域网IP: ${ip}\nMAC: ${mac}\n设备ID: ${deviceId}\n识别来源: ${reason}`
  );
}

async function loadZoteroStatus(forceItems = false) {
  state.zotero.loadingStatus = true;
  try {
    const status = await invokeWithTimeout("zotero_status", {}, 10000);
    state.zotero.status = status;
    syncZoteroFormFromStatus(status);
    if (status?.state !== "connected") {
      state.zotero.items = [];
      state.zotero.hasMore = false;
      state.zotero.page = 0;
    }
    renderToolbar();
    renderCurrentView();
    if (status?.state === "connected" && !state.zotero.loadingItems && (forceItems || state.activeView === "zotero")) {
      await loadZoteroItems();
    }
  } finally {
    state.zotero.loadingStatus = false;
  }
}

function canAutoConnectZotero(status) {
  return !!status &&
    status.state !== "connected" &&
    !state.zotero.loadingStatus &&
    !state.zotero.loadingItems &&
    state.zotero.phase !== "validating" &&
    Array.isArray(status.missing_fields) &&
    status.missing_fields.length === 0;
}

async function autoConnectZoteroIfReady(status, reason = "自动连接") {
  if (!canAutoConnectZotero(status)) return false;
  state.zotero.phaseError = `${reason}：检测到 Zotero 配置完整，正在自动连接...`;
  renderCurrentView();
  await saveZoteroConfig({ auto: true });
  return true;
}

async function loadZoteroItems(page = state.zotero.page) {
  const nextPage = Math.max(0, Number(page) || 0);
  const search = String(state.zotero.filterText || "").trim();
  state.zotero.loadingItems = true;
  state.zotero.page = nextPage;
  renderCurrentView();
  try {
    const result = await invokeWithTimeout(
      "zotero_list_recent_items",
      {
        limit: LIBRARY_PAGE_SIZE + 1,
        offset: nextPage * LIBRARY_PAGE_SIZE,
        search: search || null
      },
      30000
    );
    const list = Array.isArray(result) ? result : [];
    state.zotero.hasMore = list.length > LIBRARY_PAGE_SIZE;
    state.zotero.items = list.slice(0, LIBRARY_PAGE_SIZE);
    if (nextPage > 0 && state.zotero.items.length === 0) {
      await loadZoteroItems(nextPage - 1);
      return;
    }
  } catch (err) {
    state.zotero.hasMore = false;
    state.zotero.phaseError = `加载 Zotero 条目失败: ${String(err)}`;
  } finally {
    state.zotero.loadingItems = false;
    renderCurrentView();
  }
}

async function ensureZoteroData() {
  if (!state.zotero.status) {
    await loadZoteroStatus(true);
    return;
  }
  if (await autoConnectZoteroIfReady(state.zotero.status, "进入 Zotero 页面")) {
    return;
  }
  if (state.zotero.status?.state === "connected" && state.zotero.items.length === 0 && !state.zotero.loadingItems) {
    await loadZoteroItems();
    return;
  }
  renderCurrentView();
}

async function detectZoteroConfig() {
  state.zotero.phase = "detecting";
  state.zotero.phaseError = "";
  renderToolbar();
  renderCurrentView();
  try {
    const detection = await invokeWithTimeout("zotero_detect_config", {}, 12000);
    state.zotero.detected = detection;
    state.zotero.status = await invokeWithTimeout("zotero_status", {}, 10000);
    syncZoteroFormFromStatus(state.zotero.status, { preservePassword: true });
    const issues = Array.isArray(detection?.issues) && detection.issues.length > 0
      ? `检测完成: ${detection.issues.join("，")}`
      : "检测完成，请确认并补全缺失项。";
    state.zotero.phaseError = issues;
    if (await autoConnectZoteroIfReady(state.zotero.status, "自动检测完成")) {
      return;
    }
    if (state.zotero.status?.state === "connected" && state.activeView === "zotero") {
      await loadZoteroItems();
    }
  } catch (err) {
    state.zotero.phaseError = `自动检测失败: ${String(err)}`;
  } finally {
    state.zotero.phase = "idle";
    renderToolbar();
    renderCurrentView();
  }
}

async function pickZoteroDirectory(kind) {
  state.zotero.phaseError = "";
  renderCurrentView();
  try {
    const command = kind === "profile" ? "zotero_pick_profile_dir" : "zotero_pick_data_dir";
    state.zotero.detected = await invokeWithTimeout(command, {}, 30000);
    const status = await invokeWithTimeout("zotero_status", {}, 10000);
    state.zotero.status = status;
    syncZoteroFormFromStatus(status, { preservePassword: true });
    if (await autoConnectZoteroIfReady(status, "目录补全完成")) {
      return;
    }
    if (status?.state === "connected" && state.activeView === "zotero") {
      await loadZoteroItems();
    } else {
      renderCurrentView();
    }
  } catch (err) {
    state.zotero.phaseError = String(err);
    renderCurrentView();
  }
}

async function bootstrapZoteroAutoDetect() {
  try {
    await loadZoteroStatus();
    const status = state.zotero.status;
    const summary = status?.summary || {};
    if (await autoConnectZoteroIfReady(status, "启动时")) {
      return;
    }
    const shouldDetect =
      !status ||
      status.state === "undetected" ||
      (!summary.profile_dir && !summary.data_dir && !summary.webdav_url);
    if (shouldDetect) {
      await detectZoteroConfig();
    }
  } catch (err) {
    state.zotero.phaseError = `启动时检测 Zotero 失败: ${String(err)}`;
    if (state.activeView === "zotero") {
      renderCurrentView();
    }
  }
}

async function saveZoteroConfig(options = {}) {
  const auto = options.auto === true;
  const loadItemsAfterConnect = options.loadItemsAfterConnect === true || state.activeView === "zotero";
  state.zotero.phase = "validating";
  state.zotero.phaseError = "";
  setUploadStatusOverride(auto ? "上传进度: 正在自动连接 Zotero..." : "上传进度: 正在保存并验证 Zotero 配置...", 10000);
  renderToolbar();
  renderCurrentView();
  try {
    const status = await invokeWithTimeout(
      "zotero_save_and_validate",
      {
        input: {
          profile_dir: state.zotero.form.profileDir || null,
          data_dir: state.zotero.form.dataDir || null,
          webdav_url: state.zotero.form.webdavUrl || null,
          webdav_username: state.zotero.form.webdavUsername || null,
          webdav_password: state.zotero.form.webdavPassword || null
        }
      },
      20000
    );
    state.zotero.status = status;
    syncZoteroFormFromStatus(status, { preservePassword: false });
    if (status?.state === "connected") {
      state.zotero.phaseError = auto ? "Zotero 已自动连接。" : "Zotero 连接已就绪。";
      setUploadStatusOverride(auto ? "上传进度: Zotero 已自动连接" : "上传进度: Zotero 配置验证完成", 4000);
      if (loadItemsAfterConnect) {
        await loadZoteroItems();
      }
    } else {
      state.zotero.phaseError = formatZoteroStateNote(status);
    }
  } catch (err) {
    state.zotero.phaseError = auto ? `自动连接失败: ${String(err)}` : `保存或验证失败: ${String(err)}`;
  } finally {
    state.zotero.phase = "idle";
    renderToolbar();
    renderCurrentView();
  }
}

async function pushZoteroAttachment(attachmentId) {
  state.zotero.pushingAttachmentId = attachmentId;
  state.zotero.phaseError = "";
  setUploadStatusOverride("上传进度: 正在提交 Zotero 附件推送...", 10000);
  renderCurrentView();
  try {
    const snapshot = await invokeWithTimeout(
      "zotero_push_attachment",
      { attachmentItemId: Number(attachmentId) },
      120000
    );
    if (snapshot) renderSnapshot(snapshot);
    await loadZoteroItems();
    setUploadStatusOverride("上传进度: Zotero 附件推送完成", 5000);
  } catch (err) {
    state.zotero.phaseError = `推送失败: ${String(err)}`;
    $("sidebar-upload-status").textContent = `推送失败: ${String(err)}`;
    renderCurrentView();
  } finally {
    state.zotero.pushingAttachmentId = null;
    renderCurrentView();
  }
}

async function loadCalibreStatus(forceBooks = false) {
  state.calibre.loadingStatus = true;
  renderToolbar();
  renderCurrentView();
  try {
    const status = await invokeWithTimeout("calibre_status", {}, 10000);
    state.calibre.status = status;
    syncCalibreFormFromStatus(status);
    if (Number(status?.summary?.ready_library_count || 0) === 0) {
      state.calibre.books = [];
      state.calibre.hasMore = false;
      state.calibre.page = 0;
    }
    if (forceBooks && Number(status?.summary?.ready_library_count || 0) > 0 && !state.calibre.loadingBooks) {
      await loadCalibreBooks();
    }
  } finally {
    state.calibre.loadingStatus = false;
    renderToolbar();
    renderCurrentView();
  }
}

async function loadCalibreBooks(page = state.calibre.page) {
  const nextPage = Math.max(0, Number(page) || 0);
  const search = String(state.calibre.filterText || "").trim();
  state.calibre.loadingBooks = true;
  state.calibre.page = nextPage;
  renderCurrentView();
  try {
    const result = await invokeWithTimeout(
      "calibre_list_recent_books",
      {
        limit: LIBRARY_PAGE_SIZE + 1,
        offset: nextPage * LIBRARY_PAGE_SIZE,
        search: search || null
      },
      30000
    );
    const list = Array.isArray(result) ? result : [];
    state.calibre.hasMore = list.length > LIBRARY_PAGE_SIZE;
    state.calibre.books = list.slice(0, LIBRARY_PAGE_SIZE);
    if (nextPage > 0 && state.calibre.books.length === 0) {
      setCalibreScrollTop(0);
      await loadCalibreBooks(nextPage - 1);
      return;
    }
  } catch (err) {
    state.calibre.hasMore = false;
    state.calibre.phaseError = `加载 Calibre 书籍失败: ${String(err)}`;
  } finally {
    state.calibre.loadingBooks = false;
    renderCurrentView();
  }
}

function scheduleZoteroSearch() {
  if (state.zoteroSearchTimer) {
    clearTimeout(state.zoteroSearchTimer);
    state.zoteroSearchTimer = null;
  }
  state.zoteroSearchTimer = setTimeout(() => {
    state.zoteroSearchTimer = null;
    loadZoteroItems(0);
  }, 220);
}

function scheduleCalibreSearch() {
  if (state.calibreSearchTimer) {
    clearTimeout(state.calibreSearchTimer);
    state.calibreSearchTimer = null;
  }
  state.calibreSearchTimer = setTimeout(() => {
    state.calibreSearchTimer = null;
    setCalibreScrollTop(0);
    loadCalibreBooks(0);
  }, 220);
}

async function ensureCalibreData() {
  if (!state.calibre.status) {
    await loadCalibreStatus(true);
    return;
  }
  if (
    (state.calibre.status?.state === "connected" || Number(state.calibre.status?.summary?.ready_library_count || 0) > 0) &&
    state.calibre.books.length === 0 &&
    !state.calibre.loadingBooks
  ) {
    await loadCalibreBooks();
    return;
  }
  renderToolbar();
  renderCurrentView();
}

async function detectCalibreLibrary() {
  state.calibre.phase = "detecting";
  state.calibre.phaseError = "";
  renderToolbar();
  renderCurrentView();
  try {
    const status = await invokeWithTimeout("calibre_detect_library", {}, 12000);
    state.calibre.status = status;
    syncCalibreFormFromStatus(status);
    state.calibre.phaseError = Number(status?.summary?.ready_library_count || 0) > 0
      ? "已检测到默认 Calibre 书库。"
      : formatCalibreStateNote(status);
    if (Number(status?.summary?.ready_library_count || 0) > 0 && state.activeView === "calibre") {
      await loadCalibreBooks();
    } else {
      state.calibre.books = [];
    }
  } catch (err) {
    state.calibre.phaseError = `自动检测失败: ${String(err)}`;
  } finally {
    state.calibre.phase = "idle";
    renderToolbar();
    renderCurrentView();
  }
}

async function refreshCalibreLibraries() {
  state.calibre.loadingStatus = true;
  state.calibre.phaseError = "";
  renderToolbar();
  renderCurrentView();
  try {
    const status = await invokeWithTimeout("calibre_refresh_libraries", {}, 12000);
    state.calibre.status = status;
    syncCalibreFormFromStatus(status);
    if (Number(status?.summary?.ready_library_count || 0) > 0 && state.activeView === "calibre") {
      await loadCalibreBooks();
      state.calibre.phaseError = status?.last_error || "书库检查完成。";
    } else {
      state.calibre.books = [];
      state.calibre.phaseError = status?.last_error || formatCalibreStateNote(status);
    }
  } catch (err) {
    state.calibre.phaseError = `检查失败: ${String(err)}`;
  } finally {
    state.calibre.loadingStatus = false;
    renderToolbar();
    renderCurrentView();
  }
}

async function pickCalibreLibraryDirectory() {
  state.calibre.phaseError = "";
  renderCurrentView();
  try {
    const status = await invokeWithTimeout("calibre_pick_library_dir", {}, 30000);
    state.calibre.status = status;
    syncCalibreFormFromStatus(status);
    if (Number(status?.summary?.ready_library_count || 0) > 0 && state.activeView === "calibre") {
      await loadCalibreBooks();
    } else {
      state.calibre.books = [];
      renderCurrentView();
    }
  } catch (err) {
    state.calibre.phaseError = String(err);
    renderCurrentView();
  }
}

async function saveCalibreConfig() {
  state.calibre.phase = "validating";
  state.calibre.phaseError = "";
  setUploadStatusOverride("上传进度: 正在保存 Calibre 书库配置...", 8000);
  renderToolbar();
  renderCurrentView();
  try {
    const libraryDirs = parseCalibreLibraryDirs(state.calibre.form.libraryDirsText);
    const status = await invokeWithTimeout(
      "calibre_save_library_dir",
      {
        input: {
          library_dirs: libraryDirs
        }
      },
      15000
    );
    state.calibre.status = status;
    syncCalibreFormFromStatus(status);
    if (Number(status?.summary?.ready_library_count || 0) > 0) {
      state.calibre.phaseError = "Calibre 书库已连接。";
      setUploadStatusOverride("上传进度: Calibre 书库配置完成", 4000);
      if (state.activeView === "calibre") {
        await loadCalibreBooks();
      }
    } else if (libraryDirs.length === 0) {
      state.calibre.books = [];
      state.calibre.hasMore = false;
      state.calibre.page = 0;
      state.calibre.phaseError = "已清空 Calibre 书库目录。";
      setUploadStatusOverride("上传进度: 已移除所有 Calibre 书库目录", 4000);
    } else {
      state.calibre.books = [];
      state.calibre.phaseError = formatCalibreStateNote(status);
    }
  } catch (err) {
    state.calibre.phaseError = `保存失败: ${String(err)}`;
  } finally {
    state.calibre.phase = "idle";
    renderToolbar();
    renderCurrentView();
  }
}

async function removeCalibreLibraryDirectory(libraryDir) {
  const target = String(libraryDir || "").trim();
  if (!target) return;
  const ok = window.confirm(`确定移除这个 Calibre 书库目录吗？\n\n${target}`);
  if (!ok) return;
  const nextDirs = parseCalibreLibraryDirs(state.calibre.form.libraryDirsText).filter((dir) => dir !== target);
  state.calibre.form.libraryDirsText = nextDirs.join("\n");
  await saveCalibreConfig();
}

async function pushCalibreFormat(libraryDir, dataId) {
  const formatKey = `${String(libraryDir || "")}::${String(dataId || "")}`;
  state.calibre.pushingFormatKey = formatKey;
  state.calibre.phaseError = "";
  setUploadStatusOverride("上传进度: 正在提交 Calibre 书籍推送...", 10000);
  renderCurrentView();
  try {
    const snapshot = await invokeWithTimeout(
      "calibre_push_format",
      { libraryDir: String(libraryDir || ""), dataId: Number(dataId) },
      120000
    );
    if (snapshot) renderSnapshot(snapshot);
    await loadCalibreBooks();
    setUploadStatusOverride("上传进度: Calibre 书籍推送完成", 5000);
  } catch (err) {
    state.calibre.phaseError = `推送失败: ${String(err)}`;
    $("sidebar-upload-status").textContent = `推送失败: ${String(err)}`;
    renderCurrentView();
  } finally {
    state.calibre.pushingFormatKey = null;
    renderCurrentView();
  }
}

async function bootstrapCalibreAutoDetect() {
  try {
    await loadCalibreStatus();
    const summary = state.calibre.status?.summary || {};
    if (!Number(summary.total_library_count || 0) && !Number(summary.ready_library_count || 0)) {
      await detectCalibreLibrary();
    }
  } catch (err) {
    state.calibre.phaseError = `启动时检测 Calibre 失败: ${String(err)}`;
    if (state.activeView === "calibre") {
      renderCurrentView();
    }
  }
}

function bindActions() {
  document.addEventListener("click", async (event) => {
    const navButton = event.target?.closest(".nav-item[data-view]");
    if (navButton instanceof HTMLButtonElement) {
      setActiveView(navButton.dataset.view || "overview");
      if ((navButton.dataset.view || "overview") === "zotero") {
        await ensureZoteroData();
      } else if ((navButton.dataset.view || "overview") === "calibre") {
        await ensureCalibreData();
      }
      return;
    }

    const goViewButton = event.target?.closest("[data-view-target]");
    if (goViewButton instanceof HTMLButtonElement) {
      setActiveView(goViewButton.dataset.viewTarget || "overview");
      if ((goViewButton.dataset.viewTarget || "overview") === "zotero") {
        await ensureZoteroData();
      } else if ((goViewButton.dataset.viewTarget || "overview") === "calibre") {
        await ensureCalibreData();
      }
      return;
    }

    const calibreBook = event.target?.closest("[data-calibre-book-key]");
    if (calibreBook instanceof HTMLElement) {
      const nextBookKey = calibreBook.dataset.calibreBookKey || "";
      const changed = state.calibre.activeBookKey !== nextBookKey;
      setCalibreActiveBookKey(nextBookKey);
      if (changed) {
        document
          .querySelectorAll("[data-calibre-book-key].is-active")
          .forEach((element) => element.classList.remove("is-active"));
        calibreBook.classList.add("is-active");
      }
    }

    const actionButton = event.target?.closest("[data-action]");
    if (actionButton instanceof HTMLButtonElement) {
      const action = actionButton.dataset.action;
      if (action === "login") {
        beginLoginAuthorization();
        return;
      }
      if (action === "upload") {
        await triggerUpload();
        return;
      }
      if (action === "refresh-view") {
        await loadSnapshot(true);
        return;
      }
      if (action === "resend") {
        const row = actionButton.closest("[data-push-id]");
        const id = row instanceof HTMLElement ? row.dataset.pushId : "";
        if (!id) return;
        const originalText = actionButton.textContent || "推送";
        actionButton.disabled = true;
        actionButton.textContent = "推送中...";
        try {
          setUploadStatusOverride("上传进度: 正在重新推送文件到设备，请稍候...", 12000);
          const snapshot = await invokeWithTimeout("dashboard_push_resend", { id }, 20000);
          if (snapshot) renderSnapshot(snapshot);
          setUploadStatusOverride("上传进度: 已提交重新推送，正在刷新状态...", 6000);
          scheduleSnapshotRefresh([500, 1800, 4000], true);
        } catch (err) {
          clearUploadStatusOverride();
          $("sidebar-upload-status").textContent = `操作失败: ${String(err)}`;
        } finally {
          actionButton.disabled = false;
          actionButton.textContent = originalText;
          setTimeout(() => loadSnapshot(false), 400);
        }
        return;
      }
      if (action === "delete") {
        const row = actionButton.closest("[data-push-id]");
        const id = row instanceof HTMLElement ? row.dataset.pushId : "";
        if (!id) return;
        const ok = window.confirm("确定删除这条推送记录吗？");
        if (!ok) return;
        const originalText = actionButton.textContent || "删除";
        actionButton.disabled = true;
        actionButton.textContent = "删除中...";
        removePushItemLocally(id);
        try {
          setUploadStatusOverride("上传进度: 正在删除推送记录...", 6000);
          const snapshot = await invokeWithTimeout("dashboard_push_delete", { id }, 12000);
          if (snapshot) renderSnapshot(snapshot);
          setUploadStatusOverride("上传进度: 推送记录已删除", 3000);
        } catch (err) {
          clearUploadStatusOverride();
          $("sidebar-upload-status").textContent = `操作失败: ${String(err)}`;
          setTimeout(() => loadSnapshot(true), 120);
        } finally {
          actionButton.disabled = false;
          actionButton.textContent = originalText;
        }
        return;
      }
      if (action === "open-transfer") {
        await openTransferFromButton(actionButton);
        return;
      }
      if (action === "zotero-detect" || action === "zotero-redetect") {
        await detectZoteroConfig();
        return;
      }
      if (action === "zotero-pick-profile") {
        await pickZoteroDirectory("profile");
        return;
      }
      if (action === "zotero-pick-data") {
        await pickZoteroDirectory("data");
        return;
      }
      if (action === "zotero-save") {
        await saveZoteroConfig();
        return;
      }
      if (action === "zotero-refresh-items") {
        await loadZoteroItems();
        return;
      }
      if (action === "zotero-page-prev") {
        await loadZoteroItems(Math.max(0, state.zotero.page - 1));
        return;
      }
      if (action === "zotero-page-next") {
        await loadZoteroItems(state.zotero.page + 1);
        return;
      }
      if (action === "zotero-push-attachment") {
        await pushZoteroAttachment(actionButton.dataset.attachmentId || "");
        return;
      }
      if (action === "calibre-detect") {
        await detectCalibreLibrary();
        return;
      }
      if (action === "calibre-refresh-libraries") {
        await refreshCalibreLibraries();
        return;
      }
      if (action === "calibre-toggle-broken-only") {
        setCalibreShowBrokenOnly(!state.calibre.showBrokenOnly);
        renderCurrentView();
        return;
      }
      if (action === "calibre-pick-library") {
        await pickCalibreLibraryDirectory();
        return;
      }
      if (action === "calibre-save") {
        await saveCalibreConfig();
        return;
      }
      if (action === "calibre-remove-library") {
        await removeCalibreLibraryDirectory(actionButton.dataset.libraryDir || "");
        return;
      }
      if (action === "calibre-refresh-books") {
        await loadCalibreBooks();
        return;
      }
      if (action === "calibre-page-prev") {
        setCalibreScrollTop(0);
        await loadCalibreBooks(Math.max(0, state.calibre.page - 1));
        return;
      }
      if (action === "calibre-page-next") {
        setCalibreScrollTop(0);
        await loadCalibreBooks(state.calibre.page + 1);
        return;
      }
      if (action === "calibre-push-format") {
        await pushCalibreFormat(actionButton.dataset.libraryDir || "", actionButton.dataset.dataId || "");
        return;
      }
    }
  });

  document.addEventListener("input", (event) => {
    const input = event.target;
    if (!(input instanceof HTMLInputElement) && !(input instanceof HTMLTextAreaElement)) return;
    const zoteroSearch = input instanceof HTMLInputElement ? input.dataset.zoteroSearch : "";
    if (zoteroSearch === "items") {
      const selectionStart = input.selectionStart;
      const selectionEnd = input.selectionEnd;
      state.zotero.filterText = input.value;
      state.zotero.page = 0;
      renderCurrentView();
      scheduleZoteroSearch();
      const nextInput = document.querySelector('input[data-zotero-search="items"]');
      if (nextInput instanceof HTMLInputElement) {
        nextInput.focus();
        if (Number.isInteger(selectionStart) && Number.isInteger(selectionEnd)) {
          nextInput.setSelectionRange(selectionStart, selectionEnd);
        }
      }
      return;
    }
    const calibreSearch = input instanceof HTMLInputElement ? input.dataset.calibreSearch : "";
    if (calibreSearch === "books") {
      const selectionStart = input.selectionStart;
      const selectionEnd = input.selectionEnd;
      setCalibreFilterText(input.value);
      state.calibre.page = 0;
      renderCurrentView();
      scheduleCalibreSearch();
      const nextInput = document.querySelector('input[data-calibre-search="books"]');
      if (nextInput instanceof HTMLInputElement) {
        nextInput.focus();
        if (Number.isInteger(selectionStart) && Number.isInteger(selectionEnd)) {
          nextInput.setSelectionRange(selectionStart, selectionEnd);
        }
      }
      return;
    }
    const field = input.dataset.zoteroField;
    if (field) {
      state.zotero.form[field] = input.value;
      return;
    }
    const calibreField = input.dataset.calibreField;
    if (calibreField) {
      state.calibre.form[calibreField] = input.value;
    }
  });

  document.addEventListener("scroll", (event) => {
    const target = event.target;
    if (!(target instanceof HTMLElement)) return;
    if (target.dataset.calibreScrollContainer === "true") {
      setCalibreScrollTop(target.scrollTop);
    }
  }, true);

  $("sidebar-login-btn")?.addEventListener("click", beginLoginAuthorization);
  $("sidebar-upload-btn")?.addEventListener("click", async () => {
    await triggerUpload();
  });
  $("toolbar-refresh-btn")?.addEventListener("click", async () => {
    await loadSnapshot(true);
  });

  $("refresh-apply-btn")?.addEventListener("click", () => {
    const input = $("refresh-interval-minutes");
    if (!(input instanceof HTMLInputElement)) return;
    applyRefreshIntervalMinutes(input.value);
  });

  $("refresh-interval-minutes")?.addEventListener("keydown", (event) => {
    if (!(event instanceof KeyboardEvent) || event.key !== "Enter") return;
    const input = event.target;
    if (!(input instanceof HTMLInputElement)) return;
    applyRefreshIntervalMinutes(input.value);
  });

  $("refresh-interval-minutes")?.addEventListener("blur", (event) => {
    const target = event.target;
    if (!(target instanceof HTMLInputElement)) return;
    applyRefreshIntervalMinutes(target.value);
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
      $("sync-time").textContent = `更新于 ${timeAgoText(state.snapshot.fetched_at_ms)}`;
    }
  }, 1000);
}

document.addEventListener("DOMContentLoaded", () => {
  state.activeView = getSavedView();
  setCalibreShowBrokenOnly(getInitialCalibreShowBrokenOnly());
  setCalibreFilterText(getInitialCalibreFilterText());
  setCalibreScrollTop(getInitialCalibreScrollTop());
  setCalibreActiveBookKey(getInitialCalibreActiveBookKey());
  syncNavState();
  renderToolbar();
  renderCurrentView();
  applyRefreshIntervalMinutes(getInitialRefreshMinutes());
  bindActions();
  setTimeout(() => loadSnapshot(true), 120);
  setTimeout(() => bootstrapZoteroAutoDetect(), 180);
  setTimeout(() => bootstrapCalibreAutoDetect(), 220);
  if (state.activeView === "zotero") {
    setTimeout(() => ensureZoteroData(), 260);
  } else if (state.activeView === "calibre") {
    setTimeout(() => ensureCalibreData(), 260);
  }
  window.addEventListener("focus", () => loadSnapshot(true));
  window.addEventListener("focus", () => {
    if (state.activeView === "zotero") {
      ensureZoteroData();
    } else if (state.activeView === "calibre") {
      ensureCalibreData();
    }
  });
  window.addEventListener("beforeunload", () => {
    if (state.timer) {
      clearInterval(state.timer);
      state.timer = null;
    }
    if (state.syncTimer) {
      clearInterval(state.syncTimer);
      state.syncTimer = null;
    }
    if (state.zoteroSearchTimer) {
      clearTimeout(state.zoteroSearchTimer);
      state.zoteroSearchTimer = null;
    }
    if (state.calibreSearchTimer) {
      clearTimeout(state.calibreSearchTimer);
      state.calibreSearchTimer = null;
    }
  });
});

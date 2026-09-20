const fragment = new URLSearchParams(location.hash.slice(1));
const token = fragment.get("token") || "";
const initialWorkspace = fragment.get("workspace") || "";
function readPreference(key) {
  try { return localStorage.getItem(key); } catch { return null; }
}
function savePreference(key, value) {
  try { localStorage.setItem(key, value); } catch { /* Optional preferences never block the UI. */ }
}
const savedLanguage = readPreference("wcode.ui.language");
const savedTheme = readPreference("wcode.ui.theme");
const systemThemeQuery = window.matchMedia("(prefers-color-scheme: light)");
// Localization dictionaries and locale helpers are loaded by i18n.js.

const q = (id) => document.querySelector(id);
const els = {
  workspace: q("#workspace"),
  workspaceKicker: q("#workspaceKicker"),
  workspaceTitle: q("#workspaceTitle"),
  workspaceSubtitle: q("#workspaceSubtitle"),
  language: q("#language"),
  theme: q("#theme"),
  manage: q("#manage"),
  accessPanel: q("#accessPanel"),
  closeAccess: q("#closeAccess"),
  workspaceList: q("#workspaceList"),
  workspacePath: q("#workspacePath"),
  addWorkspace: q("#addWorkspace"),
  workspaceMessage: q("#workspaceMessage"),
  commandList: q("#commandList"),
  commandCandidate: q("#commandCandidate"),
  addCommand: q("#addCommand"),
  commandMessage: q("#commandMessage"),
  allCommandsStatus: q("#allCommandsStatus"),
  allCommandsToggle: q("#allCommandsToggle"),
  operationProgram: q("#operationProgram"),
  operationArgs: q("#operationArgs"),
  operationCwd: q("#operationCwd"),
  authorizeOperation: q("#authorizeOperation"),
  operationMessage: q("#operationMessage"),
  authorizationList: q("#authorizationList"),
  authorizationMessage: q("#authorizationMessage"),
  stats: q("#stats"),
  statusSummary: q("#statusSummary"),
  activity: q("#activity"),
  executionStatus: q("#executionStatus"),
  resourceStatus: q("#resourceStatus"),
  proofSummary: q("#proofSummary"),
  adaptiveVerification: q("#adaptiveVerification"),
  verifiedLearning: q("#verifiedLearning"),
  componentCards: q("#componentCards"),
  componentToolbar: q("#componentToolbar"),
  architectureDrilldown: q("#architectureDrilldown"),
  componentSearch: q("#componentSearch"),
  componentCount: q("#componentCount"),
  attention: q("#attention"),
  architectureBlueprint: q("#architectureBlueprint"),
  engineeringFlow: q("#engineeringFlow"),
  changeStory: q("#changeStory"),
  runtimeTopology: q("#runtimeTopology"),
  engineeringTimeline: q("#engineeringTimeline"),
  traceabilityMap: q("#traceabilityMap"),
  changeConvergenceMap: q("#changeConvergenceMap"),
  architectureGraph: q("#architectureGraph"),
  systemMapFit: q("#systemMapFit"),
  systemMapZoomOut: q("#systemMapZoomOut"),
  systemMapZoomIn: q("#systemMapZoomIn"),
  systemMapZoomValue: q("#systemMapZoomValue"),
  systemMapFull: q("#systemMapFull"),
  componentInspector: q("#componentInspector"),
  architectureLayout: q(".architecture-layout"),
  codeGraphSection: q("#codeGraphSection"),
  codeGraphSearch: q("#codeGraphSearch"),
  codeGraphSearchResults: q("#codeGraphSearchResults"),
  codeGraphSnapshot: q("#codeGraphSnapshot"),
  codeGraphDepth: q("#codeGraphDepth"),
  codeGraphFull: q("#codeGraphFull"),
  codeGraphInspectorToggle: q("#codeGraphInspectorToggle"),
  codeGraphSummary: q("#codeGraphSummary"),
  codeGraphMap: q("#codeGraphMap"),
  codeGraphInspector: q("#codeGraphInspector"),
  requirements: q("#requirements"),
  reqCount: q("#reqCount"),
  detail: q("#featureDetail"),
  search: q("#reqSearch"),
  languageQuality: q("#languageQuality"),
  qualitySummary: q("#qualitySummary"),
  codeStats: q("#codeStats"),
  revisions: q("#revisions"),
  changes: q("#changes"),
  verificationImpact: q("#verificationImpact"),
  structureSummary: q("#structureSummary"),
  fileTree: q("#fileTree"),
  fileSearch: q("#fileSearch"),
  fileSearchStatus: q("#fileSearchStatus"),
  largeFiles: q("#largeFiles"),
  auto: q("#autoRefresh"),
  refresh: q("#refresh"),
  refreshSemantic: q("#refreshSemantic"),
  syncDot: q("#syncDot"),
  syncState: q("#syncState"),
  precisionBadge: q("#precisionBadge"),
  precisionProviders: q("#precisionProviders"),
  lastUpdated: q("#lastUpdated"),
  tunnels: q("#tunnels"),
  projectNavigator: q("#projectNavigator"),
  navigatorResults: q("#navigatorResults"),
};

const state = {
  current: initialWorkspace,
  workspaceEpoch: 0,
  accessMutationEpoch: 0,
  accessRead: null,
  accessOperation: null,
  pendingSequence: 0,
  pendingApplied: 0,
  pendingValue: null,
  activityTimer: null,
  activityTickActive: false,
  projectTickActive: false,
  started: false,
  project: null,
  projectCache: new Map(),
  access: null,
  workspaceAccess: null,
  authorizations: [],
  accessLoaded: false,
  accessEpoch: 0,
  accessBusy: false,
  activitySnapshot: null,
  activityError: false,
  activityUpdated: 0,
  activityController: null,
  pollController: null,
  tunnelSnapshot: null,
  tunnelBusy: false,
  tunnelController: null,
  tunnelTimer: null,
  syncError: false,
  syncFailure: null,
  lastChecked: 0,
  workspaceTab: "architecture",
  architectureView: "blueprint",
  systemMapScale: 1,
  systemMapFit: true,
  systemMapFull: false,
  selectedSubsystem: "",
  selectedEvidenceKey: "",
  evidenceInspectorOpen: true,
  selected: "",
  selectedComponent: "",
  codeGraph: null,
  codeGraphOverview: null,
  codeGraphWorkspace: "",
  codeGraphQuery: "",
  codeGraphView: "overview",
  codeGraphMode: "all",
  codeGraphSnapshot: "",
  codeGraphDepth: 2,
  codeGraphFull: false,
  codeGraphInspectorOpen: false,
  codeGraphLoading: false,
  codeGraphError: "",
  codeGraphController: null,
  codeGraphSearchController: null,
  codeGraphSearchTimer: null,
  codeGraphSearchResults: [],
  codeGraphLayouts: new Map(),
  codeGraphViewports: new Map(),
  selectedCodeNode: "",
  filter: "all",
  architectureMode: "overlay",
  timer: null,
  language: initialLanguage(savedLanguage),
  theme: ["system", "dark", "light"].includes(savedTheme)
    ? savedTheme
    : "system",
  autoRefresh: true,
  rendered: new Map(),
  controller: null,
  requestEpoch: 0,
  inFlight: false,
  lastUpdated: 0,
  revisionKey: null,
  semanticRefreshPending: false,
};
const t = (key) => translateKey(state.language, key);
const localized = (en, zh) => state.language === "zh-CN" ? zh : en;
const unit = (value, singular, plural, zh) =>
  state.language === "zh-CN"
    ? `${num(value)} ${zh}`
    : `${num(value)} ${Number(value) === 1 ? singular : plural}`;
const statusLabel = (value) =>
  t(String(value ?? "unknown").replaceAll("_", " ").toLowerCase());
const dimensionLabel = (value) => ({
  type_check: localized("type check", "类型检查"),
  static_analysis: localized("static analysis", "静态分析"),
  runtime_canary: localized("runtime canary", "运行时金丝雀"),
  property: localized("property", "属性测试"),
  mutation: localized("mutation", "变异测试"),
  fuzz: localized("fuzz", "模糊测试"),
  format: localized("format", "格式化"),
  lint: "Lint",
  test: localized("test", "测试"),
  security: localized("security", "安全"),
  syntax: localized("syntax", "语法"),
  semantic: localized("semantic", "语义"),
}[value] || statusLabel(value));
const esc = (v) =>
  String(v ?? "—").replace(
    /[&<>"']/g,
    (c) => ({
      "&": "&amp;",
      "<": "&lt;",
      ">": "&gt;",
      '"': "&quot;",
      "'": "&#39;",
    }[c]),
  );
const num = (v) =>
  new Intl.NumberFormat(state.language === "zh-CN" ? "zh-CN" : "en").format(
    Number(v || 0),
  );
const time = (ms) =>
  ms
    ? new Date(ms).toLocaleString(state.language === "zh-CN" ? "zh-CN" : "en")
    : "—";
const pill = (label, cls = "") =>
  `<span class="pill ${cls}">${esc(label)}</span>`;
const uiIcon = (name, cls = "") => {
  const paths = {
    cube: '<path d="m12 2 9 5-9 5-9-5 9-5Z"/><path d="m3 7 9 5 9-5M12 12v10"/>',
    layers: '<path d="m12 2 9 5-9 5-9-5 9-5Z"/><path d="m3 12 9 5 9-5M3 17l9 5 9-5"/>',
    network: '<circle cx="5" cy="12" r="2.5"/><circle cx="12" cy="5" r="2.5"/><circle cx="19" cy="12" r="2.5"/><path d="m7 10 3-3m4 0 3 3M7.5 13h9"/>',
    document: '<path d="M6 2h8l4 4v16H6z"/><path d="M14 2v5h5M9 12h6M9 16h6"/>',
    calendar: '<rect x="3" y="5" width="18" height="16" rx="2"/><path d="M7 2v6M17 2v6M3 10h18"/>',
    shield: '<path d="M12 2 20 5v6c0 5-3.4 8.6-8 11-4.6-2.4-8-6-8-11V5l8-3Z"/>',
    database: '<ellipse cx="12" cy="5" rx="7" ry="3"/><path d="M5 5v7c0 1.7 3.1 3 7 3s7-1.3 7-3V5M5 12v7c0 1.7 3.1 3 7 3s7-1.3 7-3v-7"/>',
    terminal: '<path d="m4 7 5 5-5 5M11 18h9"/>',
    monitor: '<rect x="3" y="4" width="18" height="13" rx="2"/><path d="M8 21h8M12 17v4"/>',
    link: '<path d="M10 13a5 5 0 0 0 7.1.1l2-2a5 5 0 0 0-7.1-7.1l-1.1 1.1"/><path d="M14 11a5 5 0 0 0-7.1-.1l-2 2A5 5 0 0 0 12 20l1.1-1.1"/>',
    chart: '<path d="M4 20V10M10 20V4M16 20v-7M22 20V7"/>',
    code: '<path d="m8 9-4 3 4 3M16 9l4 3-4 3M14 5l-4 14"/>',
    check: '<circle cx="12" cy="12" r="9"/><path d="m8 12 3 3 5-6"/>',
    sync: '<path d="M20 7h-5V2M4 17h5v5M19 8a8 8 0 0 0-13-3L4 7m16 10-2 2a8 8 0 0 1-13-3"/>',
    warning: '<path d="M12 3 2.5 20h19L12 3Z"/><path d="M12 9v5M12 17h.01"/>',
    target: '<circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="5"/><circle cx="12" cy="12" r="1.5"/>',
    book: '<path d="M4 4h6a3 3 0 0 1 3 3v13a4 4 0 0 0-4-4H4zM20 4h-6a3 3 0 0 0-3 3v13a4 4 0 0 1 4-4h5z"/>',
    clock: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
    "chevron-up": '<path d="m7 14 5-5 5 5"/>',
    "chevron-left": '<path d="m15 18-6-6 6-6"/>',
    "chevron-right": '<path d="m9 18 6-6-6-6"/>',
    close: '<path d="M6 6l12 12M18 6 6 18"/>',
    plus: '<path d="M12 5v14M5 12h14"/>',
    minus: '<path d="M5 12h14"/>',
    settings: '<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.8 1.8 0 0 0 .36 2l.06.06-2.12 2.12-.06-.06a1.8 1.8 0 0 0-2-.36 1.8 1.8 0 0 0-1.1 1.65V21h-3v-.09a1.8 1.8 0 0 0-1.1-1.65 1.8 1.8 0 0 0-2 .36l-.06.06-2.12-2.12.06-.06a1.8 1.8 0 0 0 .36-2A1.8 1.8 0 0 0 5 14.4H5v-3h.09a1.8 1.8 0 0 0 1.65-1.1 1.8 1.8 0 0 0-.36-2l-.06-.06L8.44 6.1l.06.06a1.8 1.8 0 0 0 2 .36A1.8 1.8 0 0 0 11.6 4.9V4h3v.09a1.8 1.8 0 0 0 1.1 1.65 1.8 1.8 0 0 0 2-.36l.06-.06 2.12 2.12-.06.06a1.8 1.8 0 0 0-.36 2 1.8 1.8 0 0 0 1.65 1.1H21v3h-.09A1.8 1.8 0 0 0 19.4 15Z"/>',
  };
  return `<svg class="ui-icon ${esc(cls)}" viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">${paths[name] || paths.cube}</svg>`;
};
const statusClass = (value) => {
  const v = String(value || "").toLowerCase();
  if (
    ["complete", "aligned", "low", "stable", "valid", "ready", "pass"].includes(
      v,
    )
  ) return "good";
  if (["critical", "fail", "failed", "invalid", "error"].includes(v)) return "bad";
  if (
    [
      "medium",
      "high",
      "needs_convergence",
      "incomplete",
      "blocked",
      "disagreed",
      "undeclared_actual",
    ].includes(v)
  ) return "warn";
  return "info";
};
const changeNums = (item) =>
  `<span class="change-num add">+${
    num(item.additions || 0)
  }</span> <span class="change-num remove">-${num(item.deletions || 0)}</span>`;
const requestHeaders = (workspace = state.current) => {
  const headers = { "X-Wcode-UI-Token": token };
  if (workspace) headers["X-Wcode-Workspace"] = workspace;
  return headers;
};

function observationStamp() {
  return { workspace: state.current, view: state.workspaceEpoch,
    mutation: state.accessMutationEpoch, sequence: ++state.pendingSequence };
}
function observationCurrent(stamp) {
  return stamp.workspace === state.current && stamp.view === state.workspaceEpoch;
}
function observePending(value, stamp) {
  if (!observationCurrent(stamp) || stamp.mutation !== state.accessMutationEpoch ||
      stamp.sequence < state.pendingApplied || !Number.isSafeInteger(value) || value < 0) return false;
  state.pendingApplied = stamp.sequence;
  state.pendingValue = value;
  if (state.project) state.project.pending_authorizations = value;
  return true;
}

function setHtml(key, node, html, bind) {
  if (state.rendered.get(key) === html) return false;
  node.innerHTML = html;
  state.rendered.set(key, html);
  if (bind) bind();
  return true;
}
function invalidate(...keys) {
  for (const key of keys) state.rendered.delete(key);
}
function accessPanelOpen() {
  return !els.accessPanel.classList.contains("hidden");
}
function setAccessPanel(open, restoreFocus = true) {
  const wasOpen = accessPanelOpen();
  els.accessPanel.classList.toggle("hidden", !open);
  els.accessPanel.setAttribute("aria-hidden", String(!open));
  els.manage.setAttribute("aria-expanded", String(open));
  const modal = open && window.matchMedia(
    "(max-width: 900px) and (pointer: coarse)",
  ).matches;
  els.accessPanel.setAttribute("aria-modal", String(modal));
  document.documentElement.classList.toggle("access-open", open);
  if (open && !wasOpen) {
    requestAnimationFrame(() => els.closeAccess.focus({ preventScroll: true }));
  } else if (!open && wasOpen && restoreFocus) {
    els.manage.focus({ preventScroll: true });
  }
}
function cancelTunnelRefresh() {
  const controller = state.tunnelController;
  state.tunnelController = null;
  state.tunnelBusy = false;
  clearTimeout(state.tunnelTimer); state.tunnelTimer = null;
  controller?.abort();
}
function tunnelDashboardUrl(tunnel) {
  if (!tunnel?.url || tunnel.role !== "active") return "";
  try {
    const url = new URL("/intelligence", tunnel.url);
    if (!['http:', 'https:'].includes(url.protocol)) return "";
    const nextFragment = new URLSearchParams();
    if (token) nextFragment.set("token", token);
    if (state.current) nextFragment.set("workspace", state.current);
    url.hash = nextFragment.toString();
    return url.toString();
  } catch {
    return "";
  }
}
function validTunnelStatus(data) {
  const optionalString = value => value == null || typeof value === "string";
  const optionalCount = value => value == null ||
    (Number.isInteger(Number(value)) && Number(value) >= 0);
  return Boolean(
    data && typeof data === "object" && !Array.isArray(data) &&
    (data.public_url_healthy === undefined || typeof data.public_url_healthy === "boolean") &&
    optionalString(data.public_endpoint) &&
    Array.isArray(data.tunnels) && data.tunnels.length <= 64 &&
    data.tunnels.every(tunnel =>
      tunnel && typeof tunnel === "object" && !Array.isArray(tunnel) &&
      typeof tunnel.provider === "string" && tunnel.provider.length > 0 &&
      optionalString(tunnel.role) && optionalString(tunnel.state) && optionalString(tunnel.url) &&
      optionalCount(tunnel.lease_age_seconds) && optionalCount(tunnel.retry_in_seconds) &&
      optionalCount(tunnel.death_count)
    )
  );
}
async function refreshTunnels() {
  if (document.hidden || state.tunnelBusy) return;
  const controller = new AbortController();
  state.tunnelController = controller;
  state.tunnelBusy = true;
  const deadline = setTimeout(() => controller.abort(), 20000);
  state.tunnelTimer = deadline;
  try {
    // Keep /healthz: the setup projection omits retrying tunnels and diagnostics.
    const response = await fetch("/healthz", { cache: "no-store", signal: controller.signal });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    const data = await response.json();
    if (state.tunnelController !== controller || controller.signal.aborted) return;
    if (!validTunnelStatus(data)) throw new Error("Invalid tunnel response");
    const tunnels = data.tunnels;
    let html = "";
    if (tunnels.length) {
      html = tunnels.map((tunnel) => {
        const detail = [
          tunnel.role || "—",
          tunnel.state || "—",
          tunnel.lease_age_seconds == null ? null : `lease ${tunnel.lease_age_seconds}s`,
          tunnel.retry_in_seconds == null ? null : `retry ${tunnel.retry_in_seconds}s`,
          Number(tunnel.death_count || 0) ? `deaths ${tunnel.death_count}` : null,
        ].filter(Boolean).join(" · ");
        const label = `${tunnel.provider || "tunnel"} · ${tunnel.role || tunnel.state || "unknown"}`;
        const dashboardUrl = tunnelDashboardUrl(tunnel);
        if (dashboardUrl) {
          return `<a class="tunnel-chip" href="${esc(dashboardUrl)}" target="_blank" rel="noreferrer" title="${esc(`${tunnel.url} · ${detail}`)}"><i></i>${esc(label)}</a>`;
        }
        const diagnostic = [tunnel.url || null, detail].filter(Boolean).join(" · ");
        return `<span class="tunnel-chip connecting" title="${esc(diagnostic)}"><i></i>${esc(label)}</span>`;
      }).join("");
    } else if (data.public_endpoint === "pending") {
      html = `<span class="tunnel-chip connecting">${
        esc(localized("tunnels connecting…", "隧道连接中…"))
      }</span>`;
    }
    state.tunnelSnapshot = data;
    setHtml("tunnels", els.tunnels, html);
    renderRuntimeTopology();
  } catch {
    if (state.tunnelController === controller) {
      state.tunnelSnapshot = null;
      setHtml("tunnels", els.tunnels, `<span class="tunnel-chip connecting">${esc(localized("Tunnel status unavailable", "隧道状态不可用"))}</span>`);
      renderRuntimeTopology();
    }
  } finally {
    clearTimeout(deadline);
    if (state.tunnelController === controller) {
      state.tunnelController = null; state.tunnelTimer = null; state.tunnelBusy = false;
    }
  }
}
function setSync(kind, label) {
  els.syncDot.className = `sync-dot ${kind}`;
  els.syncState.textContent = label;
  els.syncState.title = "";
  els.syncState.parentElement?.setAttribute("aria-label", label);
  document.querySelector(".observatory-main")?.setAttribute("aria-busy", String(kind === "loading" && !state.project));
}
function setManualRefreshBusy(busy) {
  if (!els.refresh) return;
  els.refresh.disabled = Boolean(busy);
  els.refresh.setAttribute("aria-busy", String(Boolean(busy)));
}
function applyTheme() {
  document.documentElement.dataset.theme = state.theme;
  const light = state.theme === "light" ||
    (state.theme === "system" && systemThemeQuery.matches);
  if (els.theme) {
    els.theme.setAttribute("aria-pressed", String(state.theme !== "system"));
    els.theme.setAttribute("data-theme-state", state.theme);
    els.theme.title = `${t("Theme")} · ${t(state.theme === "system" ? "System" : state.theme === "dark" ? "Dark" : "Light")}`;
    els.theme.setAttribute("aria-label", els.theme.title);
    els.theme.classList.toggle("light-active", light);
  }
  const themeColor = document.querySelector('meta[name="theme-color"]');
  if (themeColor) themeColor.content = light ? "#f8f6fc" : "#0b0812";
}
function applyAutoRefreshControl() {
  els.auto.setAttribute("aria-pressed", String(state.autoRefresh));
  els.auto.classList.toggle("active", state.autoRefresh);
  const label = state.autoRefresh
    ? localized("Automatic refresh: on", "自动刷新：开启")
    : localized("Automatic refresh: paused", "自动刷新：暂停");
  els.auto.setAttribute("aria-label", label);
  els.auto.title = label;
  els.auto.textContent = state.autoRefresh ? t("Live") : localized("Paused", "暂停");
}
function applyLanguage() {
  document.documentElement.lang = state.language;
  document.title = t("wcode · Engineering Observatory");
  applyStaticTranslations(document, state.language);
  els.workspace.setAttribute("aria-label", t("Workspace"));
  els.language.setAttribute("aria-label", t("Language"));
  const languageLabel = els.language.querySelector("strong");
  if (languageLabel) languageLabel.textContent = state.language === "zh-CN" ? "EN" : "中";
  applyTheme();
  els.refresh.setAttribute("aria-label", t("Refresh now"));
  els.refreshSemantic?.setAttribute("aria-label", t("Refresh semantic providers"));
  els.manage?.setAttribute("aria-label", t("Workspace & command access"));
  els.projectNavigator?.setAttribute("aria-label", t("Search systems, components, requirements…"));
  els.fileSearch?.setAttribute("aria-label", t("Filter file tree"));
  applyAutoRefreshControl();
  if (typeof setCodeGraphFull === "function") setCodeGraphFull(state.codeGraphFull);
  els.workspacePath.placeholder = t("Absolute or relative project path");
  els.commandCandidate.placeholder = t("Executable name, e.g. hugo");
  els.operationProgram.placeholder = t("Executable name, e.g. make");
  els.operationArgs.placeholder = t('JSON arguments, e.g. ["test","--locked"]');
  state.rendered.clear();
  if (state.project) renderProject(true);
  if (state.access || state.workspaceAccess || state.authorizations.length) {
    renderAccess(true);
  }
}

async function uiJson(path, method = "GET", body, options = {}) {
  const headers = { ...requestHeaders(options.workspace ?? state.current), ...(options.headers || {}) };
  if (!token) {
    const error = new Error(localized("Open this page from the wcode TUI to authorize access.", "请从 wcode 终端面板打开此页面以授权访问。"));
    error.code = "authorization_required";
    throw error;
  }
  if (body !== undefined) headers["Content-Type"] = "application/json";
  const controller = new AbortController();
  let timedOut = false;
  const abort = () => controller.abort();
  options.signal?.addEventListener("abort", abort, { once: true });
  if (options.signal?.aborted) controller.abort();
  const deadline = setTimeout(() => { timedOut = true; controller.abort(); }, options.timeout || 30000);
  try {
    const response = await fetch(path, {
      method, headers, cache: "no-store", signal: controller.signal,
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    let data;
    try { data = await response.json(); } catch {
      if (response.ok) {
        const error = new Error(localized("Invalid JSON response", "响应不是有效 JSON"));
        error.code = "invalid_response";
        throw error;
      }
    }
    if (!response.ok) {
      const detail = typeof data?.error === "string" ? data.error : data?.error?.message;
      const error = new Error(`HTTP ${response.status}${detail ? ` · ${detail}` : ""}`);
      error.status = response.status;
      throw error;
    }
    if (!data || typeof data !== "object" || Array.isArray(data)) {
      const error = new Error("Invalid response");
      error.code = "invalid_response";
      throw error;
    }
    return data;
  } catch (error) {
    if (timedOut) {
      error = new Error(localized("Request timed out; displayed data may be stale.", "请求超时，显示的数据可能已过期。"));
      error.code = "timeout";
    } else if (!error.code && !error.status && error.name === "TypeError") {
      error.code = "network";
    }
    if (method !== "GET" && (!error.status || error.status >= 500)) {
      error.uncertain = true;
      error.message += localized(" The operation may have completed. Refresh its state before retrying.", " 操作可能已经完成，请先刷新实际状态，再决定是否重试。");
    }
    throw error;
  } finally {
    clearTimeout(deadline);
    options.signal?.removeEventListener("abort", abort);
  }
}
function requestFailureMessage(error) {
  const status = Number.isInteger(error?.status) ? error.status : null;
  let message;
  if (error?.code === "authorization_required" || status === 401) {
    message = localized("Authorization required. Reopen the current WCode page from the terminal.", "需要重新授权。请从 WCode 终端重新打开当前页面。");
  } else if (status === 403) {
    message = localized("Access denied. Review the current session authorization and try again.", "访问被拒绝。请检查当前会话授权后重试。");
  } else if (error?.code === "timeout" || status === 408 || status === 504) {
    message = localized("Request timed out. Check WCode logs and try again.", "请求超时。请检查 WCode 日志后重试。");
  } else if (error?.code === "network" || error?.name === "AbortError") {
    message = localized("Connection failed. Check that WCode is still running, then retry.", "连接失败。请确认 WCode 仍在运行后重试。");
  } else if (error?.code === "invalid_response") {
    message = localized("The server returned an invalid response. Check WCode logs and refresh the page.", "服务端返回了无效响应。请检查 WCode 日志并刷新页面。");
  } else if (status === 400) {
    message = localized("Request rejected. Check the entered values and try again.", "请求被拒绝。请检查输入内容后重试。");
  } else if (status === 404) {
    message = localized("The requested endpoint is unavailable. Refresh the current WCode page.", "当前接口不可用。请刷新当前 WCode 页面。");
  } else if (status === 409) {
    message = localized("State changed while the request was running. Refresh before retrying.", "请求执行期间状态已变化。请先刷新再重试。");
  } else if (status === 429) {
    message = localized("WCode is busy. Let the current work settle, then retry.", "WCode 当前繁忙。请等待现有任务缓解后重试。");
  } else if (status && status >= 500) {
    message = localized("WCode could not complete the request. Check the logs, then retry.", "WCode 未能完成请求。请检查日志后重试。");
  } else if (status) {
    message = state.language === "zh-CN" ? `请求失败（HTTP ${status}）。` : `Request failed (HTTP ${status}).`;
  } else {
    message = error?.message || localized("Request failed. Check WCode logs and try again.", "请求失败。请检查 WCode 日志后重试。");
  }
  if (error?.uncertain) {
    message += localized(" The operation may have completed; refresh its state before retrying.", " 操作可能已经完成；重试前请先刷新实际状态。");
  }
  return message;
}
function authorizationKind(kind) {
  return {
    "command_access": t("Executable access"),
    "risky_execution": t("Exact repository operation"),
    "runtime_executor": t("Runtime executor"),
    "destructive_delete": t("Destructive delete"),
  }[kind] || kind;
}

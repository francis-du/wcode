const fragment = new URLSearchParams(location.hash.slice(1));
const token = fragment.get("token") || "";
const initialWorkspace = fragment.get("workspace") || "";
const savedLanguage = localStorage.getItem("wcode.ui.language");
const savedTheme = localStorage.getItem("wcode.ui.theme");
const translations = {
  "zh-CN": {
    "Project Observatory": "项目观测台",
    "observatory subtitle":
      "实时查看目标状态、实现、变更、证据与收敛，并始终显示数据 Provider 与 Precision。",
    "auto refresh": "自动刷新",
    "Refresh semantics": "刷新语义",
    "Manage access": "管理访问",
    "Refresh now": "立即刷新",
    "Connecting": "连接中",
    "Live": "实时",
    "Syncing": "同步中",
    "Refresh failed": "刷新失败",
    "Theme": "主题",
    "System": "跟随系统",
    "Dark": "深色",
    "Light": "浅色",
    "Workspace & command access": "项目与命令授权",
    "access safety note":
      "Session 级权限仍受 Workspace、命令策略和高风险操作策略约束。",
    "Close": "关闭",
    "Authorized projects": "已授权项目",
    "Authorize project": "授权项目",
    "project safety note": "项目根目录会规范化处理并保持隔离。",
    "Authorized commands": "已授权命令",
    "Authorize command": "授权命令",
    "command safety note": "可执行程序权限按项目隔离；Shell 解释器继续禁止。",
    "Exact repository operation": "精确仓库操作",
    "Authorize operation": "授权操作",
    "operation safety note": "只授权当前 Session 内一个精确的仓库感知命令。",
    "Pending authorizations": "待授权请求",
    "authorization safety note": "每次只批准或拒绝一个精确请求。",
    "Approve": "批准",
    "Deny": "拒绝",
    "No pending authorizations": "没有待授权请求",
    "No authorized projects": "没有已授权项目",
    "No commands authorized": "没有已授权命令",
    "Command access": "命令权限",
    "Risky execution": "高风险执行",
    "Runtime executor": "运行时执行器",
    "Destructive delete": "删除操作",
    "Authorization approved": "授权已批准",
    "Authorization denied": "授权已拒绝",
    "Unable to update authorizations": "无法更新授权请求",
    "Unable to update access": "无法更新访问配置",
    "Workspace added": "工作区已添加",
    "Command authorized": "命令已授权",
    "Command revoked": "命令授权已撤销",
    "Operation authorized": "操作已授权",
    "Requirements": "需求",
    "All": "全部",
    "Changed": "有变更",
    "Needs convergence": "需要收敛",
    "Incomplete": "不完整",
    "Search requirement, feature, component…": "搜索需求、功能、组件…",
    "Loading project state…": "正在加载项目状态…",
    "Current changes": "当前变更",
    "current changes meta":
      "Working Tree 变更映射回 Requirement 与 Product Scope。",
    "Code statistics": "代码统计",
    "current repository snapshot": "当前有界仓库快照。",
    "Architecture revisions": "架构版本",
    "meaningful graph snapshots": "有意义的 Composite Graph 快照与结构化风险。",
    "Language quality matrix": "语言质量矩阵",
    "language quality meta":
      "按能力显示覆盖与缺口；需要 Provider 细节时再展开。",
    "Desired State": "目标状态",
    "Actual State": "实际状态",
    "Change": "变更",
    "Proof": "证据",
    "Convergence": "收敛",
    "Feature architecture · desired vs actual": "功能架构 · 目标与实际",
    "Acceptance & verification": "验收与验证",
    "Constraints, decisions & drift": "约束、决策与漂移",
    "Current changes touching this feature": "当前影响该功能的变更",
    "Design architecture": "设计架构",
    "Actual code architecture · generated from current implementation":
      "实际代码架构 · 从当前实现生成",
    "Languages": "语言",
    "Product Scopes": "Product Scope",
    "Latest structural delta": "最近结构变化",
    "Current structured risks": "当前结构化风险",
    "Path": "路径",
    "Status": "状态",
    "Scope": "范围",
    "Diff": "差异",
    "Files": "文件",
    "Language": "语言",
    "Syntax": "语法",
    "Semantic": "语义",
    "Format": "格式化",
    "Lint": "Lint",
    "Type": "类型",
    "Static": "静态分析",
    "Test": "测试",
    "Security": "安全",
    "Advanced": "高级验证",
    "Gaps": "缺口",
    "Select a requirement.": "请选择一个需求。",
    "Design valid": "设计有效",
    "Design invalid": "设计无效",
    "No critical attention items": "当前没有需要立即处理的信号",
    "Semantic graph active": "Semantic Graph 已生效",
    "Syntax fallback": "Syntax Fallback",
    "Semantic provider available": "Semantic Provider 可用",
    "Refresh semantics for stronger dependency evidence":
      "刷新语义以获得更强的依赖证据",
    "Verification failed": "验证失败",
    "Verification disagreement": "验证存在分歧",
    "Requirements need convergence": "个需求需要收敛",
    "Critical risk": "Critical 风险",
    "High risk": "High 风险",
    "Pending approval": "个待授权请求",
    "Semantic refresh complete": "语义刷新完成",
    "Semantic refresh needs approval": "语义刷新需要人工批准",
    "No implementation reference declared.": "没有声明实现引用。",
    "No responsibilities declared.": "没有声明职责。",
    "No current implementation mapping.": "没有当前实现映射。",
    "No cross-component dependency is declared or detected for this feature.":
      "该功能没有声明或观测到跨组件依赖。",
    "No acceptance criteria.": "没有验收条件。",
    "No requirement-specific constraints.": "没有该需求专属约束。",
    "No current working-tree file is mapped to this requirement.":
      "当前 Working Tree 没有文件映射到该需求。",
    "Working tree is clean or Git review is unavailable.":
      "Working Tree 干净，或 Git Review 当前不可用。",
    "No previous meaningful graph revision yet.":
      "暂无上一版有意义的 Graph Revision。",
    "No data.": "暂无数据。",
    "No supported source language detected in the bounded repository snapshot.":
      "当前有界仓库快照中未检测到支持的源码语言。",
    "declared coverage complete": "声明覆盖完整",
    "gap": "缺口",
    "gaps": "缺口",
    "provider precision": "Provider 精度",
    "advisory": "提示",
    "blocker": "阻塞",
    "blockers": "阻塞",
    "advisories": "提示",
    "not observed": "未观测到",
    "last updated": "更新于",
    "Refreshing project state…": "正在刷新项目状态…",
  },
};
Object.assign(translations["zh-CN"], {
  "Executable access": "可执行程序访问",
  "Authorize executable": "授权可执行程序",
  "Command access": "可执行程序访问",
  "Risky execution": "精确仓库操作",
  "observatory subtitle":
    "实时查看目标状态、实现、变更、证据与收敛，并始终显示数据来源与精度。",
  "access safety note":
    "会话级权限仍受项目隔离、命令策略和高风险操作策略约束。",
  "command safety note": "可执行程序权限按项目隔离；不会授权该程序的所有参数。",
  "operation safety note":
    "必须先允许可执行程序；这里只授权精确参数与工作目录。",
  "current changes meta": "工作树变更映射回需求与产品范围。",
  "meaningful graph snapshots": "有意义的软件图谱快照与结构化风险。",
  "language quality meta": "按能力显示覆盖与缺口；需要分析器细节时再展开。",
  "Product Scopes": "产品范围",
  "Semantic graph active": "语义图谱已生效",
  "Syntax fallback": "语法级回退",
  "Semantic provider available": "语义分析器可用",
  "Critical risk": "严重风险",
  "High risk": "高风险",
  "provider precision": "数据来源精度",
  "Workspace": "工作区",
  "Language": "语言",
  "Revoke": "撤销",
  "complete": "完整",
  "aligned": "已对齐",
  "low": "低",
  "stable": "稳定",
  "valid": "有效",
  "ready": "就绪",
  "pass": "通过",
  "critical": "严重",
  "failed": "失败",
  "invalid": "无效",
  "error": "错误",
  "medium": "中",
  "high": "高",
  "needs convergence": "需要收敛",
  "incomplete": "不完整",
  "blocked": "阻塞",
  "disagreed": "有分歧",
  "undeclared actual": "未声明的实际依赖",
  "unverified actual": "未验证的实际依赖",
  "unknown": "未知",
  "declared": "已声明",
  "current": "当前",
  "resolved": "已解析",
  "unresolved": "未解析",
  "semantic": "语义",
  "syntax": "语法",
  "runtime": "运行时",
  "evidence": "证据",
  "covered": "已覆盖",
  "available": "可用",
  "clean": "无变更",
  "changed": "已变更",
  "untracked": "未跟踪",
  "truncated": "已截断",
  "nodes": "节点",
  "edges": "边",
  "requirement": "需求",
  "accepted": "已采纳",
  "proposed": "提议中",
  "deprecated": "已弃用",
  "superseded": "已取代",
  "added": "新增",
  "modified": "修改",
  "deleted": "删除",
  "renamed": "重命名",
  "none": "无",
  "No requirements match this filter.": "没有符合当前筛选条件的需求。",
  "No implementation component declared.": "没有声明实现组件。",
  "No acceptance criterion declared.": "没有声明验收条件。",
  "Bounded snapshot": "有界快照",
  "bounded graph note": "实时代码图谱达到安全上限；统计与架构信息可能不完整。",
  "Design diagnostics require attention": "设计诊断需要处理",
  "Open Manage access to review exact requests": "打开“管理访问”以审核精确请求",
  "Design, proof and convergence have no active blockers":
    "设计、证据与收敛当前没有阻塞项",
  "positive evidence note":
    "只有明确的数据来源证据才会阻塞收敛；有界语法图谱中未观测到关系只作为提示。",
  "security footer":
    "只有在本页面 URL 片段中的本地 UI 令牌被提交给受保护的智能端点后，项目数据才会返回。URL 片段本身不会进入 HTTP 请求或服务器日志。",
  "Architecture overview": "整体架构",
  "architecture overview meta":
    "对比完整的设计组件架构与当前实现中实际观测到的依赖。",
  "Overlay": "叠加对比",
  "Design": "设计",
  "Implementation": "实现",
  "Aligned dependency": "设计与实现对齐",
  "Declared, not yet observed": "设计已声明、实现尚未观测",
  "Observed implementation edge": "实现中观测到的依赖",
  "Strong observed drift": "强证据架构偏离",
  "Observed drift": "已观测偏离度",
  "Evidence coverage": "设计依赖证据覆盖",
  "Implementation coverage": "组件实现覆盖",
  "Architecture size": "架构规模",
  "strong drift denominator":
    "仅用已观测实际依赖计算；强语义/运行时证据的未声明依赖才算偏离。",
  "coverage denominator":
    "设计依赖被当前实现图确认的比例；未观测不等于不存在。",
  "implementation denominator": "在设计状态中声明了实现路径的组件比例。",
  "architecture size detail": "组件 / 设计依赖 / 实际依赖",
  "Component detail": "组件详情",
  "Responsibilities": "职责",
  "Implementation mapping": "实现映射",
  "Related requirements": "关联需求",
  "Dependencies": "依赖关系",
  "Changed paths": "当前变更路径",
  "Product scopes": "产品范围",
  "No component selected.": "请选择一个组件。",
  "No implementation mapping.": "没有实现映射。",
  "No related requirements.": "没有关联需求。",
  "No dependency edges.": "没有依赖边。",
  "No current component changes.": "该组件当前没有变更。",
  "No product scope mapping.": "没有产品范围映射。",
  "Architecture aligned": "架构已对齐",
  "Architecture drift": "存在架构偏离",
  "Needs stronger evidence": "需要更强证据",
  "Click a component to inspect it.": "点击组件查看职责、实现和依赖详情。",
  "design edge": "设计依赖",
  "actual edge": "实际依赖",
  "incoming": "被依赖",
  "outgoing": "依赖",
  "observed actual": "实际已观测",
  "not observed": "未观测到",
  "deterministic": "确定性",
  "heuristic": "启发式",
  "mixed": "混合",
  "Project files": "项目文件",
  "project files meta":
    "查看有界源码快照的目录层级，并找出超过项目行数上限的文件。",
  "File structure": "文件结构",
  "Largest files": "大文件",
  "No source files in this snapshot.": "当前快照中没有源码文件。",
  "Within line limit": "均未超过行数上限",
  "Snapshot truncated": "快照已截断",
});

const q = (id) => document.querySelector(id);
const els = {
  workspace: q("#workspace"),
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
  operationProgram: q("#operationProgram"),
  operationArgs: q("#operationArgs"),
  operationCwd: q("#operationCwd"),
  authorizeOperation: q("#authorizeOperation"),
  operationMessage: q("#operationMessage"),
  authorizationList: q("#authorizationList"),
  authorizationMessage: q("#authorizationMessage"),
  stats: q("#stats"),
  attention: q("#attention"),
  architectureMetrics: q("#architectureMetrics"),
  architectureGraph: q("#architectureGraph"),
  componentInspector: q("#componentInspector"),
  requirements: q("#requirements"),
  reqCount: q("#reqCount"),
  detail: q("#featureDetail"),
  search: q("#reqSearch"),
  languageQuality: q("#languageQuality"),
  qualitySummary: q("#qualitySummary"),
  codeStats: q("#codeStats"),
  revisions: q("#revisions"),
  changes: q("#changes"),
  structureSummary: q("#structureSummary"),
  fileTree: q("#fileTree"),
  largeFiles: q("#largeFiles"),
  auto: q("#autoRefresh"),
  refresh: q("#refresh"),
  refreshSemantic: q("#refreshSemantic"),
  syncDot: q("#syncDot"),
  syncState: q("#syncState"),
  projectIdentity: q("#projectIdentity"),
  precisionBadge: q("#precisionBadge"),
  lastUpdated: q("#lastUpdated"),
  tunnels: q("#tunnels"),
};

const state = {
  current: initialWorkspace,
  project: null,
  access: null,
  workspaceAccess: null,
  authorizations: [],
  accessLoaded: false,
  selected: "",
  selectedComponent: "",
  filter: "all",
  architectureMode: "overlay",
  timer: null,
  language: savedLanguage === "zh-CN" ? "zh-CN" : "en",
  theme: ["system", "dark", "light"].includes(savedTheme)
    ? savedTheme
    : "system",
  rendered: new Map(),
  controller: null,
  requestEpoch: 0,
  inFlight: false,
  lastUpdated: 0,
  revisionKey: null,
  semanticRefreshPending: false,
};
const t = (key) => translations[state.language]?.[key] || key;
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
const statusClass = (value) => {
  const v = String(value || "").toLowerCase();
  if (
    ["complete", "aligned", "low", "stable", "valid", "ready", "pass"].includes(
      v,
    )
  ) return "good";
  if (["critical", "failed", "invalid", "error"].includes(v)) return "bad";
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
const requestHeaders = () => {
  const headers = { "X-Wcode-UI-Token": token };
  if (state.current) headers["X-Wcode-Workspace"] = state.current;
  return headers;
};

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
async function refreshTunnels() {
  try {
    const response = await fetch("/healthz");
    if (!response.ok) return;
    const data = await response.json();
    const tunnels = data.tunnels || [];
    let html = "";
    if (tunnels.length) {
      html = tunnels.map((tunnel) =>
        `<a class="tunnel-chip" href="${
          esc(tunnel.url)
        }" target="_blank" rel="noreferrer" title="${esc(tunnel.url)}"><i></i>${
          esc(tunnel.provider)
        }</a>`
      ).join("");
    } else if (data.public_endpoint === "pending") {
      html = `<span class="tunnel-chip connecting">${
        esc(localized("tunnels connecting…", "隧道连接中…"))
      }</span>`;
    }
    setHtml("tunnels", els.tunnels, html);
  } catch {}
}
function setSync(kind, label) {
  els.syncDot.className = `sync-dot ${kind}`;
  els.syncState.textContent = label;
  els.refresh.disabled = kind === "loading";
}
function applyTheme() {
  document.documentElement.dataset.theme = state.theme;
  els.theme.value = state.theme;
}
function applyLanguage() {
  document.documentElement.lang = state.language;
  els.language.value = state.language;
  document.querySelectorAll("[data-i18n]").forEach((node) => {
    node.textContent = t(node.dataset.i18n);
  });
  document.querySelectorAll("[data-i18n-placeholder]").forEach((node) => {
    node.placeholder = t(node.dataset.i18nPlaceholder);
  });
  els.workspace.setAttribute("aria-label", t("Workspace"));
  els.language.setAttribute("aria-label", t("Language"));
  els.theme.setAttribute("aria-label", t("Theme"));
  els.theme.querySelector('[value="system"]').textContent = t("System");
  els.theme.querySelector('[value="dark"]').textContent = t("Dark");
  els.theme.querySelector('[value="light"]').textContent = t("Light");
  els.workspacePath.placeholder = t("Absolute or relative project path");
  els.commandCandidate.placeholder = state.language === "zh-CN"
    ? "可执行程序名，例如 hugo"
    : "Executable name, e.g. hugo";
  els.operationProgram.placeholder = state.language === "zh-CN"
    ? "可执行程序名，例如 make"
    : "Executable name, e.g. make";
  els.operationArgs.placeholder = state.language === "zh-CN"
    ? "参数，以空格分隔"
    : "Arguments, whitespace separated";
  state.rendered.clear();
  if (state.project) renderProject(true);
  if (state.access || state.workspaceAccess || state.authorizations.length) {
    renderAccess(true);
  }
}

async function uiJson(path, method = "GET", body) {
  const headers = requestHeaders();
  if (body !== undefined) headers["Content-Type"] = "application/json";
  const response = await fetch(path, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  let data = {};
  try {
    data = await response.json();
  } catch {}
  if (!response.ok) throw new Error(data.error || `HTTP ${response.status}`);
  return data;
}
function authorizationKind(kind) {
  return {
    "command_access": t("Executable access"),
    "risky_execution": t("Exact repository operation"),
    "runtime_executor": t("Runtime executor"),
    "destructive_delete": t("Destructive delete"),
  }[kind] || kind;
}

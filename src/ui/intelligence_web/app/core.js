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
const translations = {
  "zh-CN": {
    "Engineering Observatory": "工程观测台",
    "Project digital twin": "项目数字孪生",
    "Overview": "总览",
    "Architecture": "架构",
    "Activity": "活动",
    "Proof": "证据",
    "Engineering": "工程",
    "Changes": "变更",
    "Files": "文件",
    "Diagnostics": "诊断",
    "Quality": "质量",
    "rail note": "先看架构与变更链，需要证据时再展开原始表格。",
    "Find subsystem, component, requirement or file…": "搜索子系统、组件、需求或文件…",
    "Search systems, components, requirements…": "搜索系统、组件、需求…",
    "Project navigator": "项目导航",
    "observatory subtitle":
      "不打开代码编辑器，也能看懂项目架构、Vibe Coding 做了什么、设计与现实是否偏离，以及哪些结果已经被证明。",
    "auto refresh": "自动刷新",
    "Refresh semantics": "刷新语义",
    "Manage access": "管理访问",
    "Semantics": "语义",
    "Access": "访问",
    "Refresh now": "立即刷新",
    "Fit": "适配全图",
    "Open full map": "打开全图",
    "System architecture map": "系统架构地图",
    "Blueprint": "蓝图",
    "System map": "系统地图",
    "architecture map note": "四个语义层展示系统全貌；精确依赖证据统一在“依赖”账本中查看。",
    "Dependencies": "依赖",
    "Data source": "数据来源",
    "View": "视图",
    "Relationships": "关系",
    "Focus": "聚焦",
    "All relationships": "全部",
    "Zoom": "缩放",
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
    "authorization safety note": "可批准当前精确请求、为当前 Workspace 本次运行全部授权，或拒绝。",
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
    "Authorize all commands": "全部授权",
    "Disable all command authorization": "关闭全部授权",
    "All commands authorized for this session": "本次运行已全部授权",
    "All commands require per-request approval": "命令按请求授权",
    "All command authorization enabled": "已开启全部授权",
    "All command authorization disabled": "已关闭全部授权",
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
    "Mapped": "已映射",
    "Executed": "已执行",
    "Passed": "已通过",
    "Fresh": "当前版本",
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
    "Tree-sitter only": "Tree-sitter Only",
    "LSP available": "LSP 可用",
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
  "Tree-sitter only": "仅 Tree-sitter",
  "LSP available": "LSP 可用",
  "Critical risk": "严重风险",
  "High risk": "高风险",
  "provider precision": "数据来源精度",
  "Workspace": "工作区",
  "Language / Parser": "语言 / 解析器",
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
  "Engineering architecture": "工程架构",
  "Architecture blueprint": "架构蓝图",
  "Live engineering flow": "实时工程流",
  "live engineering flow meta": "实时投影仓库理解、受控修改、验证证明、经验学习和观测状态。",
  "Vibe coding change story": "Vibe Coding 变更链",
  "vibe coding change story meta": "文件 → 组件 → 需求 → 验证 → 已确认架构偏离。",
  "Live engineering timeline": "实时工程时间线",
  "live engineering timeline meta": "把真实 Harness 活动、验证证据和架构版本合成一条有界事件流。",
  "Engineering signals": "工程信号",
  "engineering signals meta": "工程流、变更链、运行拓扑与最近工程事件。",
  "architecture overview meta":
    "先按分层蓝图读懂系统，再下钻组件或查看原始依赖图。",
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
  "Filter file tree…": "筛选文件名或路径…",
  "Filter file tree": "筛选文件树",
  "No matching files.": "没有匹配的文件。",
  "Largest files": "大文件",
  "No source files in this snapshot.": "当前快照中没有源码文件。",
  "Within line limit": "均未超过行数上限",
  "Snapshot truncated": "快照已截断",
});

Object.assign(translations["zh-CN"], {
  "observatory subtitle": "不打开代码编辑器，也能看懂项目架构、Vibe Coding 做了什么、设计与现实是否偏离，以及哪些结果已经被证明。",
  "Task activity": "任务活动", "Verification evidence": "验证证据",
  "Engineering closed loop": "工程闭环流程",
  "engineering cycle meta": "理解 → 规划 → 实施 → 证明 → 学习 → 观测，并通过版本绑定证据形成持续反馈。",
  "Change impact snapshot": "变更影响快照",
  "change impact snapshot meta": "文件 → 组件 → 需求 → 验证 → 偏离。",
  "Runtime signals": "运行时信号",
  "runtime signals meta": "入口、工作区边界、Harness、仓库模型与验证状态。",
  "Requirement to evidence traceability": "需求到证据追踪",
  "traceability map meta": "沿需求意图追踪到归属组件、实现代码、验证检查与版本绑定证据。",
  "Change impact and convergence": "变更影响与收敛",
  "change convergence meta": "看清改了什么、影响什么、需要哪些证明，以及最终还剩多少风险与信心。",
  "Task activity meta": "正在执行的任务优先。等待时间与执行时间分别展示。",
  "Proof meta": "当前版本、历史结果与尚未验证的工作，分别展示。",
  "Components": "组件", "Component map": "组件地图", "Dependency graph": "依赖连线图",
  "Find a component": "查找组件", "Name, responsibility or scope": "搜索名称、职责或所属范围",
  "Explore requirement details": "查看实现、验收条件与依赖证据",
  "Diagnostics & history": "诊断与历史", "Diagnostics meta": "代码分布、图谱版本与已记录风险",
  "Verification impact": "验证影响",
  "verification impact meta": "解释当前改动为什么会扩大到这些项目岛的验证范围。",
  "Adaptive verification": "自适应验证",
  "adaptive verification meta": "只读预览下一次 quick 为什么可能优先运行聚焦测试或 fail-fast sentinel；full 覆盖保持不变。",
  "Verified learning": "验证学习",
  "verified learning meta": "按全局时间隔离评估不保存提示词的已验证共改记忆。",
  "Live runtime topology": "实时运行拓扑",
  "live runtime topology meta": "从真实运行遥测投影当前入口、MCP/授权、工作区边界、Harness 队列、仓库模型和验证状态。",
});

// Descriptive translation keys need English copy too, not their internal IDs.
translations.en = {
  "Diagnostics meta": "Code distribution, graph revisions and recorded risks.",
  "adaptive verification meta": "Read-only preview of focused tests and fail-fast checks. Full verification coverage is unchanged.",
  "verified learning meta": "Temporal holdout evaluation of prompt-free verified change history.",
  "bounded graph note": "The live code graph reached its safety bound; statistics and architecture may be incomplete.",
  "current repository snapshot": "Current bounded repository snapshot.",
  "meaningful graph snapshots": "Meaningful graph snapshots and recorded structural changes.",
};

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
  filter: "all",
  architectureMode: "overlay",
  timer: null,
  language: savedLanguage === "zh-CN" ? "zh-CN" : "en",
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
    if (!data || typeof data !== "object" || Array.isArray(data) || !Array.isArray(data.tunnels) ||
        data.tunnels.some(tunnel => !tunnel || typeof tunnel !== "object" || Array.isArray(tunnel))) {
      throw new Error("Invalid tunnel response");
    }
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
  els.refresh.disabled = kind === "loading";
  document.querySelector(".observatory-main")?.setAttribute("aria-busy", String(kind === "loading" && !state.project));
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
  document.querySelectorAll("[data-i18n]").forEach((node) => {
    if (!node.dataset.i18nFallback) node.dataset.i18nFallback = node.textContent;
    node.textContent = translations[state.language]?.[node.dataset.i18n] || node.dataset.i18nFallback;
  });
  document.querySelectorAll("[data-i18n-placeholder]").forEach((node) => {
    node.placeholder = t(node.dataset.i18nPlaceholder);
  });
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
  els.workspacePath.placeholder = t("Absolute or relative project path");
  els.commandCandidate.placeholder = state.language === "zh-CN"
    ? "可执行程序名，例如 hugo"
    : "Executable name, e.g. hugo";
  els.operationProgram.placeholder = state.language === "zh-CN"
    ? "可执行程序名，例如 make"
    : "Executable name, e.g. make";
  els.operationArgs.placeholder = state.language === "zh-CN"
    ? 'JSON 参数数组，例如 ["commit","-m","两词说明"]'
    : 'JSON arguments, e.g. ["commit","-m","two words"]';
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

// Design mappings are navigation context, not command or verification authority.
// The native graph/source endpoints still validate Workspace, revision and SHA.
const sourceActionRenders = new Map();

function validRepositorySourcePath(path) {
  return typeof path === "string" && path.length > 0 && path.length <= 4096
    && !/^[\\/]|^[A-Za-z]:|[\u0000-\u001f\u007f]/.test(path)
    && path.split(/[\\/]/).every(part => part && part !== "." && part !== "..");
}
function designReferencePath(reference) {
  if (!reference || reference.provider === "harness") return "";
  // The native projection may put an extensionless file in `symbol` with an
  // empty `path`. A filesystem reference's exact target is the file identity.
  if (reference.provider === "filesystem") return validRepositorySourcePath(reference.target) ? reference.target : "";
  if (!validRepositorySourcePath(reference.path)) return "";
  const symbol = reference.symbol;
  if (symbol != null && (typeof symbol !== "string" || !symbol)) return "";
  // A check ID may look like a file name. Only filesystem references and
  // explicitly mapped symbols can establish a source-file observation.
  if (symbol == null) return "";
  const target = symbol == null ? reference.path : `${reference.path}::${symbol}`;
  return reference.target === target ? reference.path : "";
}
function repositorySourcePaths() {
  const project = state.project, paths = new Set();
  if (!project || project.workspace !== state.current) return paths;
  for (const item of [...(project.structure?.entries || []), ...(project.structure?.largest_files || []), ...(project.attention?.items || [])]) {
    if (validRepositorySourcePath(item?.path)) paths.add(item.path);
  }
  // Resolved mappings carry their own observed paths. They need not also fit
  // inside the separately bounded project-file tree.
  for (const requirement of project.requirements || []) {
    const references = [
      ...(requirement.components || []).flatMap(component => component.implementation || []),
      ...(requirement.acceptance || []).flatMap(criterion => criterion.verification || []),
    ];
    for (const reference of references) {
      const path = designReferencePath(reference);
      if (path && reference.resolved === true) paths.add(path);
    }
  }
  return paths;
}
function designSourceTargetPath(target, paths = repositorySourcePaths()) {
  if (typeof target !== "string" || !target) return "";
  if (paths.has(target)) return target;
  const matches = [...paths].filter(path => target.startsWith(`${path}::`) && target.length > path.length + 2);
  return matches.length === 1 ? matches[0] : "";
}
function designSourceAction(reference, paths = repositorySourcePaths()) {
  if (reference && typeof reference === "object" && reference.provider === "harness") return "";
  const path = typeof reference === "string" ? designSourceTargetPath(reference, paths) : designReferencePath(reference);
  const target = typeof reference === "string" ? reference : reference?.target || "";
  if (!path || !paths.has(path)) return `<span class="design-source-unavailable panel-meta">${esc(localized("Source is unavailable in this observation.", "此观测未提供源码入口。"))}</span>`;
  const enabled = Boolean(token) && validCodeGraphRepositoryRevision(repositorySourceRevision());
  const note = enabled
    ? localized("Opens the mapped file at the observed revision; a mapping is not execution proof.", "打开观测版本的映射文件；映射不代表执行证明。")
    : localized("Refresh the project with UI access before inspecting source.", "请在有界面访问权限时刷新项目，再检查源码。");
  return `<button type="button" class="quiet-action design-source-action" data-source-path="${esc(path)}" ${enabled ? "" : "disabled"} title="${esc(note)}" aria-label="${esc(localized("Inspect mapped file: ", "检查映射文件：") + target)}">${esc(localized("Inspect mapped file", "检查映射文件"))}</button>`;
}
function sourceActionScope(key, context) {
  return { key, workspace: state.current, owner: token, epoch: state.workspaceEpoch,
    revision: repositorySourceRevision(), ...context };
}
function sameSourceActionScope(left, right) {
  return left && left.workspace === right.workspace && left.owner === right.owner && left.epoch === right.epoch
    && left.tab === right.tab && left.selection === right.selection && left.id === right.id && left.view === right.view
    && left.revision.code === right.revision.code && (left.revision.design ?? null) === (right.revision.design ?? null);
}
function sourceActionCurrent(scope) {
  return sourceActionRenders.get(scope.key) === scope && token === scope.owner && Boolean(token)
    && state.current === scope.workspace && state.workspaceEpoch === scope.epoch
    && state.project?.workspace === scope.workspace && state.workspaceTab === scope.tab
    && repositorySourceRevisionMatches(scope.revision)
    && (scope.view === undefined || state.architectureView === scope.view)
    && (scope.selection !== "requirement" || state.selected === scope.id)
    && (scope.selection !== "component" || state.selectedComponent === scope.id);
}
function bindRepositorySourceActions(node, scope) {
  node.querySelectorAll("[data-source-path]").forEach(button => {
    const path = button.dataset.sourcePath;
    button.addEventListener("click", () => {
      if (!button.disabled && sourceActionCurrent(scope) && repositorySourcePaths().has(path)) void openRepositoryFile(path);
    });
  });
}
function setSourceActionHtml(key, node, html, context, bind) {
  const scope = sourceActionScope(key, context);
  if (!sameSourceActionScope(sourceActionRenders.get(key), scope)) invalidate(key);
  return setHtml(key, node, html, () => {
    sourceActionRenders.set(key, scope);
    bindRepositorySourceActions(node, scope);
    bind?.();
  });
}

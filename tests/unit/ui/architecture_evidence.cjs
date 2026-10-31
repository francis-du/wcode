'use strict';
function runArchitectureEvidenceRegressions() {
  const assert = require("node:assert/strict");
  const fs = require("node:fs");
  const path = require("node:path");
  const vm = require("node:vm");
  const root = process.argv[2] || path.resolve(__dirname, "../../..");
  const source = ["architecture", "engineering"].map(name =>
    fs.readFileSync(path.join(root, `src/ui/intelligence_web/app/${name}.js`), "utf8")
  ).join("\n");
  const encode = value => String(value ?? "").replace(/[&<>"']/g, char =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[char]
  );
  function element() {
    return {
      innerHTML: "", textContent: "", value: "", style: { setProperty() {} },
      classList: { add() {}, remove() {}, toggle() {} },
      querySelectorAll() { return []; }, querySelector() { return null; },
      setAttribute() {}, scrollIntoView() {}, focus() {},
    };
  }
  function fixture(language = "en") {
    const components = ["agent", "experience", "workspace"].map(id => ({
      id, name: id === "agent" ? "Agent Context" : id === "experience" ? "Experience Graph" : "Workspace",
      depends_on: [], responsibilities: [], implementation_targets: [],
    }));
    const context = {
      state: { language, architectureMode: "overlay", selectedComponent: "agent", project: {
        project: "Fixture", structure: { truncated: false }, requirements: [],
        architecture: { components, dependencies: [], dependency_candidates: [], dependency_candidates_total: 0 },
      } },
      els: { architectureGraph: element(), architectureBlueprint: element(), componentInspector: element() },
      document: { querySelector() { return null; }, querySelectorAll() { return []; } },
      num: value => String(value), esc: encode, statusLabel: value => String(value),
      t: value => value, unit: value => String(value), uiIcon: () => "",
      localized: (en, zh) => context.state.language === "zh-CN" ? zh : en,
      pill: (label, tone) => `<span class="pill ${encode(tone)}">${encode(label)}</span>`,
      setHtml: (_key, target, html, callback) => { target.innerHTML = html; callback?.(); },
      requestAnimationFrame: callback => callback(), invalidate() {},
    };
    vm.createContext(context); vm.runInContext(source, context);
    context.applySystemMapScale = () => {};
    context.setSystemMapFull = () => {};
    context.renderSubsystemInspector = () => {};
    return { c: context, a: context.state.project.architecture, run: code => vm.runInContext(code, context) };
  }
  const evidence = (overrides = {}) => ({
    from_node: "symbol:repo_map", to_node: "symbol:classify",
    from_symbol: "ranked_repo_map", to_symbol: "classify_repo_map_intent",
    from_path: "src/repo_map.rs", to_path: "src/retrieval.rs",
    kind: "calls", provider: "tree-sitter", precision: "syntax", ownership: "unique",
    from_owners: [{ component: "experience", sources: ["symbol_reference"] }],
    to_owners: [{ component: "agent", sources: ["file_reference"] }],
    from_owners_total: 1, to_owners_total: 1, ...overrides,
  });
  const edge = (overrides = {}) => ({
    from: "experience", to: "agent", from_name: "Experience Graph", to_name: "Agent Context",
    desired: true, actual: true, precision: "syntax", status: "aligned", blocking: false,
    evidence: [evidence()], evidence_total: 1, evidence_truncated: false, ...overrides,
  });
  const results = [];
  function test(name, check) {
    try { check(); results.push({ name, passed: true }); }
    catch (error) { results.push({ name, passed: false, error: error.stack }); }
  }
  test("shared ownership and ambiguous symbol targets remain unconfirmed", () => {
    const s = fixture();
    s.a.dependency_candidates = [evidence({
      ownership: "ambiguous",
      from_owners: [{ component: "agent", sources: ["ambiguous_symbol_reference"] }, { component: "experience", sources: ["trace_symbol"] }],
      from_owners_total: 2,
    })]; s.a.dependency_candidates_total = 1;
    s.run("renderArchitectureGraph()");
    const html = s.c.els.architectureGraph.innerHTML;
    assert.match(html, /Unresolved source relations/); assert.match(html, /ranked_repo_map/);
    assert.match(html, /Ownership or symbol target is unresolved/); assert.match(html, /symbol trace/);
    assert.match(html, /symbol target not proven unique/);
    assert.doesNotMatch(html, /data-dependency-component=/);
    assert.equal(s.run('architectureNodeTone(architectureData().components[0], [])'), "uncertain");
    s.a.dependency_candidates[0].from_owners = [{ component: "experience", sources: ["ambiguous_symbol_reference"] }];
    s.a.dependency_candidates[0].from_owners_total = 1;
    for (const language of ["en", "zh-CN"]) {
      s.c.state.language = language; s.run("renderArchitectureGraph()");
      const singleOwner = s.c.els.architectureGraph.innerHTML;
      assert.match(singleOwner, language === "en" ? /Ownership or symbol target is unresolved/ : /归属或符号目标尚未确认/);
      assert.doesNotMatch(singleOwner, /Shared ownership|存在共享归属/);
      assert.doesNotMatch(singleOwner, /data-dependency-component=/);
    }
  });
  test("confirmed source evidence preserves precision, owner source, paths and bounded counts", () => {
    const s = fixture();
    s.a.dependencies = [edge({ evidence_total: 12, evidence_truncated: true })];
    s.run("renderArchitectureGraph()");
    const html = s.c.els.architectureGraph.innerHTML;
    assert.match(html, /Source evidence 1 \/ 12/); assert.match(html, /src\/repo_map\.rs/);
    assert.match(html, /tree-sitter/); assert.match(html, /syntax/); assert.match(html, /whole-file mapping/);
    assert.match(html, /bounded sample/); assert.match(html, /not feature completion or executed verification/);
  });
  test("unobserved design and truncated graph stay unknown in the ledger and system map", () => {
    const s = fixture("zh-CN");
    s.a.dependencies = [edge({ actual: false, status: "unverified_actual", evidence: [], evidence_total: 0 })];
    s.a.subsystems = [{ id: "core", title: "Core", layer: 2, component_ids: ["agent"], components: 1, depends_on: [] }];
    s.c.state.project.structure.truncated = true;
    s.run("renderArchitectureGraph(); renderArchitectureBlueprint()");
    const html = s.c.els.architectureGraph.innerHTML;
    assert.match(html, /不能.*认定缺少实现/); assert.match(html, /本次快照不完整/);
    assert.match(html, /不代表功能完成或验证通过/);
    assert.doesNotMatch(s.c.els.architectureBlueprint.innerHTML, />健康</);
    assert.equal(s.run("subsystemBlueprintTone(architectureData().subsystems[0])"), "uncertain");
  });
  test("candidate provenance and missing ownership never enter actual or risk filters", () => {
    const s = fixture();
    s.a.dependency_candidates = [evidence({ ownership: "unowned", to_owners: [], to_owners_total: 0, precision: "declared" })];
    s.a.dependency_candidates_total = 1;
    s.c.state.architectureMode = "unverified"; s.run("renderArchitectureGraph()");
    assert.match(s.c.els.architectureGraph.innerHTML, /Endpoint ownership incomplete/);
    assert.match(s.c.els.architectureGraph.innerHTML, /declared/);
    for (const mode of ["actual", "risk"]) {
      s.c.state.architectureMode = mode; s.run("renderArchitectureGraph()");
      assert.doesNotMatch(s.c.els.architectureGraph.innerHTML, /ranked_repo_map|data-dependency-component=/);
    }
  });
  test("evidence text is escaped and candidate sample truncation stays visible", () => {
    const s = fixture("zh-CN");
    s.a.dependency_candidates = [evidence({ from_symbol: '<img src=x onerror="bad()">', to_path: "src/<script>.rs", from_owners_total: 20 })];
    s.a.dependency_candidates_total = 200; s.a.dependency_candidates_truncated = true;
    s.run("renderArchitectureGraph()");
    const html = s.c.els.architectureGraph.innerHTML;
    assert.match(html, /&lt;img/); assert.match(html, /&lt;script&gt;/);
    assert.doesNotMatch(html, /<img|<script/); assert.match(html, /1\/20/);
    assert.match(html, /1 \/ 200/); assert.match(html, /候选明细已截断/);
  });
  test("component inspection includes related candidates and does not label them aligned", () => {
    const s = fixture();
    s.a.dependencies = [edge()];
    s.a.dependency_candidates = [evidence({ ownership: "ambiguous", from_owners: [{ component: "agent", sources: ["symbol_reference"] }, { component: "experience", sources: ["symbol_reference"] }] })];
    s.a.dependency_candidates_total = 1;
    s.run("renderComponentInspector()");
    const html = s.c.els.componentInspector.innerHTML;
    assert.match(html, /Unresolved source relations/); assert.match(html, /Needs stronger evidence/);
    assert.doesNotMatch(html, />Architecture aligned</);
    assert.match(html, /classify_repo_map_intent/);
  });
  test("owner sample truncation keeps omitted component membership unknown", () => {
    for (const language of ["en", "zh-CN"]) {
      const s = fixture(language);
      s.c.state.selectedComponent = "workspace";
      s.a.dependencies = [edge({
        from: "workspace", from_name: "Workspace",
        evidence: [evidence({
          from_symbol: "workspace_entry", to_symbol: "agent_helper",
          from_path: "src/workspace.rs", to_path: "src/agent.rs",
          from_owners: [{ component: "workspace", sources: ["symbol_reference"] }],
        })],
      })];
      s.a.subsystems = [{ id: "ws", title: "Workspace", component_ids: ["workspace"], components: 1 }];
      s.a.dependency_candidates = [evidence({ ownership: "ambiguous", from_owners_total: 20 })];
      s.a.dependency_candidates_total = 1;
      s.a.dependency_candidates_truncated = false;
      assert.equal(s.run('architectureDependencyCandidates("workspace").length'), 0);
      assert.equal(s.run('architectureNodeTone(architectureData().components[2], architectureData().dependencies)'), "uncertain");
      assert.equal(s.run("subsystemBlueprintTone(architectureData().subsystems[0])"), "uncertain");
      s.run("renderComponentInspector()");
      const html = s.c.els.componentInspector.innerHTML;
      assert.match(html, /Needs stronger evidence/);
      assert.doesNotMatch(html, />Architecture aligned</);
      assert.match(html, language === "en" ? /Owner samples are incomplete; membership is unknown/ : /归属样本不完整；是否涉及当前组件尚不明确/);
      assert.doesNotMatch(html, /ranked_repo_map|classify_repo_map_intent/);
    }
  });
  test("older snapshots retain missing evidence details without fabricating source proof", () => {
    const s = fixture();
    const oldEdge = edge(); delete oldEdge.evidence; delete oldEdge.evidence_total; delete oldEdge.evidence_truncated;
    s.a.dependencies = [oldEdge]; delete s.a.dependency_candidates; delete s.a.dependency_candidates_total;
    s.run("renderArchitectureGraph()");
    assert.match(s.c.els.architectureGraph.innerHTML, /no source evidence details/);
    assert.doesNotMatch(s.c.els.architectureGraph.innerHTML, /src\/repo_map\.rs/);
  });
  test("strong undeclared evidence remains the only row in the risk filter", () => {
    const s = fixture();
    s.a.dependencies = [
      edge({ desired: false, precision: "semantic", status: "undeclared_actual", blocking: true }),
      edge({ from: "workspace", desired: false, status: "observed_actual" }),
    ];
    s.c.state.architectureMode = "risk"; s.run("renderArchitectureGraph()");
    const html = s.c.els.architectureGraph.innerHTML;
    assert.equal((html.match(/data-dependency-component=/g) || []).length, 1);
    assert.match(html, /undeclared_actual/); assert.doesNotMatch(html, /observed_actual/);
  });
  process.stdout.write(JSON.stringify({ results, passed: results.filter(item => item.passed).length }) + "\n");
  if (results.some(item => !item.passed)) process.exitCode = 1;
}
runArchitectureEvidenceRegressions();

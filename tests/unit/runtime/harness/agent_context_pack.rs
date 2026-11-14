//! Agent Context pack compilation against a real budget and SHA.
use super::*;

#[test]
fn agent_context_compiles_edit_ready_pack_with_real_budget_and_sha() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".wcode/design")).unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(root.path().join("Cargo.lock"), "# fixture lock\n").unwrap();
    fs::write(
        root.path().join("AGENTS.md"),
        format!(
            "# Rules\nReuse existing helpers. {}\n",
            "bounded guidance ".repeat(80)
        ),
    )
    .unwrap();
    fs::write(
        root.path().join(".wcode/project.yaml"),
        "schema_version: 1\nname: demo\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".wcode/design/requirements.yaml"),
        r#"- schema_version: 1
  id: REQ-CONTEXT-001
  title: Compact feature context
  intent: Route feature work through the existing helper.
  priority: high
  implemented_by:
    - component:feature
  acceptance:
    - AC-CONTEXT-001
  constraints: []
  risk: {}
"#,
    )
    .unwrap();
    fs::write(
        root.path().join(".wcode/design/components.yaml"),
        r#"- schema_version: 1
  id: component:feature
  name: Feature entry
  responsibilities:
    - reuse the helper boundary
  depends_on: []
  constraints: []
  implementation:
    - kind: symbol
      path: src/lib.rs
      symbol: feature_entry
"#,
    )
    .unwrap();
    fs::write(
        root.path().join(".wcode/design/acceptance.yaml"),
        r#"- schema_version: 1
  id: AC-CONTEXT-001
  title: Feature flow stays verified
  statement: The feature entry continues to call the helper.
  verification:
    - kind: test
      path: src/lib.rs
      symbol: tests::feature_flow
"#,
    )
    .unwrap();
    fs::write(
        root.path().join("src/lib.rs"),
        r#"pub fn feature_entry() {
    helper();
}

fn helper() {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn feature_flow() {
        feature_entry();
    }
}
"#,
    )
    .unwrap();
    fs::write(
        root.path().join("src/worker.rs"),
        "pub fn batch_worker() { let _ = 1 + 1; }\n",
    )
    .unwrap();

    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let pack = harness
        .agent_context("demo", &workspace, "feature entry", 1_000, &[])
        .unwrap();

    let actual_bytes = serde_json::to_vec(&pack).unwrap().len() as u64;
    assert!(pack["estimated_tokens"].as_u64().unwrap() <= 1_000);
    assert!(pack["serialized_bytes"].as_u64().unwrap() <= 4_000);
    assert_eq!(pack["serialized_bytes"], actual_bytes);
    assert_eq!(pack["estimated_tokens"], actual_bytes.div_ceil(4));
    assert!(pack["project"].get("root").is_none());
    assert_eq!(pack["budget_mode"], "explicit");
    assert_eq!(
        pack["truncated"], true,
        "finalization must retain earlier truncation"
    );
    assert_eq!(pack["budget"], 1_000);
    assert_eq!(pack["requested_budget"], 1_000);
    assert!(pack["targets"].as_array().unwrap().iter().any(|target| {
        target["qualified_name"] == "feature_entry" && target["path"] == "src/lib.rs"
    }));
    let file = pack["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"] == "src/lib.rs")
        .unwrap();
    assert_eq!(file["sha256"].as_str().unwrap().len(), 64);
    assert!(pack["tests"].as_array().unwrap().iter().any(|test| {
        test["target"] == "src/lib.rs::tests::feature_flow" && test["resolved"] == true
    }));
    assert!(pack["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| check["id"] == "rust-check"));
    let repo_items = pack["repo_map"]["items"].as_array().unwrap();
    assert!(repo_items
        .iter()
        .any(|item| item["qualified_name"] == "feature_entry"));
    assert_eq!(
        pack["provenance_defaults"]["repo_map_symbols"]["precision"],
        "syntax"
    );
    assert!(repo_items
        .iter()
        .all(|item| item.get("precision").is_none()));
    let hot_source = pack["hot_source"].as_array().unwrap();
    assert!((1..=2).contains(&hot_source.len()));
    assert_eq!(hot_source[0]["qualified_name"], "feature_entry");
    assert!(hot_source[0]["body"]["content"]
        .as_str()
        .unwrap()
        .contains("helper();"));
    assert_eq!(
        pack["provenance_defaults"]["hot_source"]["precision"],
        "syntax"
    );
    assert!(hot_source[0].get("precision").is_none());
    assert_eq!(pack["readiness"]["edit"], "ready");
    assert_eq!(pack["readiness"]["verify"], "ready");
    assert_eq!(pack["readiness"]["graph_precision"], "syntax");
    assert_eq!(pack["readiness"]["recommended_edit_tool"], "apply_edits");
    assert_eq!(pack["readiness"]["parallelism"]["strategy"], "single_lane");
    assert_eq!(pack["readiness"]["parallelism"]["candidate_lanes"], 1);
    assert_eq!(pack["readiness"]["change_strategy"], "minimal_patch");
    assert_eq!(
        pack["readiness"]["complexity_budget"]["new_production_files"],
        0
    );
    assert_eq!(
        pack["readiness"]["complexity_budget"]["new_abstractions"],
        0
    );
    assert_eq!(
        pack["readiness"]["complexity_budget"]["new_config_knobs"],
        0
    );
    assert_eq!(
        pack["readiness"]["next_actions"],
        json!(["apply_edits", "review_changes", "verify_project"])
    );
    assert!(pack["timing"]["build_ms"].as_u64().is_some());
    assert!(pack["timing"]["software_context_ms"].as_u64().is_some());
    assert!(pack["readiness"]["advisories"]
        .as_array()
        .unwrap()
        .iter()
        .any(|advisory| advisory == "syntax_only_relationships"));
    assert_eq!(pack["repo_map"]["cache_hit"], false);
    assert_eq!(pack["repo_map"]["scope_path"], "src");
    assert_eq!(pack["repo_map"]["files_indexed"], 2);
    assert!(pack["context_bytes_avoided"].as_u64().unwrap() > 0);
    assert!(pack["context_reduction_percent"].as_f64().unwrap() > 20.0);

    let cached = harness
        .agent_context("demo", &workspace, "feature entry", 2_000, &[])
        .unwrap();
    assert_eq!(cached["repo_map"]["cache_hit"], true);

    let parallel_task = harness
        .agent_context("demo", &workspace, "feature_entry batch_worker", 3_000, &[])
        .unwrap();
    assert_eq!(
        parallel_task["readiness"]["parallelism"]["strategy"],
        "parallel_required"
    );
    assert_eq!(parallel_task["readiness"]["parallelism"]["required"], true);
    assert!(
        parallel_task["readiness"]["parallelism"]["candidate_lanes"]
            .as_u64()
            .unwrap()
            >= 2
    );
    assert_eq!(
        parallel_task["readiness"]["parallelism"]["execution_bias"],
        "parallel_first"
    );
    assert!(
        parallel_task["readiness"]["parallelism"]["recommended_concurrency"]
            .as_u64()
            .unwrap()
            >= 2
    );
    assert!(parallel_task["readiness"]["parallelism"]["instruction"]
        .as_str()
        .unwrap()
        .contains("required"));
    assert_eq!(
        parallel_task["readiness"]["parallelism"]["serialize_only"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        parallel_task["readiness"]["change_strategy"],
        "localized_refactor"
    );
    assert_eq!(
        parallel_task["readiness"]["complexity_budget"]["new_production_files"],
        1
    );
    let compact_parallel_task = harness
        .agent_context("demo", &workspace, "feature_entry batch_worker", 1_000, &[])
        .unwrap();
    assert_eq!(
        compact_parallel_task["readiness"]["parallelism"]["strategy"],
        "parallel_required"
    );
    assert!(
        compact_parallel_task["readiness"]["parallelism"]["candidate_lanes"]
            .as_u64()
            .unwrap()
            >= 2
    );
    assert_eq!(
        compact_parallel_task["readiness"]["change_strategy"],
        "localized_refactor"
    );

    crate::worklist::update(
        &workspace,
        crate::worklist::WorklistUpdate {
            expected_revision: 0,
            goal: Some(format!(
                "resume guarded work {}",
                "without budget bypass ".repeat(20)
            )),
            restart: false,
            items: (0..8)
                .map(|index| crate::worklist::WorkItemPatch {
                    write_paths: None,
                    id: format!("item-{index}"),
                    title: Some(format!(
                        "Preserve this unfinished item {index}: {}",
                        "bounded progress context ".repeat(6)
                    )),
                    status: None,
                    depends_on: None,
                    note: None,
                })
                .collect(),
        },
    )
    .unwrap();
    let worklist_pack = harness
        .agent_context("demo", &workspace, "feature entry", 1_000, &[])
        .unwrap();
    assert!(worklist_pack["worklist"].is_object());
    assert_eq!(worklist_pack["worklist"]["revision"], 1);
    assert_eq!(worklist_pack["worklist"]["truncated"], true);
    let durable = crate::worklist::status(&workspace).unwrap();
    assert_eq!(durable["items"].as_array().unwrap().len(), 8);
    assert!(durable["goal"].as_str().unwrap().len() > 120);
    assert!(worklist_pack["estimated_tokens"].as_u64().unwrap() <= 1_000);
    assert!(serde_json::to_vec(&worklist_pack).unwrap().len() <= 4_000);

    let relationship_task = harness
        .agent_context(
            "demo",
            &workspace,
            "feature entry callers and references",
            2_000,
            &[],
        )
        .unwrap();
    assert!(relationship_task["readiness"]["next_actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|action| action == "semantic_navigation"));
    assert!(relationship_task["readiness"]["advisories"]
        .as_array()
        .unwrap()
        .iter()
        .any(|advisory| advisory == "semantic_navigation_recommended"));
    assert!(cached["estimated_tokens"].as_u64().unwrap() <= 2_000);
    let cached_items = cached["repo_map"]["items"].as_array().unwrap();
    let helper = cached_items
        .iter()
        .find(|item| item["qualified_name"] == "helper")
        .unwrap();
    assert_eq!(helper["reason"], "callee_of_direct");
    assert!(helper["signature"].as_str().unwrap().contains("fn helper"));
    assert!(helper["relationships"]
        .as_array()
        .unwrap()
        .iter()
        .any(|relation| {
            relation["relation"] == "callee_of_direct" && relation["precision"] == "syntax"
        }));

    let adaptive = harness
        .agent_context("demo", &workspace, "feature entry", 0, &[])
        .unwrap();
    assert_eq!(adaptive["budget_mode"], "adaptive");
    assert!(adaptive["requested_budget"].is_null());
    let adaptive_budget = adaptive["budget"].as_u64().unwrap();
    assert!((1_200..=1_800).contains(&adaptive_budget));
    assert!(adaptive["estimated_tokens"].as_u64().unwrap() <= adaptive_budget);
    assert_eq!(adaptive["readiness"]["edit"], "ready");

    let compact_long_query = harness
        .agent_context(
            "demo",
            &workspace,
            "feature_entry 请继续深入优化代码智能上下文召回精度缓存新鲜度验证反馈并检查跨模块边界性能一致性可维护性错误恢复并尽量减少无效扫描和上下文浪费同时保留现有安全边界与确定性验证能力",
            0,
            &[],
        )
        .unwrap();
    assert_eq!(compact_long_query["budget_mode"], "adaptive");
    assert!(compact_long_query["budget"].as_u64().unwrap() > adaptive_budget);

    let complex_scopes = vec!["customer-domain".to_owned(), "platform-domain".to_owned()];
    let complex = harness
        .agent_context(
            "demo",
            &workspace,
            "feature entry refactor architecture helper compatibility verification cross module behavior reusable boundary performance safety reliability migration",
            0,
            &complex_scopes,
        )
        .unwrap();
    assert_eq!(complex["budget_mode"], "adaptive");
    let complex_budget = complex["budget"].as_u64().unwrap();
    assert!(complex_budget > adaptive_budget);
    assert!(complex_budget <= 4_000);
    assert!(complex["estimated_tokens"].as_u64().unwrap() <= complex_budget);

    let source_sha256 = workspace.read_file("src/lib.rs", 1, None).unwrap().sha256;
    harness
        .graph_provider_import(
            &workspace,
            crate::graph::GraphProviderImport {
                provider: "lsp:fixture".into(),
                precision: GraphPrecision::Semantic,
                revision: "semantic-fixture-1".into(),
                nodes: vec![
                    crate::graph::GraphImportNode {
                        id: "semantic:feature-entry".into(),
                        kind: NodeKind::Function,
                        label: "feature_entry".into(),
                        attributes: BTreeMap::from([
                            ("path".into(), json!("src/lib.rs")),
                            ("source_sha256".into(), json!(source_sha256.clone())),
                            ("name".into(), json!("feature_entry")),
                            ("qualified_name".into(), json!("feature_entry")),
                        ]),
                    },
                    crate::graph::GraphImportNode {
                        id: "semantic:helper".into(),
                        kind: NodeKind::Function,
                        label: "helper".into(),
                        attributes: BTreeMap::from([
                            ("path".into(), json!("src/lib.rs")),
                            ("source_sha256".into(), json!(source_sha256)),
                            ("name".into(), json!("helper")),
                            ("qualified_name".into(), json!("helper")),
                        ]),
                    },
                ],
                edges: vec![crate::graph::GraphImportEdge {
                    from: "semantic:feature-entry".into(),
                    to: "semantic:helper".into(),
                    kind: EdgeKind::Calls,
                }],
            },
        )
        .unwrap();
    let semantic = harness
        .agent_context("demo", &workspace, "feature entry", 2_000, &[])
        .unwrap();
    assert_eq!(semantic["repo_map"]["provider"], "wcode-composite");
    assert_eq!(semantic["repo_map"]["precision"], "semantic");
    assert_eq!(semantic["readiness"]["graph_precision"], "semantic");
    let semantic_helper = semantic["repo_map"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["qualified_name"] == "helper")
        .unwrap();
    assert_eq!(semantic_helper["reason"], "callee_of_direct");
    assert!(semantic_helper["relationships"]
        .as_array()
        .unwrap()
        .iter()
        .any(|relation| {
            relation["provider"] == "lsp:fixture" && relation["precision"] == "semantic"
        }));

    let changed_source = format!(
        "{}\n// semantic provider should become stale after this edit\n",
        fs::read_to_string(root.path().join("src/lib.rs")).unwrap()
    );
    fs::write(root.path().join("src/lib.rs"), changed_source).unwrap();
    let stale_semantic = harness
        .agent_context("demo", &workspace, "feature entry", 2_000, &[])
        .unwrap();
    assert_eq!(stale_semantic["repo_map"]["provider"], "tree-sitter");
    assert_eq!(stale_semantic["repo_map"]["precision"], "syntax");
    assert_eq!(stale_semantic["readiness"]["graph_precision"], "syntax");

    let read_only_workspace = Workspace::new(root.path(), false, true).unwrap();
    let read_only = harness
        .agent_context("demo", &read_only_workspace, "feature entry", 2_000, &[])
        .unwrap();
    assert_eq!(read_only["project"]["write_enabled"], false);
    assert_eq!(read_only["readiness"]["edit"], "read_only_workspace");
    assert_eq!(read_only["readiness"]["next_actions"], json!([]));
    assert!(read_only["readiness"]["advisories"]
        .as_array()
        .unwrap()
        .iter()
        .any(|advisory| advisory == "workspace_write_disabled"));
}

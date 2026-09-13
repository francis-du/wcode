use super::*;

#[test]
fn contract_bridges_propagate_static_codegen_inputs_to_consumer_islands_only() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".git")).unwrap();
    fs::create_dir_all(root.path().join("contracts/proto")).unwrap();
    fs::create_dir_all(root.path().join("web/src/generated")).unwrap();
    fs::create_dir_all(root.path().join("sdk/src/generated")).unwrap();
    fs::create_dir_all(root.path().join("portal/src")).unwrap();
    fs::create_dir_all(root.path().join("go-client/gen")).unwrap();
    fs::create_dir_all(root.path().join("server/src")).unwrap();
    fs::write(
        root.path().join("server/Cargo.toml"),
        "[package]\nname='server'\nversion='0.1.0'\n",
    )
    .unwrap();
    fs::write(
        root.path().join("server/src/lib.rs"),
        "pub fn server() {}\n",
    )
    .unwrap();
    fs::write(
        root.path().join("contracts/package.json"),
        r#"{"name":"contracts","scripts":{"test":"node --test"}}"#,
    )
    .unwrap();
    fs::write(
        root.path().join("web/package.json"),
        r#"{"scripts":{"lint":"eslint .","test":"node --test"}}"#,
    )
    .unwrap();
    fs::write(
        root.path().join("web/src/app.ts"),
        "export const app = 1;\n",
    )
    .unwrap();
    fs::write(
        root.path().join("sdk/package.json"),
        r#"{"scripts":{"lint":"eslint .","test":"node --test"}}"#,
    )
    .unwrap();
    fs::write(
        root.path().join("sdk/src/index.ts"),
        "export const sdk = 1;\n",
    )
    .unwrap();
    fs::write(
        root.path().join("portal/package.json"),
        r#"{"dependencies":{"sdk-client":"file:../sdk"},"scripts":{"lint":"eslint .","test":"node --test"}}"#,
    )
    .unwrap();
    fs::write(
        root.path().join("portal/src/index.ts"),
        "export const portal = 1;\n",
    )
    .unwrap();
    fs::write(
        root.path().join("go-client/go.mod"),
        "module example.com/client\n",
    )
    .unwrap();
    fs::write(root.path().join("go-client/main.go"), "package client\n").unwrap();

    fs::write(
        root.path().join("contracts/openapi.yaml"),
        "openapi: 3.0.0\ninfo: {title: Demo, version: 1.0.0}\npaths: {}\n",
    )
    .unwrap();
    fs::write(
        root.path().join("sdk/openapi-generator.yaml"),
        "generatorName: typescript-fetch\ninputSpec: ../contracts/openapi.yaml\noutputDir: src/generated\n",
    )
    .unwrap();
    fs::write(
        root.path().join("contracts/schema.graphql"),
        "type Query { ping: String! }\n",
    )
    .unwrap();
    fs::write(
        root.path().join("web/codegen.yml"),
        "schema: ../contracts/schema.graphql\ngenerates:\n  src/generated/types.ts:\n    plugins: [typescript]\n",
    )
    .unwrap();
    fs::write(
        root.path().join("contracts/proto/service.proto"),
        "syntax = \"proto3\";\npackage demo;\nmessage Request {}\n",
    )
    .unwrap();
    fs::write(
        root.path().join("contracts/buf.yaml"),
        "version: v2\nmodules:\n  - path: proto\n",
    )
    .unwrap();
    fs::write(
        root.path().join("contracts/buf.gen.yaml"),
        "version: v2\nplugins:\n  - remote: buf.build/protocolbuffers/go\n    out: ../go-client/gen\n",
    )
    .unwrap();

    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let (profile, _) = harness.load_project_profile(&workspace).unwrap();
    assert_eq!(profile.contracts.provider, "contract-config");
    assert_eq!(profile.contracts.precision, "structural");
    assert!(profile.contracts.bridges.iter().any(|bridge| {
        bridge.kind == "openapi_codegen"
            && bridge.source == "contracts/openapi.yaml"
            && bridge.consumer_island == "sdk"
    }));
    assert!(profile.contracts.bridges.iter().any(|bridge| {
        bridge.kind == "graphql_schema_codegen"
            && bridge.source == "contracts/schema.graphql"
            && bridge.consumer_island == "web"
    }));
    assert!(profile.contracts.bridges.iter().any(|bridge| {
        bridge.kind == "protobuf_codegen"
            && bridge.source == "contracts/proto"
            && bridge.source_kind == "directory"
            && bridge.consumer_island == "go-client"
    }));

    for (changed, expected, excluded) in [
        ("contracts/openapi.yaml", "sdk", "server"),
        ("contracts/schema.graphql", "web", "sdk"),
        ("contracts/proto/service.proto", "go-client", "web"),
    ] {
        let snapshot = json!({
            "available": true,
            "truncated": false,
            "files": [{"path": changed}]
        });
        let plan = super::super::harness_profile::verification_checks_for_snapshot(
            &profile,
            Some(&snapshot),
            "full",
        );
        assert!(
            plan.iter().any(|check| check.island == expected),
            "{changed} must propagate to {expected}"
        );
        assert!(
            !plan.iter().any(|check| check.island == excluded),
            "{changed} must not guess an unrelated {excluded} consumer"
        );
    }

    let openapi_snapshot = json!({
        "available": true,
        "truncated": false,
        "files": [{"path":"contracts/openapi.yaml"}]
    });
    let impact = super::super::harness_profile::verification_impact_for_snapshot(
        &profile,
        Some(&openapi_snapshot),
    );
    assert!(impact.selective);
    assert_eq!(
        impact.affected_islands,
        vec![
            "contracts".to_owned(),
            "portal".to_owned(),
            "sdk".to_owned()
        ]
    );
    assert!(impact.reasons.iter().any(|reason| {
        reason.island == "contracts"
            && reason.kind == "direct_change"
            && reason.source == "contracts/openapi.yaml"
    }));
    assert!(impact.reasons.iter().any(|reason| {
        reason.island == "sdk"
            && reason.kind == "contract_bridge"
            && reason.relationship == "openapi_codegen"
            && reason.evidence == "sdk/openapi-generator.yaml:inputSpec+outputDir"
    }));
    assert!(impact.reasons.iter().any(|reason| {
        reason.island == "portal"
            && reason.kind == "manifest_dependency"
            && reason.source == "sdk"
            && reason.relationship == "package_local"
            && reason.evidence == "package.json:dependencies.sdk-client"
    }));
    assert!(!impact
        .reasons
        .iter()
        .any(|reason| matches!(reason.island.as_str(), "web" | "go-client")));

    let sdk_snapshot = json!({
        "available": true,
        "truncated": false,
        "files": [{"path":"sdk/src/index.ts"}]
    });
    let sdk_plan = super::super::harness_profile::verification_checks_for_snapshot(
        &profile,
        Some(&sdk_snapshot),
        "full",
    );
    assert!(sdk_plan.iter().any(|check| check.island == "sdk"));
    assert!(sdk_plan.iter().any(|check| check.island == "portal"));
    assert!(!sdk_plan.iter().any(|check| check.island == "web"));
    assert!(!sdk_plan.iter().any(|check| check.island == "go-client"));

    let changed = ["contracts/openapi.yaml".to_owned()]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    let freshness = super::super::harness_profile::contract_freshness_advisories(
        root.path(),
        &profile.contracts,
        &changed,
    );
    assert!(freshness.iter().any(|advisory| {
        advisory.kind == "openapi_codegen"
            && advisory.source == "contracts/openapi.yaml"
            && advisory.output == "sdk/src/generated"
            && advisory.consumer_island == "sdk"
    }));

    let changed_with_output = [
        "contracts/openapi.yaml".to_owned(),
        "sdk/src/generated/client.ts".to_owned(),
    ]
    .into_iter()
    .collect::<std::collections::BTreeSet<_>>();
    assert!(
        super::super::harness_profile::contract_freshness_advisories(
            root.path(),
            &profile.contracts,
            &changed_with_output,
        )
        .is_empty()
    );

    let graphql_only = ["contracts/schema.graphql".to_owned()]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    assert!(
        super::super::harness_profile::contract_freshness_advisories(
            root.path(),
            &profile.contracts,
            &graphql_only,
        )
        .is_empty()
    );
}

#[tokio::test]
async fn change_review_advises_only_when_existing_generated_output_has_no_worktree_change() {
    use std::process::Command;

    let root = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(root.path())
            .status()
            .expect("git must be available for contract freshness review")
    };
    assert!(git(&["init", "-q"]).success());
    assert!(git(&["config", "user.email", "wcode@example.test"]).success());
    assert!(git(&["config", "user.name", "wcode test"]).success());
    fs::create_dir_all(root.path().join("contracts")).unwrap();
    fs::create_dir_all(root.path().join("sdk/src/generated")).unwrap();
    fs::write(
        root.path().join("contracts/openapi.yaml"),
        "openapi: 3.0.0\ninfo: {title: Demo, version: 1.0.0}\npaths: {}\n",
    )
    .unwrap();
    fs::write(
        root.path().join("sdk/openapi-generator.yaml"),
        "generatorName: typescript-fetch\ninputSpec: ../contracts/openapi.yaml\noutputDir: src/generated\n",
    )
    .unwrap();
    fs::write(
        root.path().join("sdk/package.json"),
        r#"{"scripts":{"test":"node --test"}}"#,
    )
    .unwrap();
    fs::write(
        root.path().join("sdk/src/index.ts"),
        "export const sdk = 1;\n",
    )
    .unwrap();
    fs::write(
        root.path().join("sdk/src/generated/client.ts"),
        "export const generated = 1;\n",
    )
    .unwrap();
    assert!(git(&["add", "."]).success());
    assert!(git(&["-c", "commit.gpgsign=false", "commit", "-qm", "initial"]).success());

    fs::write(
        root.path().join("contracts/openapi.yaml"),
        "openapi: 3.0.0\ninfo: {title: Demo, version: 2.0.0}\npaths: {}\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), false, true).unwrap();
    let workspace_id = "freshness-review".to_owned();
    let harness = ToolHarness::new(4).unwrap();
    let monitor = TaskMonitor::new([workspace_id.clone()]);
    let report = harness
        .review_changes(workspace_id.clone(), &workspace, 30, &monitor)
        .await
        .unwrap();
    let finding = report
        .findings
        .iter()
        .find(|finding| finding.code == "generated-artifact-freshness")
        .expect("changed contract with existing untouched output must be surfaced");
    assert_eq!(finding.severity, "info");
    assert!(finding
        .message
        .contains("not proof that the output is stale"));
    assert!(finding
        .paths
        .iter()
        .any(|path| path == "contracts/openapi.yaml"));
    assert!(finding.paths.iter().any(|path| path == "sdk/src/generated"));

    fs::write(
        root.path().join("sdk/src/generated/client.ts"),
        "export const generated = 2;\n",
    )
    .unwrap();
    let report = harness
        .review_changes(workspace_id, &workspace, 30, &monitor)
        .await
        .unwrap();
    assert!(!report
        .findings
        .iter()
        .any(|finding| finding.code == "generated-artifact-freshness"));
}

#[test]
fn contract_bridges_ignore_remote_escaping_and_dynamic_configs_without_execution() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".git")).unwrap();
    fs::create_dir_all(root.path().join("web/src/generated")).unwrap();
    fs::create_dir_all(root.path().join("contracts")).unwrap();
    fs::write(
        root.path().join("web/package.json"),
        r#"{"scripts":{"lint":"eslint .","test":"node --test"}}"#,
    )
    .unwrap();
    fs::write(
        root.path().join("web/src/app.ts"),
        "export const app = 1;\n",
    )
    .unwrap();
    fs::write(
        root.path().join("web/codegen.yml"),
        "schema: https://example.com/schema.graphql\ngenerates:\n  src/generated/types.ts:\n    plugins: [typescript]\n",
    )
    .unwrap();
    fs::write(
        root.path().join("web/codegen.ts"),
        "throw new Error('must never execute dynamic codegen config');\n",
    )
    .unwrap();
    fs::write(
        root.path().join("web/openapi-generator.yaml"),
        "generatorName: typescript-fetch\ninputSpec: ../../outside.yaml\noutputDir: src/generated\n",
    )
    .unwrap();

    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(2).unwrap();
    let (profile, _) = harness.load_project_profile(&workspace).unwrap();
    assert!(profile.contracts.bridges.is_empty());
    assert!(profile.contracts.diagnostics.iter().any(|diagnostic| {
        diagnostic.path == "web/codegen.ts"
            && diagnostic.reason == "dynamic_graphql_config_not_executed"
    }));
    assert!(profile
        .contracts
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.reason == "no_local_contract_bridge"));
}

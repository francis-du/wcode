use super::corpus::{base_case, Identity};
use super::scoring::{digest, score};
use crate::code_index::CodeIndex;
use crate::graph::{EdgeKind, GraphPrecision};
use crate::harness::ToolHarness;
use crate::workspace::Workspace;
use anyhow::{ensure, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::{collections::BTreeSet, fs, sync::Barrier, thread};

#[derive(Debug, Serialize)]
pub(super) struct Control {
    pub id: String,
    pub passed: bool,
    pub observation: Option<Value>,
    pub error: Option<String>,
}

fn record(id: &str, result: Result<Value>) -> Control {
    match result {
        Ok(value) => Control {
            id: id.into(),
            passed: true,
            observation: Some(value),
            error: None,
        },
        Err(error) => Control {
            id: id.into(),
            passed: false,
            observation: None,
            error: Some(format!("{error:#}")),
        },
    }
}

fn syntax_graph(language: &str) -> Result<Value> {
    let case = base_case(language);
    let (_root, workspace) = case.instantiate();
    let graph = CodeIndex::new()?.software_graph("fitness-graph", &workspace, "src", 100, 100)?;
    let path = case.required[0].identity.path.as_str();
    let expected: BTreeSet<_> = [
        (
            Identity::new(path, "refresh_session"),
            Identity::new(path, "cleanup_if_owner"),
        ),
        (
            Identity::new(path, "observe_epoch"),
            Identity::new(path, "refresh_session"),
        ),
    ]
    .into_iter()
    .collect();
    let mut observed = BTreeSet::new();
    for edge in graph
        .graph
        .edges
        .iter()
        .filter(|edge| edge.kind == EdgeKind::Calls)
    {
        ensure!(
            edge.provenance.precision == GraphPrecision::Syntax,
            "call precision was promoted"
        );
        let from = &graph.graph.nodes[&edge.from];
        let to = &graph.graph.nodes[&edge.to];
        let id = |node: &crate::graph::GraphNode| {
            Identity::new(
                node.attributes
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
                &node.label,
            )
        };
        observed.insert((id(from), id(to)));
    }
    let hits = observed.intersection(&expected).count();
    ensure!(
        observed == expected,
        "{language}: observed {observed:?}; gold {expected:?}"
    );
    ensure!(
        !graph.truncated && !graph.scan_truncated,
        "tiny graph unexpectedly truncated"
    );
    Ok(
        json!({"language":language,"provider":"tree-sitter","precision":"syntax",
        "gold_edges":expected.len(),"observed_edges":observed.len(),"hits":hits,
        "precision_value":hits as f64 / observed.len() as f64,"recall":hits as f64 / expected.len() as f64}),
    )
}

fn guarded_edit_and_invalidation() -> Result<Value> {
    let mut case = base_case("rust");
    let (_root, workspace) = case.instantiate();
    let harness = ToolHarness::new(4)?;
    let old = harness.agent_context("fitness", &workspace, &case.query, 4_000, &[])?;
    let path = "src/session.rs";
    let sha = digest(case.files[path].as_bytes());
    let edit = workspace.replace_text(path, "old == current", "old != current", &sha)?;
    ensure!(
        edit.sha256_after != sha,
        "edit did not change source identity"
    );
    let current = workspace.read_file(path, 1, None)?;
    let stale = workspace.replace_text(path, "old != current", "false", &sha);
    ensure!(stale.is_err(), "stale write accepted");
    ensure!(
        workspace.read_file(path, 1, None)?.sha256 == current.sha256,
        "rejected edit changed file"
    );
    *case.files.get_mut(path).unwrap() =
        case.files[path].replace("old == current", "old != current");
    case.required[0].fragment = case.required[0]
        .fragment
        .replace("old == current", "old != current");
    ensure!(
        score(&old, &case).fresh_sha_hits == 0,
        "evaluator credited stale SHA"
    );
    let new = harness.agent_context("fitness", &workspace, &case.query, 4_000, &[])?;
    let result = score(&new, &case);
    ensure!(
        result.fresh_sha_hits == 2 && result.complete_body_hits == 2,
        "warm context did not refresh edited source: {result:?}"
    );
    Ok(
        json!({"stale_write_rejected":true,"warm_source_refreshed":true,"old_evidence_rejected":true}),
    )
}

fn read_only() -> Result<Value> {
    let mut case = base_case("rust");
    case.writable = false;
    let (_root, workspace) = case.instantiate();
    let sha = digest(case.files["src/session.rs"].as_bytes());
    ensure!(
        workspace
            .replace_text("src/session.rs", "old == current", "true", &sha)
            .is_err(),
        "read-only edit accepted"
    );
    let pack =
        ToolHarness::new(4)?.agent_context("fitness", &workspace, &case.query, 4_000, &[])?;
    ensure!(
        !score(&pack, &case).all_required_edit_inputs,
        "read-only fixture marked writable"
    );
    ensure!(
        pack["readiness"]["edit"] != "ready",
        "readiness claimed writable workspace"
    );
    ensure!(
        workspace.read_file("src/session.rs", 1, None)?.sha256 == sha,
        "read-only source changed"
    );
    Ok(json!({"write_rejected":true,"source_unchanged":true}))
}

fn concurrent_edits() -> Result<Value> {
    let case = base_case("rust");
    let (_root, workspace) = case.instantiate();
    let sha = digest(case.files["src/session.rs"].as_bytes());
    let barrier = Barrier::new(2);
    let outcomes = thread::scope(|scope| {
        let handles: Vec<_> = ["false", "true"]
            .into_iter()
            .map(|replacement| {
                let workspace = &workspace;
                let barrier = &barrier;
                let sha = &sha;
                scope.spawn(move || {
                    barrier.wait();
                    workspace
                        .replace_text("src/session.rs", "old == current", replacement, sha)
                        .is_ok()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    ensure!(
        outcomes.iter().filter(|ok| **ok).count() == 1,
        "same-SHA race allowed {outcomes:?}"
    );
    let current = workspace.read_file("src/session.rs", 1, None)?;
    ensure!(current.sha256 != sha, "winning edit was lost");
    Ok(json!({"attempts":2,"committed":1,"rejected":1}))
}

fn workspace_boundary() -> Result<Value> {
    let case = base_case("rust");
    let (_root, workspace) = case.instantiate();
    let external = tempfile::tempdir()?;
    let path = external.path().join("sentinel.txt");
    fs::write(&path, "unchanged")?;
    let absolute = path.to_str().unwrap();
    ensure!(
        workspace.read_file(absolute, 1, None).is_err(),
        "outside read accepted"
    );
    ensure!(
        workspace.create_file(absolute, "changed").is_err(),
        "outside write accepted"
    );
    ensure!(
        workspace.read_file("../sentinel.txt", 1, None).is_err(),
        "parent escape accepted"
    );
    ensure!(
        fs::read_to_string(path)? == "unchanged",
        "external sentinel changed"
    );
    Ok(
        json!({"outside_read_rejected":true,"outside_write_rejected":true,"parent_escape_rejected":true}),
    )
}

fn bounded_graph() -> Result<Value> {
    let case = base_case("rust");
    let (_root, workspace) = case.instantiate();
    let graph = CodeIndex::new()?.software_graph("fitness", &workspace, "src", 1, 1)?;
    ensure!(
        graph.truncated || graph.scan_truncated,
        "partial graph hidden as complete"
    );
    ensure!(
        graph.precision == GraphPrecision::Syntax,
        "bounded graph promoted precision"
    );
    Ok(
        json!({"truncated":graph.truncated,"scan_truncated":graph.scan_truncated,"precision":"syntax"}),
    )
}

pub(super) fn run_controls() -> Vec<Control> {
    let mut controls = Vec::new();
    for language in ["rust", "go", "typescript", "python"] {
        controls.push(record(
            &format!("syntax-graph-{language}"),
            syntax_graph(language),
        ));
    }
    controls.push(record(
        "guarded-edit-and-invalidation",
        guarded_edit_and_invalidation(),
    ));
    controls.push(record("read-only", read_only()));
    controls.push(record("concurrent-same-sha-edits", concurrent_edits()));
    controls.push(record("workspace-boundary", workspace_boundary()));
    controls.push(record("partial-graph-honesty", bounded_graph()));
    controls
}

pub(super) fn report_workspace() -> Result<Workspace> {
    Workspace::new(env!("CARGO_MANIFEST_DIR"), true, false)
}

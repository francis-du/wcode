use super::corpus::{base_case, Identity};
use super::scoring::{digest, score};
use crate::code_index::{CodeIndex, SyntaxSearchRequest};
use crate::graph::{EdgeKind, GraphPrecision};
use crate::harness::ToolHarness;
use crate::workspace::{Workspace, Workspaces};
use anyhow::{ensure, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fs,
    process::Command,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Barrier,
    },
    thread,
};

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

fn syntax_cache_pressure_coverage() -> Result<Value> {
    const FILES: usize = 96;
    let root = tempfile::tempdir()?;
    for index in 0..FILES {
        fs::write(
            root.path().join(format!("file_{index:03}.go")),
            format!("package fixture\nfunc value_{index}() int {{ return {index} }}\n"),
        )?;
    }
    let workspace = Workspace::new(root.path(), false, false)?;
    let index = CodeIndex::new()?;
    for file in 0..FILES {
        index.file_outline(
            "fitness-cache-pressure",
            &workspace,
            &format!("file_{file:03}.go"),
            8,
        )?;
    }

    let stop = AtomicBool::new(false);
    let trims = AtomicUsize::new(0);
    let barrier = Barrier::new(2);
    let result = thread::scope(|scope| {
        let trimmer = index.clone();
        let stop = &stop;
        let trims = &trims;
        let barrier = &barrier;
        let worker = scope.spawn(move || {
            barrier.wait();
            while !stop.load(Ordering::Acquire) {
                trimmer.trim_memory(false);
                trims.fetch_add(1, Ordering::Relaxed);
                thread::yield_now();
            }
        });
        barrier.wait();
        let request = SyntaxSearchRequest {
            path: ".".into(),
            node_kinds: vec!["function_declaration".into()],
            text_regex: None,
            include_comments: false,
            bug_patterns: Vec::new(),
            max_files: 1_000,
            max_results: 1_000,
        };
        let result = index.search_ast_nodes("fitness-cache-pressure", &workspace, &request);
        stop.store(true, Ordering::Release);
        ensure!(worker.join().is_ok(), "AST pressure worker panicked");
        result
    })?;
    ensure!(
        trims.load(Ordering::Relaxed) > 0,
        "AST pressure did not run"
    );
    ensure!(
        result["files_considered"].as_u64() == Some(FILES as u64),
        "syntax pressure skipped files: {result}"
    );
    ensure!(
        result["count"].as_u64() == Some(FILES as u64),
        "syntax pressure lost matches: {result}"
    );
    ensure!(
        result["files_failed"].as_u64() == Some(0),
        "AST cache eviction was misreported as source failure: {result}"
    );
    ensure!(
        result["coverage_complete"].as_bool() == Some(true),
        "complete syntax scan was downgraded under cache pressure: {result}"
    );
    Ok(json!({
        "files": FILES,
        "matches": result["count"],
        "cache_trims": trims.load(Ordering::Relaxed),
        "files_failed": result["files_failed"],
        "coverage_complete": result["coverage_complete"]
    }))
}

fn workspace_relative_verification_flight() -> Result<Value> {
    let root = tempfile::tempdir()?;
    let fixture = r#"use std::{fs::OpenOptions, io::Write, thread, time::Duration};
fn main() {
    let mut file = OpenOptions::new().create(true).append(true).open("fitness-command-count.txt").unwrap();
    file.write_all(b"1").unwrap();
    file.flush().unwrap();
    thread::sleep(Duration::from_millis(100));
}
"#;
    fs::write(root.path().join("fixture.rs"), fixture)?;
    let built = root
        .path()
        .join(format!("fitness-fixture{}", std::env::consts::EXE_SUFFIX));
    let output = Command::new("rustc")
        .arg("fixture.rs")
        .arg("-o")
        .arg(&built)
        .current_dir(root.path())
        .output()?;
    ensure!(
        output.status.success(),
        "fitness verification fixture did not compile: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::create_dir_all(root.path().join("vendor/bin"))?;
    fs::copy(&built, root.path().join("vendor/bin/phpunit"))?;

    let workspaces = Workspaces::new([root.path()], false, true)?;
    workspaces.set_all_commands_authorized(None, true)?;
    let (_, workspace) = workspaces.select(None)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let args = Vec::new();
    let (left, right) = runtime.block_on(async {
        tokio::join!(
            workspace.run_command_at_revision(
                "vendor/bin/phpunit",
                &args,
                ".",
                30,
                "fitness-relative-program"
            ),
            workspace.run_command_at_revision(
                "vendor/bin/phpunit",
                &args,
                ".",
                30,
                "fitness-relative-program"
            )
        )
    });
    let left = left?;
    let right = right?;
    ensure!(left.success && right.success, "verification flight failed");
    let executions = fs::read_to_string(root.path().join("fitness-command-count.txt"))?;
    ensure!(
        executions == "1",
        "revision flight spawned duplicate workspace-relative executables: {executions:?}"
    );
    Ok(json!({
        "platform": std::env::consts::OS,
        "workspace_relative_program": "vendor/bin/phpunit",
        "callers": 2,
        "underlying_executions": 1,
        "both_successful": true
    }))
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
    controls.push(record(
        "syntax-cache-pressure-coverage",
        syntax_cache_pressure_coverage(),
    ));
    controls.push(record(
        "workspace-relative-verification-flight",
        workspace_relative_verification_flight(),
    ));
    controls
}

pub(super) fn report_workspace() -> Result<Workspace> {
    Workspace::new(env!("CARGO_MANIFEST_DIR"), true, false)
}

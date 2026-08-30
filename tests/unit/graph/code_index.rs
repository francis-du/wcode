use super::*;
use std::fs;

#[test]
fn rust_outline_keeps_ast_and_qualifies_impl_methods() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("service.rs"),
        "pub struct Service;\n\nimpl Service {\n    pub fn run(&self) { helper(); }\n}\n\nfn helper() {}\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    let outline = index
        .file_outline("demo", &workspace, "service.rs", 100)
        .unwrap();
    let symbols = outline["symbols"].as_array().unwrap();
    assert!(symbols.iter().any(|symbol| symbol["name"] == "Service"));
    assert!(symbols
        .iter()
        .any(|symbol| symbol["qualified_name"] == "Service::run"));
    assert_eq!(outline["index"]["ast_cached_files"], 1);

    let second = index
        .file_outline("demo", &workspace, "service.rs", 100)
        .unwrap();
    assert_eq!(second["symbol_cache_hit"], true);
    assert_eq!(second["ast_cache_hit"], true);
}

#[test]
fn symbol_search_supports_multiple_languages_and_context() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("worker.py"),
        "class Worker:\n    def execute(self):\n        return helper()\n\ndef helper():\n    return 1\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("worker.ts"),
        "export class TsWorker { execute(): number { return 1; } }\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    let search = index
        .find_symbol("demo", &workspace, "execute", ".", None, 20)
        .unwrap();
    assert_eq!(search["result_count"], 2);
    let python = search["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|symbol| symbol["language"] == "python")
        .unwrap();
    let symbol_id = python["id"].as_str().unwrap();
    let context = index
        .symbol_context("demo", &workspace, symbol_id, 50)
        .unwrap();
    assert!(context["body"]["content"]
        .as_str()
        .unwrap()
        .contains("def execute"));
    assert!(context["syntax_calls"]
        .as_array()
        .unwrap()
        .iter()
        .any(|call| call["name"] == "helper"));
}

#[test]
fn multi_query_symbol_search_scans_and_parses_each_file_once() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("service.rs"),
        "pub fn alpha_service() {}\npub fn beta_helper() {}\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    let queries = vec![
        "ALPHA_SERVICE".to_owned(),
        "beta_helper".to_owned(),
        "ALPHA_SERVICE".to_owned(),
    ];

    let first = index
        .find_symbols_many("demo", &workspace, &queries, ".", None, 10)
        .unwrap();
    assert_eq!(first["query_count"], 2);
    assert_eq!(first["files_considered"], 1);
    assert_eq!(first["files_parsed"], 1);
    assert_eq!(first["result_count"], 2);
    assert_eq!(first["results"][0]["name"], "alpha_service");
    assert_eq!(first["results"][1]["name"], "beta_helper");

    let cached = index
        .find_symbols_many("demo", &workspace, &queries, ".", None, 10)
        .unwrap();
    assert_eq!(cached["files_parsed"], 0);
    assert_eq!(cached["file_cache_hits"], 1);
    assert_eq!(cached["result_count"], 2);
}

#[test]
fn common_language_grammars_produce_real_symbols() {
    let dir = tempfile::tempdir().unwrap();
    let fixtures = [
        (
            "build.sh",
            "build_project() { echo ready; }\nbuild_project\n",
            "bash",
            "build_project",
        ),
        (
            "engine.c",
            "int compute_c(int value) { return value + 1; }\n",
            "c",
            "compute_c",
        ),
        (
            "engine.cpp",
            "int compute_cpp(int value) { return value + 1; }\n",
            "cpp",
            "compute_cpp",
        ),
        (
            "Worker.cs",
            "class Worker { public int Execute() { return 1; } }\n",
            "csharp",
            "Execute",
        ),
        (
            "Worker.java",
            "class Worker { int execute() { return 1; } }\n",
            "java",
            "execute",
        ),
        (
            "worker.php",
            "<?php class Worker { public function execute() { return 1; } }\n",
            "php",
            "execute",
        ),
        (
            "worker.rb",
            "class Worker\n  def execute\n    1\n  end\nend\n",
            "ruby",
            "execute",
        ),
    ];
    for (path, source, _, _) in fixtures {
        fs::write(dir.path().join(path), source).unwrap();
    }

    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    let capabilities = index.capabilities();
    assert_eq!(
        capabilities["language_count"].as_u64(),
        capabilities["languages"]
            .as_array()
            .map(|languages| languages.len() as u64)
    );
    assert!(capabilities["language_count"].as_u64().unwrap_or_default() >= 13);

    for (path, _, language, expected_name) in fixtures {
        let outline = index.file_outline("demo", &workspace, path, 100).unwrap();
        assert_eq!(outline["language"], language, "wrong language for {path}");
        assert_eq!(outline["parse_errors"], false, "parse error in {path}");
        assert!(
            outline["symbols"]
                .as_array()
                .unwrap()
                .iter()
                .any(|symbol| symbol["name"] == expected_name),
            "{path} did not expose {expected_name}: {}",
            outline["symbols"]
        );
    }

    let qualified = index
        .find_symbol("demo", &workspace, "Worker.execute", ".", None, 20)
        .unwrap();
    assert!(
        qualified["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|symbol| symbol["language"] == "java"),
        "qualified-name search should use the leaf token as its source prefilter"
    );
}

#[test]
fn extended_language_grammars_produce_real_symbols() {
    let dir = tempfile::tempdir().unwrap();
    let fixtures = [
        (
            "styles.css",
            ":root { --space: 8px; }\n.card, #main { color: red; }\n@keyframes fade { from { opacity: 0; } to { opacity: 1; } }\n",
            "css",
            ".card, #main",
        ),
        (
            "page.html",
            "<main id=\"app\"><user-card></user-card></main>\n",
            "html",
            "app",
        ),
        (
            "worker.dart",
            "class Worker { int execute() => 1; }\n",
            "dart",
            "execute",
        ),
        (
            "worker.ex",
            "defmodule Worker do\n  def execute, do: 1\nend\n",
            "elixir",
            "execute",
        ),
        (
            "worker.lua",
            "local function execute()\n  return 1\nend\n",
            "lua",
            "execute",
        ),
        ("worker.ml", "let execute () = 1\n", "ocaml", "execute"),
        (
            "worker.mli",
            "val execute : unit -> int\n",
            "ocaml-interface",
            "execute",
        ),
        (
            "worker.R",
            "execute <- function() {\n  1\n}\n",
            "r",
            "execute",
        ),
        (
            "Worker.swift",
            "final class Worker {\n  func execute() -> Int { 1 }\n}\n",
            "swift",
            "execute",
        ),
    ];
    for (path, source, _, _) in fixtures {
        fs::write(dir.path().join(path), source).unwrap();
    }

    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    let capabilities = index.capabilities();
    assert_eq!(
        capabilities["language_count"].as_u64(),
        capabilities["languages"]
            .as_array()
            .map(|languages| languages.len() as u64)
    );
    assert!(capabilities["language_count"].as_u64().unwrap_or_default() >= 20);

    for (path, _, language, expected_name) in fixtures {
        let outline = index.file_outline("demo", &workspace, path, 100).unwrap();
        assert_eq!(outline["language"], language, "wrong language for {path}");
        assert_eq!(outline["parse_errors"], false, "parse error in {path}");
        assert!(
            outline["symbols"]
                .as_array()
                .unwrap()
                .iter()
                .any(|symbol| symbol["name"] == expected_name),
            "{path} did not expose {expected_name}: {}",
            outline["symbols"]
        );
    }
}

#[test]
fn html_and_css_outlines_keep_navigation_signal_without_tag_noise() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("page.html"),
        "<main id=\"app\">\n  <div>noise</div>\n  <user-card id=\"profile\"></user-card>\n</main>\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("styles.css"),
        ":root {\n  --space: 8px;\n}\n.card, #main { color: red; }\n@keyframes fade { from { opacity: 0; } to { opacity: 1; } }\n",
    )
    .unwrap();

    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();

    let html = index
        .file_outline("demo", &workspace, "page.html", 100)
        .unwrap();
    let html_symbols = html["symbols"].as_array().unwrap();
    assert!(html_symbols
        .iter()
        .any(|symbol| symbol["name"] == "app" && symbol["kind"] == "element"));
    assert!(html_symbols
        .iter()
        .any(|symbol| { symbol["name"] == "user-card" && symbol["kind"] == "component" }));
    assert!(html_symbols
        .iter()
        .all(|symbol| symbol["name"] != "main" && symbol["name"] != "div"));

    let app_id = html_symbols
        .iter()
        .find(|symbol| symbol["name"] == "app")
        .and_then(|symbol| symbol["id"].as_str())
        .unwrap();
    let html_context = index
        .symbol_context("demo", &workspace, app_id, 50)
        .unwrap();
    assert!(html_context["body"]["content"]
        .as_str()
        .unwrap()
        .contains("<user-card"));

    let css = index
        .file_outline("demo", &workspace, "styles.css", 100)
        .unwrap();
    let css_symbols = css["symbols"].as_array().unwrap();
    for (name, kind) in [
        (".card, #main", "selector"),
        ("--space", "variable"),
        ("fade", "keyframes"),
    ] {
        assert!(
            css_symbols
                .iter()
                .any(|symbol| symbol["name"] == name && symbol["kind"] == kind),
            "CSS outline did not expose {name} as {kind}: {}",
            css["symbols"]
        );
    }

    let fade = index
        .find_symbol("demo", &workspace, "fade", "styles.css", None, 10)
        .unwrap();
    let fade_id = fade["results"][0]["id"].as_str().unwrap();
    let css_context = index
        .symbol_context("demo", &workspace, fade_id, 50)
        .unwrap();
    assert!(css_context["body"]["content"]
        .as_str()
        .unwrap()
        .contains("@keyframes fade"));
}

#[test]
fn extensionless_script_names_are_detected() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("Rakefile"), "task :build do\nend\n").unwrap();
    fs::write(dir.path().join(".bashrc"), "load_env() { echo ready; }\n").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();

    assert_eq!(
        index
            .file_outline("demo", &workspace, "Rakefile", 100)
            .unwrap()["language"],
        "ruby"
    );
    assert_eq!(
        index
            .file_outline("demo", &workspace, ".bashrc", 100)
            .unwrap()["language"],
        "bash"
    );
}

#[test]
fn text_prefilter_avoids_building_ast_for_clear_misses() {
    let dir = tempfile::tempdir().unwrap();
    for index in 0..32 {
        fs::write(
            dir.path().join(format!("module_{index}.rs")),
            format!("pub fn unrelated_{index}() {{}}\n"),
        )
        .unwrap();
    }
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();

    let result = index
        .find_symbol("demo", &workspace, "DefinitelyAbsentSymbol", ".", None, 20)
        .unwrap();

    assert_eq!(result["files_considered"], 32);
    assert_eq!(result["files_parsed"], 0);
    assert_eq!(result["result_count"], 0);
    assert_eq!(result["index"]["indexed_files"], 0);
    assert_eq!(result["index"]["ast_cached_files"], 0);
}

#[test]
fn symbol_signatures_reuse_workspace_secret_redaction() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("settings.py"),
        "api_key = \"super-secret-value\"\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();

    let outline = index
        .file_outline("demo", &workspace, "settings.py", 100)
        .unwrap();
    let symbol = outline["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .find(|symbol| symbol["name"] == "api_key")
        .unwrap();
    assert_eq!(symbol["signature_redacted"], true);
    assert!(symbol["signature"].as_str().unwrap().contains("[REDACTED]"));
    assert!(!outline.to_string().contains("super-secret-value"));
}

#[test]
fn c_context_expands_function_extent_and_extracts_calls() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("engine.c"),
        "int helper(void) { return 1; }\nint compute(void) {\n    return helper();\n}\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();

    let search = index
        .find_symbol("demo", &workspace, "compute", ".", None, 20)
        .unwrap();
    let symbol_id = search["results"][0]["id"].as_str().unwrap();
    let context = index
        .symbol_context("demo", &workspace, symbol_id, 50)
        .unwrap();
    assert!(context["body"]["content"]
        .as_str()
        .unwrap()
        .contains("return helper();"));
    assert!(context["syntax_calls"]
        .as_array()
        .unwrap()
        .iter()
        .any(|call| call["name"] == "helper" && call["kind"] == "call"));
}

#[test]
fn scan_failure_count_is_not_limited_by_diagnostic_sample() {
    let dir = tempfile::tempdir().unwrap();
    for index in 0..10 {
        fs::write(
            dir.path().join(format!("broken_{index}.py")),
            b"Needle = \xff\n",
        )
        .unwrap();
    }
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();

    let result = index
        .find_symbol("demo", &workspace, "Needle", ".", None, 20)
        .unwrap();
    assert_eq!(result["files_failed"], 10);
    assert_eq!(result["failures"].as_array().unwrap().len(), 8);
    assert_eq!(result["failures_truncated"], true);
}

#[test]
fn software_graph_reuses_indexed_symbols_and_marks_syntax_precision() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("engine.rs"),
        "fn helper() -> u8 { 1 }\nfn compute() -> u8 { helper() }\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();

    let snapshot = index
        .software_graph("demo", &workspace, ".", 100, 100)
        .unwrap();
    assert_eq!(snapshot.provider, "tree-sitter");
    assert_eq!(snapshot.precision, GraphPrecision::Syntax);
    assert_eq!(snapshot.files_indexed, 1);
    assert!(!snapshot.truncated);

    let file = snapshot
        .graph
        .nodes
        .values()
        .find(|node| node.kind == NodeKind::File)
        .unwrap();
    let helper = snapshot
        .graph
        .nodes
        .values()
        .find(|node| node.label == "helper")
        .unwrap();
    let compute = snapshot
        .graph
        .nodes
        .values()
        .find(|node| node.label == "compute")
        .unwrap();
    assert_eq!(helper.provenance.precision, GraphPrecision::Syntax);
    assert!(snapshot.graph.edges.iter().any(|edge| {
        edge.kind == EdgeKind::Defines && edge.from == file.id && edge.to == helper.id
    }));
    assert!(snapshot.graph.edges.iter().any(|edge| {
        edge.kind == EdgeKind::Calls && edge.from == compute.id && edge.to == helper.id
    }));
}

#[test]
fn software_graph_resolves_unique_cross_file_calls_at_syntax_precision() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("helper.rs"),
        "pub fn helper() -> u8 { 1 }\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("main.rs"),
        "fn compute() -> u8 { helper() }\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();

    let snapshot = index
        .software_graph("demo", &workspace, ".", 100, 100)
        .unwrap();
    let helper = snapshot
        .graph
        .nodes
        .values()
        .find(|node| node.label == "helper")
        .unwrap();
    let compute = snapshot
        .graph
        .nodes
        .values()
        .find(|node| node.label == "compute")
        .unwrap();
    let edge = snapshot
        .graph
        .edges
        .iter()
        .find(|edge| {
            edge.kind == EdgeKind::Calls && edge.from == compute.id && edge.to == helper.id
        })
        .expect("unique cross-file call edge");
    assert_eq!(edge.provenance.precision, GraphPrecision::Syntax);
    assert_eq!(
        edge.provenance.provider,
        "tree-sitter/global-name-resolution"
    );
}

#[test]
fn invalidation_rebuilds_changed_symbols() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("main.go"),
        "package main\nfunc OldName() {}\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), true, false).unwrap();
    let index = CodeIndex::new().unwrap();
    let first = index
        .find_symbol("demo", &workspace, "OldName", ".", None, 10)
        .unwrap();
    assert_eq!(first["result_count"], 1);
    let view = workspace.read_file("main.go", 1, None).unwrap();
    workspace
        .replace_text("main.go", "OldName", "NewName", &view.sha256)
        .unwrap();
    index.invalidate(workspace.root(), "main.go");
    let second = index
        .find_symbol("demo", &workspace, "NewName", ".", None, 10)
        .unwrap();
    assert_eq!(second["result_count"], 1);
    assert_eq!(second["results"][0]["name"], "NewName");
}

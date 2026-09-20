use super::*;
use std::fs;

#[path = "budget.rs"]
mod budget;
#[path = "calls.rs"]
mod calls;
#[path = "consistency.rs"]
mod consistency;
#[path = "flights.rs"]
mod flights;
#[path = "search_perf.rs"]
mod search_perf;

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
fn rust_module_reexport_shell_is_indexed_as_module() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "mod session;\npub use session::*;\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("session.rs"),
        "pub fn cleanup_if_owner() {}\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    let outline = index
        .file_outline("demo", &workspace, "lib.rs", 100)
        .unwrap();
    let session = outline["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .find(|symbol| symbol["qualified_name"] == "session")
        .expect("mod declaration should be indexed");
    println!("MODULE_REEXPORT_SHELL {session}");
    assert_eq!(session["kind"], "module");
}

#[test]
fn batch_symbol_resolution_matches_single_file_resolution_semantics() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("service.rs"),
        "fn first() {}\nfn second() {}\nmod nested { pub fn second() {} }\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    let requested = vec![
        "first".to_owned(),
        "nested::second".to_owned(),
        "second".to_owned(),
        "missing".to_owned(),
    ];
    let resolved = index
        .resolve_symbols(&workspace, "service.rs", &requested)
        .unwrap();

    assert_eq!(resolved["first"].as_ref().unwrap().qualified_name, "first");
    assert_eq!(
        resolved["nested::second"].as_ref().unwrap().qualified_name,
        "nested::second"
    );
    assert_eq!(
        resolved["second"].as_ref().unwrap().qualified_name,
        "second"
    );
    assert!(resolved["missing"].is_none());
    for requested_name in &requested {
        let single = index
            .resolve_symbol(&workspace, "service.rs", requested_name)
            .unwrap();
        let batch = resolved.get(requested_name).unwrap();
        assert_eq!(
            batch.as_ref().map(|resolution| resolution.id.as_str()),
            single.as_ref().map(|resolution| resolution.id.as_str()),
            "batch resolution changed single-symbol semantics for {requested_name}"
        );
    }
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
    assert_eq!(search["result_count"], 2, "search={search}");
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
fn multi_query_symbol_search_preserves_later_exact_queries_under_global_limit() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["first", "second", "third"] {
        fs::write(dir.path().join(format!("{name}.rs")), "pub fn run() {}\n").unwrap();
    }
    fs::write(
        dir.path().join("target.rs"),
        "pub fn critical_target() {}\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    let queries = vec!["run".to_owned(), "critical_target".to_owned()];

    let search = index
        .find_symbols_many("demo", &workspace, &queries, ".", None, 2)
        .unwrap();
    let names = search["results"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|symbol| symbol["name"].as_str())
        .collect::<Vec<_>>();

    assert_eq!(search["result_count"], 2);
    assert!(search["truncated"].as_bool().unwrap());
    assert!(names.contains(&"run"));
    assert!(names.contains(&"critical_target"));
}

#[test]
fn cached_exact_symbol_seed_revalidates_external_edits() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("target.rs"),
        "pub fn target_feature() -> usize { 7 }\n",
    )
    .unwrap();
    fs::write(dir.path().join("noise.rs"), "pub fn unrelated() {}\n").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    index
        .ensure_indexed(&workspace, "target.rs", false)
        .unwrap();
    index.ensure_indexed(&workspace, "noise.rs", false).unwrap();

    let query = vec!["target_feature".to_owned()];
    let cached = index
        .cached_exact_symbols_many(&workspace, &query, None, 10)
        .unwrap();
    assert_eq!(cached.len(), 1);
    assert_eq!(cached[0]["qualified_name"], "target_feature");

    fs::write(
        dir.path().join("target.rs"),
        "pub fn renamed_feature() -> usize { 9 }\n",
    )
    .unwrap();
    let stale = index
        .cached_exact_symbols_many(&workspace, &query, None, 10)
        .unwrap();
    assert!(
        stale.is_empty(),
        "stale exact candidates must be revalidated"
    );

    let renamed = vec!["renamed_feature".to_owned()];
    let refreshed = index
        .cached_exact_symbols_many(&workspace, &renamed, None, 10)
        .unwrap();
    assert_eq!(refreshed.len(), 1);
    assert_eq!(refreshed[0]["qualified_name"], "renamed_feature");
}

#[test]
fn cached_exact_symbol_seed_uses_reverse_index_and_drops_invalidated_entries() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("target.rs"),
        "pub fn target_feature() -> usize { 7 }\n",
    )
    .unwrap();
    for index in 0..32 {
        fs::write(
            dir.path().join(format!("noise_{index}.rs")),
            format!("pub fn unrelated_{index}() {{}}\n"),
        )
        .unwrap();
    }
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    index
        .ensure_indexed(&workspace, "target.rs", false)
        .unwrap();
    for file in 0..32 {
        index
            .ensure_indexed(&workspace, &format!("noise_{file}.rs"), false)
            .unwrap();
    }

    let key = "target_feature".to_owned();
    {
        let state = index.state.lock().unwrap();
        let files = state.exact_symbol_files.get(&key).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files.iter().any(|file| file.path == "target.rs"));
    }
    let cached = index
        .cached_exact_symbols_many(&workspace, std::slice::from_ref(&key), None, 10)
        .unwrap();
    assert_eq!(cached.len(), 1);
    assert_eq!(cached[0]["qualified_name"], "target_feature");

    index.invalidate(workspace.root(), "target.rs");
    let state = index.state.lock().unwrap();
    assert!(!state.exact_symbol_files.contains_key(&key));
}

#[test]
fn cached_exact_symbol_seed_preserves_multiple_queries_across_files() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("entry.rs"),
        "pub fn feature_entry() -> usize { 1 }\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("worker.rs"),
        "pub fn batch_worker() -> usize { 2 }\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    index.ensure_indexed(&workspace, "entry.rs", false).unwrap();
    index
        .ensure_indexed(&workspace, "worker.rs", false)
        .unwrap();

    let queries = vec!["feature_entry".to_owned(), "batch_worker".to_owned()];
    let cached = index
        .cached_exact_symbols_many(&workspace, &queries, None, 10)
        .unwrap();
    let names = cached
        .iter()
        .filter_map(|symbol| symbol["qualified_name"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["feature_entry", "batch_worker"]);
}

#[test]
fn software_graph_preserves_rust_module_struct_trait_and_enum_kinds() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "mod nested;\npub struct Packet;\npub trait Worker { fn run(&self); }\npub enum Mode { Fast, Safe }\n",
    )
    .unwrap();
    fs::write(dir.path().join("nested.rs"), "pub fn helper() {}\n").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();

    let snapshot = index
        .software_graph("demo", &workspace, ".", 100, 100)
        .unwrap();

    let find = |label: &str| {
        snapshot
            .graph
            .nodes
            .values()
            .find(|node| node.label == label)
            .unwrap_or_else(|| panic!("missing graph node {label}"))
    };
    let module = find("nested");
    let packet = find("Packet");
    let worker = find("Worker");
    let mode = find("Mode");

    assert_eq!(module.kind, NodeKind::Module);
    assert_eq!(module.attributes["source_kind"], "module");
    assert_eq!(packet.kind, NodeKind::Struct);
    assert_eq!(packet.attributes["source_kind"], "struct");
    assert_eq!(worker.kind, NodeKind::Trait);
    assert_eq!(worker.attributes["source_kind"], "trait");
    assert_eq!(mode.kind, NodeKind::Enum);
    assert_eq!(mode.attributes["source_kind"], "enum");
    for node in [module, packet, worker, mode] {
        assert_eq!(node.attributes["language"], "rust");
    }
}

#[test]
fn software_graph_resolves_rust_qualified_calls_without_guessing_ambiguous_bare_calls() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("alpha.rs"), "pub fn helper() -> u8 { 1 }\n").unwrap();
    fs::write(dir.path().join("beta.rs"), "pub fn helper() -> u8 { 2 }\n").unwrap();
    fs::write(
        dir.path().join("main.rs"),
        "pub fn choose() -> u8 { alpha::helper() }\npub fn ambiguous() -> u8 { helper() }\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();

    let snapshot = index
        .software_graph("demo", &workspace, ".", 100, 100)
        .unwrap();
    let choose = snapshot
        .graph
        .nodes
        .values()
        .find(|node| node.label == "choose")
        .unwrap();
    let ambiguous = snapshot
        .graph
        .nodes
        .values()
        .find(|node| node.label == "ambiguous")
        .unwrap();
    let alpha_helper = snapshot
        .graph
        .nodes
        .values()
        .find(|node| node.label == "helper" && node.attributes["path"].as_str() == Some("alpha.rs"))
        .unwrap();

    let qualified_edge = snapshot
        .graph
        .edges
        .iter()
        .find(|edge| {
            edge.kind == EdgeKind::Calls && edge.from == choose.id && edge.to == alpha_helper.id
        })
        .expect("qualified Rust path should resolve the duplicate function name");
    assert_eq!(
        qualified_edge.provenance.provider,
        "tree-sitter/rust-path-resolution"
    );
    assert!(!snapshot
        .graph
        .edges
        .iter()
        .any(|edge| { edge.kind == EdgeKind::Calls && edge.from == ambiguous.id }));
}

#[test]
fn software_graph_uses_rust_imports_to_disambiguate_bare_cross_file_calls() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("alpha.rs"), "pub fn helper() -> u8 { 1 }\n").unwrap();
    fs::write(dir.path().join("beta.rs"), "pub fn helper() -> u8 { 2 }\n").unwrap();
    fs::write(
        dir.path().join("main.rs"),
        "use alpha::helper;\npub fn choose() -> u8 { helper() }\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("other.rs"),
        "pub fn ambiguous() -> u8 { helper() }\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();

    let snapshot = index
        .software_graph("demo", &workspace, ".", 100, 100)
        .unwrap();
    let choose = snapshot
        .graph
        .nodes
        .values()
        .find(|node| node.label == "choose")
        .unwrap();
    let ambiguous = snapshot
        .graph
        .nodes
        .values()
        .find(|node| node.label == "ambiguous")
        .unwrap();
    let alpha_helper = snapshot
        .graph
        .nodes
        .values()
        .find(|node| node.label == "helper" && node.attributes["path"].as_str() == Some("alpha.rs"))
        .unwrap();

    let imported_edge = snapshot
        .graph
        .edges
        .iter()
        .find(|edge| {
            edge.kind == EdgeKind::Calls && edge.from == choose.id && edge.to == alpha_helper.id
        })
        .expect("explicit Rust use should disambiguate the bare call");
    assert_eq!(
        imported_edge.provenance.provider,
        "tree-sitter/rust-import-resolution"
    );
    assert!(!snapshot
        .graph
        .edges
        .iter()
        .any(|edge| { edge.kind == EdgeKind::Calls && edge.from == ambiguous.id }));
}

#[test]
fn code_indexes_share_language_configs_but_keep_independent_state() {
    let first = CodeIndex::new().unwrap();
    let second = CodeIndex::new().unwrap();
    assert!(Arc::ptr_eq(&first.configs, &second.configs));
    assert!(!Arc::ptr_eq(&first.state, &second.state));
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
    fs::write(
        dir.path().join("smoke.bats"),
        "#!/usr/bin/env bats\n@test \"works\" { true; }\n",
    )
    .unwrap();
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
    assert_eq!(
        index
            .file_outline("demo", &workspace, "smoke.bats", 100)
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

#[test]
fn non_aggressive_memory_trim_keeps_a_warm_ast_working_set() {
    let dir = tempfile::tempdir().unwrap();
    for file in 0..20 {
        fs::write(
            dir.path().join(format!("module_{file}.rs")),
            format!("pub fn symbol_{file}() {{}}\n"),
        )
        .unwrap();
    }
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let index = CodeIndex::new().unwrap();
    for file in 0..20 {
        index
            .file_outline("demo", &workspace, &format!("module_{file}.rs"), 20)
            .unwrap();
    }
    assert_eq!(
        index.stats_for_root(workspace.root())["ast_cached_files"],
        20
    );
    index.trim_memory(false);
    let remaining = index.stats_for_root(workspace.root())["ast_cached_files"]
        .as_u64()
        .unwrap();
    assert!((1..20).contains(&remaining));

    let newest = index
        .file_outline("demo", &workspace, "module_19.rs", 20)
        .unwrap();
    assert_eq!(newest["ast_cache_hit"], true);
    let oldest = index
        .file_outline("demo", &workspace, "module_0.rs", 20)
        .unwrap();
    assert_eq!(oldest["ast_cache_hit"], false);
}

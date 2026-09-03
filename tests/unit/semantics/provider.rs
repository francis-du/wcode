use super::*;

#[test]
fn every_indexed_language_has_a_semantic_provider_candidate() {
    for language in SemanticLanguage::ALL {
        assert!(
            PROVIDERS
                .iter()
                .any(|provider| provider.languages.contains(&language)),
            "missing semantic provider candidate for {}",
            language.as_str()
        );
    }
}

#[test]
fn every_indexed_language_has_exactly_one_canonical_launch_profile() {
    let workspace_dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(workspace_dir.path(), false, true).unwrap();
    let mut provider_ids = BTreeSet::new();
    for provider in PROVIDERS {
        assert!(
            provider_ids.insert(provider.id),
            "duplicate provider id {}",
            provider.id
        );
        assert!(
            !provider.executables.is_empty()
                && provider.executables.iter().all(|name| !name.is_empty()),
            "provider {} must have at least one executable alias",
            provider.id
        );
    }

    for language in SemanticLanguage::ALL {
        let canonical = PROVIDERS
            .iter()
            .copied()
            .filter(|provider| provider.canonical && provider.languages.contains(&language))
            .collect::<Vec<_>>();
        assert_eq!(
            canonical.len(),
            1,
            "{} must have exactly one canonical provider profile",
            language.as_str()
        );
        let provider = canonical[0];
        assert!(!language.lsp_language_id().is_empty());
        assert!(client::initialization_options(provider.id).is_object());
        let args = provider_launch_args(&workspace, provider).unwrap();
        assert!(args.iter().all(|arg| !arg.is_empty()));
    }
}

#[test]
fn canonical_provider_matrix_covers_all_22_languages() {
    let expected = [
        (SemanticLanguage::Bash, "bash-language-server"),
        (SemanticLanguage::C, "clangd"),
        (SemanticLanguage::Cpp, "clangd"),
        (SemanticLanguage::CSharp, "csharp-ls"),
        (SemanticLanguage::Css, "vscode-css-language-server"),
        (SemanticLanguage::Dart, "dart-language-server"),
        (SemanticLanguage::Elixir, "elixir-ls"),
        (SemanticLanguage::Go, "gopls"),
        (SemanticLanguage::Html, "vscode-html-language-server"),
        (SemanticLanguage::Java, "jdtls"),
        (SemanticLanguage::JavaScript, "typescript-language-server"),
        (SemanticLanguage::Lua, "lua-language-server"),
        (SemanticLanguage::Ocaml, "ocamllsp"),
        (SemanticLanguage::OcamlInterface, "ocamllsp"),
        (SemanticLanguage::Php, "phpactor"),
        (SemanticLanguage::Python, "pyright"),
        (SemanticLanguage::R, "r-languageserver"),
        (SemanticLanguage::Ruby, "ruby-lsp"),
        (SemanticLanguage::Rust, "rust-analyzer"),
        (SemanticLanguage::Swift, "sourcekit-lsp"),
        (SemanticLanguage::TypeScript, "typescript-language-server"),
        (SemanticLanguage::Tsx, "typescript-language-server"),
    ];
    assert_eq!(expected.len(), SemanticLanguage::ALL.len());
    for (language, provider_id) in expected {
        let canonical = PROVIDERS
            .iter()
            .find(|provider| provider.canonical && provider.languages.contains(&language))
            .unwrap();
        assert_eq!(canonical.id, provider_id, "{}", language.as_str());
    }
}

#[test]
fn alternate_provider_matrix_keeps_real_fallbacks_only() {
    let alternates = PROVIDERS
        .iter()
        .filter(|provider| !provider.canonical)
        .map(|provider| provider.id)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        alternates,
        BTreeSet::from(["intelephense", "pylsp", "solargraph"])
    );
}

#[test]
fn provider_specific_launch_profiles_match_current_server_contracts() {
    let first_root = tempfile::tempdir().unwrap();
    let second_root = tempfile::tempdir().unwrap();
    let first = Workspace::new(first_root.path(), false, true).unwrap();
    let second = Workspace::new(second_root.path(), false, true).unwrap();
    let provider = |id| {
        PROVIDERS
            .iter()
            .copied()
            .find(|provider| provider.id == id)
            .unwrap()
    };

    assert_eq!(
        provider_launch_args(&first, provider("gopls")).unwrap(),
        ["serve"]
    );
    let dart = provider_launch_args(&first, provider("dart-language-server")).unwrap();
    assert_eq!(dart.first().map(String::as_str), Some("language-server"));
    assert!(dart.iter().any(|arg| arg == "--protocol=lsp"));
    assert!(dart.windows(2).any(|pair| pair == ["--client-id", "wcode"]));
    assert!(dart.iter().any(|arg| arg == "--client-version"));
    assert_eq!(
        provider_launch_args(&first, provider("r-languageserver")).unwrap(),
        ["--no-echo", "-e", "languageserver::run()"]
    );
    let jdtls_first = provider_launch_args(&first, provider("jdtls")).unwrap();
    let jdtls_second = provider_launch_args(&second, provider("jdtls")).unwrap();
    assert_eq!(jdtls_first.first().map(String::as_str), Some("-data"));
    assert_ne!(
        jdtls_first, jdtls_second,
        "jdtls data directories must be workspace-specific"
    );
    for args in [&jdtls_first, &jdtls_second] {
        let data = PathBuf::from(&args[1]);
        assert!(data.is_absolute());
        assert!(!data.starts_with(first.root()) && !data.starts_with(second.root()));
    }
    let elixir = provider("elixir-ls");
    assert!(elixir.executables.contains(&"language_server.sh"));
    assert!(elixir.executables.contains(&"language_server"));
    assert!(elixir.executables.contains(&"elixir-ls"));
    assert!(!PROVIDERS.iter().any(|provider| provider.id == "omnisharp"));
}

#[tokio::test(flavor = "current_thread")]
async fn every_canonical_profile_completes_stdio_lsp_initialize() {
    let fixture_dir = tempfile::tempdir().unwrap();
    let executable = fixture_dir.path().join(if cfg!(windows) {
        "wcode-lsp-mock.exe"
    } else {
        "wcode-lsp-mock"
    });
    let source =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lsp_mock_server.rs");
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| std::ffi::OsString::from("rustc"));
    let compile = std::process::Command::new(rustc)
        .args(["--edition=2021", "-C", "debuginfo=0"])
        .arg(&source)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "mock LSP must compile: {}",
        String::from_utf8_lossy(&compile.stderr)
    );

    let workspace_dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(workspace_dir.path(), false, true).unwrap();
    let root_uri = Url::from_directory_path(workspace.root())
        .unwrap()
        .to_string();
    for language in SemanticLanguage::ALL {
        let provider = PROVIDERS
            .iter()
            .copied()
            .find(|provider| provider.canonical && provider.languages.contains(&language))
            .unwrap();
        let mut client = client::LspClient::start(&workspace, provider, &executable)
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "{} canonical provider {} failed to spawn through wcode: {error}",
                    language.as_str(),
                    provider.id
                )
            });
        let capabilities = client.initialize(&root_uri).await.unwrap_or_else(|error| {
            panic!(
                "{} canonical provider {} failed initialize framing: {error}",
                language.as_str(),
                provider.id
            )
        });
        assert_eq!(
            capabilities.get("positionEncoding").and_then(Value::as_str),
            Some("utf-8")
        );
        assert_eq!(
            capabilities
                .pointer("/textDocumentSync/change")
                .and_then(Value::as_u64),
            Some(2)
        );
        let uri = format!("file:///wcode-conformance/{}.txt", language.as_str());
        client
            .notify(
                "textDocument/didOpen",
                json!({"textDocument":{"uri":uri,"languageId":language.lsp_language_id(),"version":1,"text":"one"}}),
            )
            .await
            .unwrap();
        client
            .notify(
                "textDocument/didChange",
                json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":3}},"text":"two"}]}),
            )
            .await
            .unwrap();
        let hover = client
            .request(
                "textDocument/hover",
                json!({"textDocument":{"uri":uri},"position":{"line":0,"character":0}}),
            )
            .await
            .unwrap();
        assert_eq!(
            hover.pointer("/contents").and_then(Value::as_str),
            Some("mock-hover")
        );
        client
            .notify("textDocument/didClose", json!({"textDocument":{"uri":uri}}))
            .await
            .unwrap();
        let state = client.request("mock/state", json!({})).await.unwrap();
        assert_eq!(state.get("opened").and_then(Value::as_u64), Some(1));
        assert_eq!(state.get("changed").and_then(Value::as_u64), Some(1));
        assert_eq!(state.get("closed").and_then(Value::as_u64), Some(1));
    }
}

#[cfg(unix)]
#[test]
fn provider_launch_path_preserves_rustup_proxy_but_resolves_luals_symlink() {
    use std::os::unix::fs::symlink;

    let workspace_dir = tempfile::tempdir().unwrap();
    let tools = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(workspace_dir.path(), false, true).unwrap();
    let target = tools.path().join("provider-target");
    std::fs::write(&target, "fixture").unwrap();
    let rust_link = tools.path().join("rust-analyzer");
    let lua_link = tools.path().join("lua-language-server");
    symlink(&target, &rust_link).unwrap();
    symlink(&target, &lua_link).unwrap();
    let rust = PROVIDERS
        .iter()
        .copied()
        .find(|provider| provider.id == "rust-analyzer")
        .unwrap();
    let lua = PROVIDERS
        .iter()
        .copied()
        .find(|provider| provider.id == "lua-language-server")
        .unwrap();

    assert_eq!(
        client::provider_launch_executable(&workspace, rust, &rust_link).unwrap(),
        rust_link
    );
    assert_eq!(
        client::provider_launch_executable(&workspace, lua, &lua_link).unwrap(),
        target.canonicalize().unwrap()
    );
}

#[test]
fn language_detection_covers_the_full_index_surface() {
    let fixtures = [
        ("script.sh", SemanticLanguage::Bash),
        ("a.c", SemanticLanguage::C),
        ("a.cpp", SemanticLanguage::Cpp),
        ("a.cs", SemanticLanguage::CSharp),
        ("a.css", SemanticLanguage::Css),
        ("a.dart", SemanticLanguage::Dart),
        ("a.ex", SemanticLanguage::Elixir),
        ("a.go", SemanticLanguage::Go),
        ("a.html", SemanticLanguage::Html),
        ("A.java", SemanticLanguage::Java),
        ("a.js", SemanticLanguage::JavaScript),
        ("a.lua", SemanticLanguage::Lua),
        ("a.ml", SemanticLanguage::Ocaml),
        ("a.mli", SemanticLanguage::OcamlInterface),
        ("a.php", SemanticLanguage::Php),
        ("a.py", SemanticLanguage::Python),
        ("a.R", SemanticLanguage::R),
        ("a.rb", SemanticLanguage::Ruby),
        ("a.rs", SemanticLanguage::Rust),
        ("a.swift", SemanticLanguage::Swift),
        ("a.ts", SemanticLanguage::TypeScript),
        ("a.tsx", SemanticLanguage::Tsx),
    ];
    for (path, expected) in fixtures {
        assert_eq!(language_for_path(path), Some(expected), "{path}");
    }
}

#[test]
fn provider_revision_changes_only_when_index_inputs_change() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.rs"), "fn one() {}\n").unwrap();
    let executable = dir.path().join("rust-analyzer-fixture");
    std::fs::write(&executable, "fixture-binary-v1").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let provider = PROVIDERS
        .iter()
        .copied()
        .find(|provider| provider.id == "rust-analyzer")
        .unwrap();
    let files = vec![("a.rs".to_owned(), SemanticLanguage::Rust)];
    let first_sources = prepare_sources(&workspace, &files).unwrap();
    let first = provider_revision(provider, &executable, 1_000, &first_sources);
    let same_sources = prepare_sources(&workspace, &files).unwrap();
    let same = provider_revision(provider, &executable, 1_000, &same_sources);
    assert_eq!(first, same);

    let different_bound = provider_revision(provider, &executable, 2_000, &same_sources);
    assert_ne!(first, different_bound);
    std::fs::write(dir.path().join("a.rs"), "fn two() {}\n").unwrap();
    let changed_sources = prepare_sources(&workspace, &files).unwrap();
    let changed_source = provider_revision(provider, &executable, 1_000, &changed_sources);
    assert_ne!(first, changed_source);
}

#[test]
fn automatic_semantics_only_trust_the_hardened_rust_provider() {
    let rust = PROVIDERS
        .iter()
        .copied()
        .find(|provider| provider.id == "rust-analyzer")
        .unwrap();
    let clangd = PROVIDERS
        .iter()
        .copied()
        .find(|provider| provider.id == "clangd")
        .unwrap();
    assert!(automatic_provider(rust));
    assert!(!automatic_provider(clangd));
    let options = client::initialization_options("rust-analyzer");
    assert_eq!(
        options.pointer("/cargo/buildScripts/enable"),
        Some(&json!(false))
    );
    assert_eq!(options.pointer("/cargo/autoreload"), Some(&json!(false)));
    assert_eq!(options.pointer("/procMacro/enable"), Some(&json!(false)));
    assert_eq!(options.get("checkOnSave"), Some(&json!(false)));
}

#[test]
fn gopls_discovery_includes_standard_go_install_locations() {
    let home = PathBuf::from("/fixture/home");
    let gobin = PathBuf::from("/fixture/gobin");
    let gopath = PathBuf::from("/fixture/gopath");
    let paths = known_language_tool_paths_from(
        "gopls",
        Some(gobin.clone().into_os_string()),
        Some(gopath.clone().into_os_string()),
        Some(home.clone().into_os_string()),
    );
    assert!(paths.contains(&gobin.join(executable_name("gopls"))));
    assert!(paths.contains(&gopath.join("bin").join(executable_name("gopls"))));
    assert!(paths.contains(&home.join("go/bin").join(executable_name("gopls"))));
}

#[test]
fn external_gopls_needs_provider_identity_approval_not_home_workspace_access() {
    let workspace_dir = tempfile::tempdir().unwrap();
    let tools_dir = tempfile::tempdir().unwrap();
    let gopls = tools_dir.path().join(executable_name("gopls"));
    std::fs::write(&gopls, "fixture").unwrap();
    let workspaces =
        crate::workspace::Workspaces::new([workspace_dir.path()], false, true).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let (_, workspace) = workspaces.select(Some(&workspace_id)).unwrap();
    let provider = PROVIDERS
        .iter()
        .copied()
        .find(|provider| provider.id == "gopls")
        .unwrap();

    let error = authorize_provider_session(&workspace, provider, &gopls).unwrap_err();
    assert!(error.to_string().contains("authorization required"));
    let request = workspaces.latest_pending_authorization().unwrap();
    assert_eq!(request.kind, AuthorizationKind::RiskyExecution);
    assert!(request.summary.contains("gopls"));
    assert!(!workspaces.full_access_enabled());

    assert!(workspaces.approve_authorization_session(&request.id));
    authorize_provider_session(&workspace, provider, &gopls).unwrap();
    let operation = provider_session_operation(&workspace, provider, &gopls).unwrap();
    assert!(workspace.risky_operation_authorized(&operation));
    assert!(!workspaces.full_access_enabled());
}

#[test]
fn missing_provider_binary_reports_discovery_stage_and_action() {
    let workspace_dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(workspace_dir.path(), false, true).unwrap();
    let provider = PROVIDERS
        .iter()
        .copied()
        .find(|provider| provider.id == "gopls")
        .unwrap();
    let missing = workspace_dir.path().join("../definitely-missing-gopls");
    let error = client::provider_launch_executable(&workspace, provider, &missing)
        .unwrap_err()
        .to_string();
    assert!(error.contains("stage=discovery"));
    assert!(error.contains("action=refresh_lsp_discovery_or_reinstall"));
    assert!(!error.starts_with("No such file or directory"));
}

#[test]
fn provider_executables_inside_the_workspace_are_rejected() {
    let workspace_dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(workspace_dir.path(), false, false).unwrap();
    let injected = workspace_dir.path().join("rust-analyzer");
    std::fs::write(&injected, "fixture").unwrap();
    assert!(trusted_provider_path(&workspace, &injected).is_none());

    let external_dir = tempfile::tempdir().unwrap();
    let external = external_dir.path().join("rust-analyzer");
    std::fs::write(&external, "fixture").unwrap();
    assert_eq!(trusted_provider_path(&workspace, &external), Some(external));
}

#[cfg(unix)]
#[test]
fn trusted_provider_path_preserves_proxy_symlink_identity() {
    use std::os::unix::fs::symlink;

    let workspace_dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(workspace_dir.path(), false, false).unwrap();
    let tools = tempfile::tempdir().unwrap();
    let proxy_target = tools.path().join("rustup");
    let provider_link = tools.path().join("rust-analyzer");
    std::fs::write(&proxy_target, "fixture").unwrap();
    symlink(&proxy_target, &provider_link).unwrap();

    assert_eq!(
        trusted_provider_path(&workspace, &provider_link),
        Some(provider_link),
        "validation must follow the symlink without replacing the executable path"
    );
}

#[test]
fn provider_session_authorization_binds_provider_and_binary_identity() {
    let workspace_dir = tempfile::tempdir().unwrap();
    let tools = tempfile::tempdir().unwrap();
    let executable = tools.path().join("provider");
    std::fs::write(&executable, "first").unwrap();
    let workspace = Workspace::new(workspace_dir.path(), false, true).unwrap();
    let rust = PROVIDERS
        .iter()
        .copied()
        .find(|provider| provider.id == "rust-analyzer")
        .unwrap();
    let clangd = PROVIDERS
        .iter()
        .copied()
        .find(|provider| provider.id == "clangd")
        .unwrap();

    let first = provider_session_operation(&workspace, rust, &executable).unwrap();
    let other_provider = provider_session_operation(&workspace, clangd, &executable).unwrap();
    assert_ne!(first, other_provider);

    std::fs::write(&executable, "second-binary").unwrap();
    let replaced = provider_session_operation(&workspace, rust, &executable).unwrap();
    assert_ne!(first, replaced);
}

#[test]
fn relation_expansion_prefers_high_value_symbol_kinds() {
    assert!(call_hierarchy_candidate(6));
    assert!(call_hierarchy_candidate(12));
    assert!(!call_hierarchy_candidate(13));
    assert!(implementation_candidate(11));
    assert!(implementation_candidate(23));
    assert!(!implementation_candidate(13));
}

#[test]
fn lsp_document_symbols_flatten_with_qualified_names() {
    let value = json!([{
        "name":"Outer","kind":5,
        "selectionRange":{"start":{"line":1,"character":2},"end":{"line":1,"character":7}},
        "children":[{
            "name":"work","kind":6,
            "selectionRange":{"start":{"line":3,"character":4},"end":{"line":3,"character":8}}
        }]
    }]);
    let mut output = Vec::new();
    flatten_document_symbols(&value, "src/a.rs", None, &mut output);
    assert_eq!(output.len(), 2);
    assert_eq!(output[1].qualified_name, "Outer::work");
    assert_eq!(output[1].line, 3);
}

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
fn install_plans_cover_every_canonical_language_without_arbitrary_commands() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, true).unwrap();
    let allowed_managers = BTreeSet::from(["rustup", "go", "npm", "dotnet", "opam", "gem"]);
    let mut model_installable = 0usize;
    let mut manual = 0usize;
    for language in SemanticLanguage::ALL {
        let plan = install::install_plan(&workspace, language).unwrap();
        let canonical = PROVIDERS
            .iter()
            .find(|provider| provider.canonical && provider.languages.contains(&language))
            .unwrap();
        assert_eq!(plan.provider, canonical.id);
        assert_eq!(plan.post_install_action, "semantic_provider_refresh");
        assert!(plan.requires_approval);
        if plan.model_can_install {
            model_installable += 1;
            assert!(
                allowed_managers.contains(plan.manager),
                "{language:?}: {}",
                plan.manager
            );
            let program = plan
                .program
                .as_deref()
                .expect("installable plan needs a program");
            assert_eq!(program, plan.manager);
            assert!(!plan.args.is_empty());
            assert!(plan
                .args
                .iter()
                .all(|arg| !arg.contains(['\0', '\n', '\r'])));
        } else {
            manual += 1;
            assert_eq!(plan.strategy, "manual");
            assert!(plan.program.is_none());
            assert!(plan.args.is_empty());
        }
    }
    assert!(model_installable >= 10);
    assert!(manual > 0);
}

#[test]
fn lsp_install_authorization_denial_and_retry_are_fail_closed_before_side_effects() {
    let root = tempfile::tempdir().unwrap();
    let workspaces = crate::workspace::Workspaces::new([root.path()], false, true).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let (_, workspace) = workspaces.select(Some(&workspace_id)).unwrap();
    let language = SemanticLanguage::Python;
    let provider = install::canonical_provider(language).unwrap();
    let plan = install::install_plan(&workspace, language).unwrap();
    let destination = PathBuf::from(plan.destination.as_deref().unwrap());
    assert!(!destination.exists());

    let first = install::authorize_install_plan(&workspace, language, provider, &plan).unwrap_err();
    assert!(first.to_string().contains("authorization required"));
    let first_request = workspaces.latest_pending_authorization().unwrap();
    assert_eq!(
        first_request.kind,
        crate::authorization::AuthorizationKind::RiskyExecution
    );
    assert_eq!(first_request.workspace, workspace_id);
    assert!(workspaces.deny_authorization(&first_request.id));
    assert!(!destination.exists());

    let second =
        install::authorize_install_plan(&workspace, language, provider, &plan).unwrap_err();
    assert!(second.to_string().contains("authorization required"));
    let second_request = workspaces.latest_pending_authorization().unwrap();
    assert_ne!(second_request.id, first_request.id);
    assert!(workspaces.approve_authorization_session(&second_request.id));
    install::authorize_install_plan(&workspace, language, provider, &plan).unwrap();
    assert!(!destination.exists());
}

#[test]
fn semantic_rename_plan_merges_same_line_and_applies_sha_guarded_multi_file_edits() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.rs"), "old old\n").unwrap();
    std::fs::write(root.path().join("b.rs"), "old\n").unwrap();
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let target = workspace.load_source("a.rs").unwrap();
    let a_uri = Url::from_file_path(root.path().join("a.rs"))
        .unwrap()
        .to_string();
    let b_uri = Url::from_file_path(root.path().join("b.rs"))
        .unwrap()
        .to_string();
    let mut workspace_edit = json!({"changes": {}});
    let changes = workspace_edit["changes"].as_object_mut().unwrap();
    changes.insert(
        a_uri,
        json!([
            {"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":3}},"newText":"new"},
            {"range":{"start":{"line":0,"character":4},"end":{"line":0,"character":7}},"newText":"new"}
        ]),
    );
    changes.insert(
        b_uri,
        json!([
            {"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":3}},"newText":"new"}
        ]),
    );

    let files = rename::workspace_edit_to_guarded_files(
        &workspace,
        &workspace_edit,
        rename::GuardedRenameRequest {
            old_name: "old",
            new_name: "new",
            target_path: "a.rs",
            target_sha: &target.sha256,
            encoding: "utf-8",
            max_files: 32,
        },
    )
    .unwrap();
    assert_eq!(files.len(), 2);
    let a = files.iter().find(|file| file["path"] == "a.rs").unwrap();
    assert_eq!(a["edits"].as_array().unwrap().len(), 1);
    assert_eq!(a["edits"][0]["old_text"], "old old");
    assert_eq!(a["edits"][0]["new_text"], "new new");
    assert_eq!(a["edits"][0]["start_line"], 1);
    assert_eq!(a["edits"][0]["end_line"], 1);

    let requests =
        serde_json::from_value::<Vec<crate::workspace::FileEditRequest>>(Value::Array(files))
            .unwrap();
    let applied = workspace.apply_file_edits(&requests).unwrap();
    assert!(applied.iter().all(|item| item.ok));
    assert_eq!(
        std::fs::read_to_string(root.path().join("a.rs")).unwrap(),
        "new new\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("b.rs")).unwrap(),
        "new\n"
    );
}

#[test]
fn semantic_rename_plan_rejects_external_files_and_resource_operations() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.rs"), "old\n").unwrap();
    std::fs::write(outside.path().join("outside.rs"), "old\n").unwrap();
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let target = workspace.load_source("a.rs").unwrap();
    let outside_uri = Url::from_file_path(outside.path().join("outside.rs"))
        .unwrap()
        .to_string();
    let mut external = json!({"changes": {}});
    external["changes"].as_object_mut().unwrap().insert(
        outside_uri,
        json!([
            {"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":3}},"newText":"new"}
        ]),
    );
    let error = rename::workspace_edit_to_guarded_files(
        &workspace,
        &external,
        rename::GuardedRenameRequest {
            old_name: "old",
            new_name: "new",
            target_path: "a.rs",
            target_sha: &target.sha256,
            encoding: "utf-8",
            max_files: 32,
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("outside the selected workspace"));

    let a_uri = Url::from_file_path(root.path().join("a.rs"))
        .unwrap()
        .to_string();
    let resource = json!({
        "documentChanges":[{
            "kind":"rename",
            "oldUri":a_uri,
            "newUri":format!("{}/renamed.rs", Url::from_directory_path(root.path()).unwrap())
        }]
    });
    let error = rename::workspace_edit_to_guarded_files(
        &workspace,
        &resource,
        rename::GuardedRenameRequest {
            old_name: "old",
            new_name: "new",
            target_path: "a.rs",
            target_sha: &target.sha256,
            encoding: "utf-8",
            max_files: 32,
        },
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("resource operations are not supported"));
}

#[test]
fn semantic_rename_plan_rejects_utf16_ranges_that_split_surrogate_pairs() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.rs"), "😀old\n").unwrap();
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let target = workspace.load_source("a.rs").unwrap();
    let uri = Url::from_file_path(root.path().join("a.rs"))
        .unwrap()
        .to_string();
    let mut workspace_edit = json!({"changes": {}});
    workspace_edit["changes"].as_object_mut().unwrap().insert(
        uri,
        json!([
            {"range":{"start":{"line":0,"character":1},"end":{"line":0,"character":4}},"newText":"new"}
        ]),
    );
    let error = rename::workspace_edit_to_guarded_files(
        &workspace,
        &workspace_edit,
        rename::GuardedRenameRequest {
            old_name: "old",
            new_name: "new",
            target_path: "a.rs",
            target_sha: &target.sha256,
            encoding: "utf-16",
            max_files: 32,
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("splits a surrogate pair"));
}

#[test]
fn semantic_rename_plan_withholds_sensitive_guarded_source() {
    let root = tempfile::tempdir().unwrap();
    let sentinel = "synthetic-private-fixture";
    std::fs::write(
        root.path().join("a.rs"),
        format!("let password = \"{sentinel}\"; old();\n"),
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, true).unwrap();
    let target = workspace.load_source("a.rs").unwrap();
    let uri = Url::from_file_path(root.path().join("a.rs"))
        .unwrap()
        .to_string();
    let mut workspace_edit = json!({"changes": {}});
    workspace_edit["changes"].as_object_mut().unwrap().insert(
        uri,
        json!([
            {"range":{"start":{"line":0,"character":44},"end":{"line":0,"character":47}},"newText":"new"}
        ]),
    );
    let error = rename::workspace_edit_to_guarded_files(
        &workspace,
        &workspace_edit,
        rename::GuardedRenameRequest {
            old_name: "old",
            new_name: "new",
            target_path: "a.rs",
            target_sha: &target.sha256,
            encoding: "utf-8",
            max_files: 32,
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("sensitive source"));
    assert!(!error.to_string().contains(sentinel));
}

#[test]
fn managed_lsp_destinations_stay_in_wcode_state_outside_the_repository() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.py"), "def f():\n    return 1\n").unwrap();
    let workspace = Workspace::new(root.path(), false, true).unwrap();
    let plan = install::install_plan(&workspace, SemanticLanguage::Python).unwrap();
    let destination = PathBuf::from(plan.destination.as_deref().unwrap());
    assert!(!destination.starts_with(workspace.root()));
    assert!(destination.to_string_lossy().contains("language-tools"));

    let candidates = install::managed_executable_candidates(&workspace, "pyright-langserver");
    assert!(candidates.iter().any(|path| path.starts_with(&destination)));
}

#[test]
fn rust_and_go_install_plans_follow_the_canonical_toolchains() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, true).unwrap();
    let rust = install::install_plan(&workspace, SemanticLanguage::Rust).unwrap();
    assert_eq!(rust.program.as_deref(), Some("rustup"));
    assert_eq!(rust.args, ["component", "add", "rust-analyzer", "rust-src"]);
    let go = install::install_plan(&workspace, SemanticLanguage::Go).unwrap();
    assert_eq!(go.program.as_deref(), Some("go"));
    assert_eq!(go.args, ["install", "golang.org/x/tools/gopls@latest"]);
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
    assert!(!dart.iter().any(|arg| arg.starts_with("--protocol")));
    assert!(dart.windows(2).any(|pair| pair == ["--client-id", "wcode"]));
    assert!(dart.iter().any(|arg| arg == "--client-version"));
    assert_eq!(
        provider_launch_args(&first, provider("r-languageserver")).unwrap(),
        ["--vanilla", "--no-echo", "-e", "languageserver::run()"]
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
        let diagnostics = client.diagnostics_for_uri(&uri, 2);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].get("code").and_then(Value::as_str),
            Some("mock.quickfix")
        );
        assert_eq!(
            diagnostics[0].pointer("/data/fix").and_then(Value::as_str),
            Some("mock")
        );
        assert!(client.diagnostics_for_uri(&uri, 1).is_empty());
        assert!(capabilities
            .pointer("/renameProvider/prepareProvider")
            .and_then(Value::as_bool)
            .unwrap_or(false));
        let prepared = client
            .request(
                "textDocument/prepareRename",
                json!({"textDocument":{"uri":uri},"position":{"line":0,"character":0}}),
            )
            .await
            .unwrap();
        assert_eq!(
            prepared
                .pointer("/range/end/character")
                .and_then(Value::as_u64),
            Some(3)
        );
        let rename = client
            .request(
                "textDocument/rename",
                json!({"textDocument":{"uri":uri},"position":{"line":0,"character":0},"newName":"three"}),
            )
            .await
            .unwrap();
        assert_eq!(
            rename
                .get("changes")
                .and_then(Value::as_object)
                .and_then(|changes| changes.get(&uri))
                .and_then(Value::as_array)
                .and_then(|edits| edits.first())
                .and_then(|edit| edit.get("newText"))
                .and_then(Value::as_str),
            Some("three")
        );
        assert!(capabilities.get("codeActionProvider").is_some());
        let code_actions = client
            .request(
                "textDocument/codeAction",
                json!({
                    "textDocument":{"uri":uri},
                    "range":{"start":{"line":0,"character":0},"end":{"line":0,"character":3}},
                    "context":{"diagnostics":[],"only":["source.organizeImports"],"triggerKind":1}
                }),
            )
            .await
            .unwrap();
        let action = code_actions
            .as_array()
            .and_then(|actions| actions.first())
            .unwrap();
        assert_eq!(action["kind"], "source.organizeImports");
        assert_eq!(
            action["edit"]["changes"]
                .as_object()
                .and_then(|changes| changes.get(&uri))
                .and_then(Value::as_array)
                .and_then(|edits| edits.first())
                .and_then(|edit| edit.get("newText"))
                .and_then(Value::as_str),
            Some("two")
        );
        let quick_fixes = client
            .request(
                "textDocument/codeAction",
                json!({
                    "textDocument":{"uri":uri},
                    "range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},
                    "context":{"diagnostics":diagnostics,"only":["quickfix"],"triggerKind":1}
                }),
            )
            .await
            .unwrap();
        let quick_fix = quick_fixes
            .as_array()
            .and_then(|actions| actions.first())
            .unwrap();
        assert_eq!(quick_fix["kind"], "quickfix");
        assert_eq!(quick_fix["isPreferred"], true);
        assert_eq!(
            quick_fix["edit"]["changes"]
                .as_object()
                .and_then(|changes| changes.get(&uri))
                .and_then(Value::as_array)
                .and_then(|edits| edits.first())
                .and_then(|edit| edit.get("newText"))
                .and_then(Value::as_str),
            Some("fixed")
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
fn rustup_proxy_detection_handles_symlinks_and_hardlinks() {
    use std::os::unix::fs::symlink;

    let tools = tempfile::tempdir().unwrap();
    let rustup = tools.path().join("rustup");
    std::fs::write(&rustup, "fixture").unwrap();
    let symlink_proxy = tools.path().join("rust-analyzer-link");
    symlink(&rustup, &symlink_proxy).unwrap();
    assert_eq!(
        discovery::rustup_proxy_path(&symlink_proxy).unwrap(),
        rustup.canonicalize().unwrap()
    );

    let hardlink_proxy = tools.path().join("rust-analyzer");
    std::fs::hard_link(&rustup, &hardlink_proxy).unwrap();
    assert_eq!(
        discovery::rustup_proxy_path(&hardlink_proxy).unwrap(),
        rustup
    );
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
        ("property.bats", SemanticLanguage::Bash),
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
fn automatic_semantics_trust_only_hardened_read_only_providers() {
    let rust = PROVIDERS
        .iter()
        .copied()
        .find(|provider| provider.id == "rust-analyzer")
        .unwrap();
    let gopls = PROVIDERS
        .iter()
        .copied()
        .find(|provider| provider.id == "gopls")
        .unwrap();
    let clangd = PROVIDERS
        .iter()
        .copied()
        .find(|provider| provider.id == "clangd")
        .unwrap();
    assert!(automatic_provider(rust));
    assert!(automatic_provider(gopls));
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
fn external_gopls_read_only_session_does_not_request_risky_execution() {
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

    authorize_provider_session(&workspace, provider, &gopls).unwrap();
    assert!(workspaces.latest_pending_authorization().is_none());
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

#[cfg(unix)]
#[test]
fn rustup_proxy_is_available_only_when_workspace_toolchain_has_rust_analyzer() {
    use std::os::unix::fs::{symlink, PermissionsExt};

    let workspace_dir = tempfile::tempdir().unwrap();
    let tools = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(workspace_dir.path(), false, true).unwrap();
    let rustup = tools.path().join("rustup");
    let proxy = tools.path().join("rust-analyzer");
    let component = tools.path().join("real-rust-analyzer");
    std::fs::write(&component, "fixture").unwrap();
    std::fs::write(
        &rustup,
        format!(
            "#!/bin/sh\nif [ \"$1\" = which ] && [ \"$2\" = rust-analyzer ]; then echo '{}'; exit 0; fi\nexit 1\n",
            component.display()
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&rustup).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&rustup, permissions).unwrap();
    symlink(&rustup, &proxy).unwrap();

    assert!(is_rustup_proxy(&proxy));
    assert!(rustup_proxy_component_ready_uncached(&workspace, &proxy));

    std::fs::write(&rustup, "#!/bin/sh\nexit 1\n").unwrap();
    let mut permissions = std::fs::metadata(&rustup).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&rustup, permissions).unwrap();
    assert!(!rustup_proxy_component_ready_uncached(&workspace, &proxy));
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

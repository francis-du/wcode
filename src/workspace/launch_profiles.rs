use super::*;

pub(super) const MAX_MANIFEST_BYTES: u64 = 256 * 1024;
pub(super) const MAX_PROFILES: usize = 8;
const MAX_DISCOVERY_ENTRIES: usize = 128;
const COMMON_SCRIPT_NAMES: &[&str] = &["dev", "start", "serve", "preview"];
const JUSTFILE_NAMES: &[&str] = &["Justfile", "justfile", ".justfile"];
const MAKEFILE_NAMES: &[&str] = &["GNUmakefile", "Makefile", "makefile"];
const TASKFILE_NAMES: &[&str] = &[
    "Taskfile.yml",
    "Taskfile.yaml",
    "taskfile.yml",
    "taskfile.yaml",
];
const COMPOSE_FILE_NAMES: &[&str] = &[
    "compose.yaml",
    "compose.yml",
    "docker-compose.yaml",
    "docker-compose.yml",
];
const MAX_COMPOSE_SERVICES: usize = 32;
const COMPOSE_STATUS_FORMAT: &str = "{{.Service}}\t{{.State}}\t{{.Health}}\t{{.ExitCode}}";

pub(super) fn discover(root: &Path) -> Vec<serde_json::Value> {
    let mut profiles = Vec::new();

    discover_cargo(root, &mut profiles);
    discover_go(root, &mut profiles);
    discover_package_scripts(root, &mut profiles);
    discover_deno_tasks(root, &mut profiles);
    discover_uv_scripts(root, &mut profiles);
    discover_dart_bin(root, &mut profiles);
    discover_compose(root, &mut profiles);
    discover_just_tasks(root, &mut profiles);
    discover_make_tasks(root, &mut profiles);
    discover_taskfile_tasks(root, &mut profiles);

    profiles.truncate(MAX_PROFILES);
    profiles
}

fn discover_cargo(root: &Path, profiles: &mut Vec<serde_json::Value>) {
    let Some(cargo) = bounded_text(&root.join("Cargo.toml")) else {
        return;
    };
    let Ok(document) = cargo.parse::<toml_edit::DocumentMut>() else {
        return;
    };
    let package = document
        .get("package")
        .and_then(|item| item.as_table_like());
    let has_default_run = package
        .and_then(|table| table.get("default-run"))
        .and_then(|item| item.as_str())
        .is_some_and(|name| !name.is_empty());
    if package.is_none() {
        return;
    }
    if has_default_run {
        profiles.push(profile(
            "cargo-run",
            "Cargo.toml#package.default-run",
            "cargo",
            vec!["run".to_owned()],
        ));
        return;
    }

    if let Some(bins) = document
        .get("bin")
        .and_then(|item| item.as_array_of_tables())
    {
        if bins.len() != 1 {
            return;
        }
        let Some(bin) = bins.iter().next() else {
            return;
        };
        if bin
            .get("required-features")
            .and_then(|item| item.as_array())
            .is_some_and(|features| !features.is_empty())
        {
            return;
        }
        let Some(name) = bin
            .get("name")
            .and_then(|item| item.as_str())
            .filter(|name| !name.is_empty())
        else {
            return;
        };
        profiles.push(profile(
            "cargo-run",
            "Cargo.toml#bin",
            "cargo",
            vec!["run".to_owned(), "--bin".to_owned(), name.to_owned()],
        ));
        return;
    }

    let auto_bins_enabled = package
        .and_then(|table| table.get("autobins"))
        .and_then(|item| item.as_bool())
        .unwrap_or(true);
    if auto_bins_enabled && inferred_cargo_binary_count(root) == 1 {
        profiles.push(profile(
            "cargo-run",
            "Cargo.toml",
            "cargo",
            vec!["run".to_owned()],
        ));
    }
}

fn discover_go(root: &Path, profiles: &mut Vec<serde_json::Value>) {
    if !regular_file(&root.join("go.mod")) {
        return;
    }
    let Some(target) = go_main_target(root) else {
        return;
    };
    profiles.push(profile(
        "go-run",
        "go.mod + package main",
        "go",
        vec!["run".to_owned(), target],
    ));
}

fn discover_package_scripts(root: &Path, profiles: &mut Vec<serde_json::Value>) {
    let Some(package) = bounded_text(&root.join("package.json")) else {
        return;
    };
    let Ok(package) = serde_json::from_str::<serde_json::Value>(&package) else {
        return;
    };
    let Some(scripts) = package
        .get("scripts")
        .and_then(serde_json::Value::as_object)
    else {
        return;
    };
    let Some(manager) = package_manager(root, &package) else {
        return;
    };
    for name in COMMON_SCRIPT_NAMES {
        if scripts
            .get(*name)
            .and_then(serde_json::Value::as_str)
            .is_some_and(|script| !script.trim().is_empty())
        {
            profiles.push(profile(
                &format!("{manager}-{name}"),
                &format!("package.json#scripts.{name}"),
                manager,
                vec!["run".to_owned(), (*name).to_owned()],
            ));
        }
    }
}

fn discover_deno_tasks(root: &Path, profiles: &mut Vec<serde_json::Value>) {
    let Some(deno) = bounded_text(&root.join("deno.json")) else {
        return;
    };
    let Ok(deno) = serde_json::from_str::<serde_json::Value>(&deno) else {
        return;
    };
    let Some(tasks) = deno.get("tasks").and_then(serde_json::Value::as_object) else {
        return;
    };
    for name in COMMON_SCRIPT_NAMES {
        if tasks
            .get(*name)
            .and_then(serde_json::Value::as_str)
            .is_some_and(|task| !task.trim().is_empty())
        {
            profiles.push(profile(
                &format!("deno-{name}"),
                &format!("deno.json#tasks.{name}"),
                "deno",
                vec!["task".to_owned(), (*name).to_owned()],
            ));
        }
    }
}

fn discover_uv_scripts(root: &Path, profiles: &mut Vec<serde_json::Value>) {
    if !regular_file(&root.join("uv.lock")) {
        return;
    }
    let Some(pyproject) = bounded_text(&root.join("pyproject.toml")) else {
        return;
    };
    let Ok(document) = pyproject.parse::<toml_edit::DocumentMut>() else {
        return;
    };
    let Some(project) = document
        .get("project")
        .and_then(|item| item.as_table_like())
    else {
        return;
    };
    let Some(scripts) = project.get("scripts").and_then(|item| item.as_table_like()) else {
        return;
    };
    for name in COMMON_SCRIPT_NAMES {
        if scripts
            .get(name)
            .and_then(|item| item.as_str())
            .is_some_and(|target| !target.trim().is_empty())
        {
            profiles.push(profile(
                &format!("uv-{name}"),
                &format!("pyproject.toml#project.scripts.{name}"),
                "uv",
                vec!["run".to_owned(), (*name).to_owned()],
            ));
        }
    }
}

fn discover_dart_bin(root: &Path, profiles: &mut Vec<serde_json::Value>) {
    let Some(pubspec) = bounded_text(&root.join("pubspec.yaml")) else {
        return;
    };
    let Ok(pubspec) = serde_yaml::from_str::<serde_json::Value>(&pubspec) else {
        return;
    };
    if !pubspec
        .get("name")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|name| !name.trim().is_empty())
    {
        return;
    }
    let Some(target) = unique_dart_bin(root) else {
        return;
    };
    profiles.push(profile(
        "dart-run",
        "pubspec.yaml + unique bin entry",
        "dart",
        vec!["run".to_owned(), target],
    ));
}

fn discover_compose(root: &Path, profiles: &mut Vec<serde_json::Value>) {
    let Some((manifest, compose)) = single_bounded_manifest(root, COMPOSE_FILE_NAMES) else {
        return;
    };
    if compose.contains('$') {
        return;
    }
    let Ok(document) = serde_yaml::from_str::<serde_json::Value>(&compose) else {
        return;
    };
    if !compose_manifest_is_bounded(&document) {
        return;
    }
    profiles.push(serde_json::json!({
        "id": "docker-compose-up",
        "source": manifest,
        "program": "docker",
        "args": ["compose", "up", "--no-build", "--pull", "never", "--abort-on-container-exit"],
        "cwd": ".",
        "task_mode_recommended": true,
        "auto_run": false,
        "authorization": "exact_risky_execution",
        "lifecycle": "attached-supervised-process-group",
        "program_available": program_available("docker"),
        "status_probe": {
            "program": "docker",
            "args": ["compose", "ps", "--all", "--orphans=false", "--format", COMPOSE_STATUS_FORMAT],
            "precision": "runtime_container_state_reported_health",
            "interpretation": "State and Docker-reported Health only; empty Health is unknown, not healthy",
            "auto_run": false,
        },
    }));
}

fn compose_manifest_is_bounded(document: &serde_json::Value) -> bool {
    let Some(root) = document.as_object() else {
        return false;
    };
    if ["name", "include", "secrets", "configs", "volumes"]
        .iter()
        .any(|field| root.contains_key(*field))
    {
        return false;
    }
    if root
        .get("networks")
        .is_some_and(|networks| !compose_networks_are_project_scoped(networks))
    {
        return false;
    }
    let Some(services) = root.get("services").and_then(serde_json::Value::as_object) else {
        return false;
    };
    !services.is_empty()
        && services.len() <= MAX_COMPOSE_SERVICES
        && services.values().all(compose_service_is_bounded)
}

fn compose_networks_are_project_scoped(networks: &serde_json::Value) -> bool {
    networks.as_object().is_some_and(|networks| {
        networks.len() <= MAX_COMPOSE_SERVICES
            && networks.values().all(|network| match network {
                serde_json::Value::Null => true,
                serde_json::Value::Object(options) => options.is_empty(),
                _ => false,
            })
    })
}

fn compose_service_is_bounded(service: &serde_json::Value) -> bool {
    let Some(service) = service.as_object() else {
        return false;
    };
    let literal_image = service
        .get("image")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|image| !image.trim().is_empty() && !image.contains('$'));
    if !literal_image
        || [
            "build",
            "privileged",
            "devices",
            "device_cgroup_rules",
            "cap_add",
            "security_opt",
            "network_mode",
            "pid",
            "ipc",
            "uts",
            "userns_mode",
            "env_file",
            "secrets",
            "configs",
            "extends",
            "profiles",
            "container_name",
            "develop",
            "deploy",
            "volumes",
            "volumes_from",
            "gpus",
            "runtime",
            "credential_spec",
            "post_start",
            "pre_start",
            "pre_stop",
            "use_api_socket",
        ]
        .iter()
        .any(|field| service.contains_key(*field))
    {
        return false;
    }
    if service
        .get("restart")
        .is_some_and(|restart| restart.as_str() != Some("no"))
    {
        return false;
    }
    service
        .get("ports")
        .is_none_or(compose_ports_are_loopback_only)
}

fn compose_ports_are_loopback_only(ports: &serde_json::Value) -> bool {
    ports.as_array().is_some_and(|ports| {
        !ports.is_empty()
            && ports.iter().all(|port| match port {
                serde_json::Value::String(value) => compose_short_port_is_loopback(value),
                serde_json::Value::Object(value) => {
                    value.keys().all(|key| {
                        matches!(
                            key.as_str(),
                            "target"
                                | "published"
                                | "host_ip"
                                | "protocol"
                                | "name"
                                | "app_protocol"
                        )
                    }) && value
                        .get("host_ip")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|host| matches!(host, "127.0.0.1" | "::1"))
                        && value.get("target").is_some_and(compose_port_number)
                        && value.get("published").is_some_and(compose_port_number)
                        && value
                            .get("protocol")
                            .and_then(serde_json::Value::as_str)
                            .is_none_or(|protocol| matches!(protocol, "tcp" | "udp"))
                }
                _ => false,
            })
    })
}

fn compose_port_number(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Number(value) => value
            .as_u64()
            .is_some_and(|port| (1..=u16::MAX.into()).contains(&port)),
        serde_json::Value::String(value) => value.parse::<u16>().is_ok_and(|port| port != 0),
        _ => false,
    }
}

fn compose_short_port_is_loopback(value: &str) -> bool {
    let rest = value
        .strip_prefix("127.0.0.1:")
        .or_else(|| value.strip_prefix("[::1]:"));
    let Some(rest) = rest else {
        return false;
    };
    let mut parts = rest.split(':');
    let published = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    let mut target_parts = target.split('/');
    let target_port = target_parts.next().unwrap_or_default();
    let protocol = target_parts.next();
    parts.next().is_none()
        && target_parts.next().is_none()
        && protocol.is_none_or(|protocol| matches!(protocol, "tcp" | "udp"))
        && published.parse::<u16>().is_ok_and(|port| port != 0)
        && target_port.parse::<u16>().is_ok_and(|port| port != 0)
}

fn discover_just_tasks(root: &Path, profiles: &mut Vec<serde_json::Value>) {
    let Some((manifest, justfile)) = single_bounded_manifest(root, JUSTFILE_NAMES) else {
        return;
    };
    let mut previous_significant_was_attribute = false;
    let mut discovered = [false; COMMON_SCRIPT_NAMES.len()];
    for line in justfile.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('[') {
            previous_significant_was_attribute = true;
            continue;
        }
        let attributed = previous_significant_was_attribute;
        previous_significant_was_attribute = false;
        if attributed || line.chars().next().is_some_and(char::is_whitespace) {
            continue;
        }
        for (index, name) in COMMON_SCRIPT_NAMES.iter().enumerate() {
            if !discovered[index]
                && line
                    .strip_prefix(name)
                    .and_then(|rest| rest.strip_prefix(':'))
                    .is_some()
            {
                profiles.push(profile(
                    &format!("just-{name}"),
                    &format!("{manifest}#{name}"),
                    "just",
                    vec![(*name).to_owned()],
                ));
                discovered[index] = true;
            }
        }
    }
}

fn discover_make_tasks(root: &Path, profiles: &mut Vec<serde_json::Value>) {
    let Some((manifest, makefile)) = single_bounded_manifest(root, MAKEFILE_NAMES) else {
        return;
    };
    let mut in_define = false;
    let mut discovered = [false; COMMON_SCRIPT_NAMES.len()];
    for line in makefile.lines() {
        let trimmed = line.trim();
        if in_define {
            if trimmed == "endef" {
                in_define = false;
            }
            continue;
        }
        if trimmed == "define" || trimmed.starts_with("define ") {
            in_define = true;
            continue;
        }
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || line.chars().next().is_some_and(char::is_whitespace)
        {
            continue;
        }
        for (index, name) in COMMON_SCRIPT_NAMES.iter().enumerate() {
            if discovered[index] {
                continue;
            }
            let Some(rest) = line
                .strip_prefix(name)
                .and_then(|rest| rest.strip_prefix(':'))
            else {
                continue;
            };
            if rest.starts_with('=')
                || !(rest.is_empty()
                    || rest.starts_with('#')
                    || rest.chars().next().is_some_and(char::is_whitespace))
            {
                continue;
            }
            profiles.push(profile(
                &format!("make-{name}"),
                &format!("{manifest}#{name}"),
                "make",
                vec![(*name).to_owned()],
            ));
            discovered[index] = true;
        }
    }
}

fn discover_taskfile_tasks(root: &Path, profiles: &mut Vec<serde_json::Value>) {
    let Some((manifest, taskfile)) = single_bounded_manifest(root, TASKFILE_NAMES) else {
        return;
    };
    let Ok(document) = serde_yaml::from_str::<serde_json::Value>(&taskfile) else {
        return;
    };
    let Some(tasks) = document.get("tasks").and_then(serde_json::Value::as_object) else {
        return;
    };
    for name in COMMON_SCRIPT_NAMES {
        if tasks.get(*name).is_some_and(taskfile_task_is_runnable) {
            profiles.push(profile(
                &format!("task-{name}"),
                &format!("{manifest}#tasks.{name}"),
                "task",
                vec![(*name).to_owned()],
            ));
        }
    }
}

fn taskfile_task_is_runnable(task: &serde_json::Value) -> bool {
    let Some(task) = task.as_object() else {
        return false;
    };
    ["cmds", "deps"].iter().any(|field| {
        task.get(*field)
            .and_then(serde_json::Value::as_array)
            .is_some_and(|steps| {
                steps.iter().any(|step| match step {
                    serde_json::Value::String(value) => !value.trim().is_empty(),
                    serde_json::Value::Object(value) => !value.is_empty(),
                    _ => false,
                })
            })
    })
}

fn profile(id: &str, source: &str, program: &str, args: Vec<String>) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "source": source,
        "program": program,
        "args": args,
        "cwd": ".",
        "task_mode_recommended": true,
        "auto_run": false,
        "program_available": program_available(program),
    })
}

fn program_available(program: &str) -> bool {
    if program.is_empty() || program.contains(['/', '\\']) {
        return false;
    }
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    #[cfg(windows)]
    let extensions = std::env::var_os("PATHEXT")
        .map(|value| {
            value
                .to_string_lossy()
                .split(';')
                .take(16)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec![".EXE".into(), ".CMD".into(), ".BAT".into()]);
    for directory in std::env::split_paths(&path).take(MAX_DISCOVERY_ENTRIES) {
        if executable_file(&directory.join(program)) {
            return true;
        }
        #[cfg(windows)]
        for extension in &extensions {
            if executable_file(&directory.join(format!("{program}{extension}"))) {
                return true;
            }
        }
    }
    false
}

fn executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn package_manager(root: &Path, package: &serde_json::Value) -> Option<&'static str> {
    if let Some(declared) = package.get("packageManager") {
        let declared = declared.as_str()?;
        let name = match declared.split_once('@') {
            Some((name, version))
                if !name.is_empty() && !version.is_empty() && !version.contains('@') =>
            {
                name
            }
            Some(_) => return None,
            None => declared,
        };
        return match name {
            "npm" => Some("npm"),
            "pnpm" => Some("pnpm"),
            "yarn" => Some("yarn"),
            "bun" => Some("bun"),
            _ => None,
        };
    }

    let lockfiles = [
        (
            "npm",
            regular_file(&root.join("package-lock.json"))
                || regular_file(&root.join("npm-shrinkwrap.json")),
        ),
        ("pnpm", regular_file(&root.join("pnpm-lock.yaml"))),
        ("yarn", regular_file(&root.join("yarn.lock"))),
        (
            "bun",
            regular_file(&root.join("bun.lock")) || regular_file(&root.join("bun.lockb")),
        ),
    ];
    let mut detected = lockfiles
        .into_iter()
        .filter_map(|(manager, present)| present.then_some(manager));
    let first = detected.next();
    if detected.next().is_some() {
        return None;
    }
    first.or(Some("npm"))
}

fn single_bounded_manifest(
    root: &Path,
    names: &'static [&'static str],
) -> Option<(&'static str, String)> {
    let mut selected = None::<(&'static str, String, PathBuf)>;
    for name in names {
        let path = root.join(name);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return None,
        };
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() > MAX_MANIFEST_BYTES
        {
            return None;
        }
        let canonical = path.canonicalize().ok()?;
        if let Some((_, _, selected_path)) = &selected {
            if selected_path == &canonical {
                continue;
            }
            return None;
        }
        selected = Some((*name, fs::read_to_string(path).ok()?, canonical));
    }
    selected.map(|(name, text, _)| (name, text))
}

fn bounded_text(path: &Path) -> Option<String> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_MANIFEST_BYTES
    {
        return None;
    }
    fs::read_to_string(path).ok()
}

fn regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
}

fn regular_dir(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
}

fn inferred_cargo_binary_count(root: &Path) -> usize {
    let mut count = usize::from(regular_file(&root.join("src/main.rs")));
    let Ok(mut entries) = fs::read_dir(root.join("src/bin")) else {
        return count;
    };
    for _ in 0..MAX_DISCOVERY_ENTRIES {
        let Some(entry) = entries.next() else {
            return count;
        };
        let Ok(entry) = entry else {
            return 2;
        };
        let path = entry.path();
        let direct_bin =
            path.extension().and_then(|ext| ext.to_str()) == Some("rs") && regular_file(&path);
        let nested_bin = regular_file(&path.join("main.rs"));
        if direct_bin || nested_bin {
            count += 1;
        }
        if count > 1 {
            return count;
        }
    }
    if entries.next().is_some() {
        2
    } else {
        count
    }
}

fn unique_dart_bin(root: &Path) -> Option<String> {
    let mut entries = fs::read_dir(root.join("bin")).ok()?;
    let mut target = None;
    for _ in 0..MAX_DISCOVERY_ENTRIES {
        let Some(entry) = entries.next() else {
            return target;
        };
        let entry = entry.ok()?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("dart") || !regular_file(&path) {
            continue;
        }
        let name = path.file_name()?.to_str()?;
        if target.is_some() {
            return None;
        }
        target = Some(format!("bin/{name}"));
    }
    if entries.next().is_some() {
        None
    } else {
        target
    }
}

fn go_main_target(root: &Path) -> Option<String> {
    match go_directory_is_main_package(root) {
        Some(true) => return Some(".".to_owned()),
        Some(false) => {}
        None => return None,
    }

    let mut targets = Vec::new();
    let mut entries = fs::read_dir(root.join("cmd")).ok()?;
    for _ in 0..MAX_DISCOVERY_ENTRIES {
        let Some(entry) = entries.next() else {
            return targets.pop();
        };
        let entry = entry.ok()?;
        let path = entry.path();
        if !regular_dir(&path) {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        match go_directory_is_main_package(&path) {
            Some(true) => {
                targets.push(format!("./cmd/{name}"));
                if targets.len() > 1 {
                    return None;
                }
            }
            Some(false) => {}
            None => return None,
        }
    }
    if entries.next().is_some() {
        None
    } else {
        targets.pop()
    }
}

fn go_directory_is_main_package(root: &Path) -> Option<bool> {
    let mut entries = fs::read_dir(root).ok()?;
    let mut found_main = false;
    for _ in 0..MAX_DISCOVERY_ENTRIES {
        let Some(entry) = entries.next() else {
            return Some(found_main);
        };
        let entry = entry.ok()?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.ends_with(".go") || name.ends_with("_test.go") {
            continue;
        }
        let source = bounded_text(&path)?;
        if source.lines().any(|line| line.trim() == "package main") {
            found_main = true;
        }
    }
    if entries.next().is_some() {
        None
    } else {
        Some(found_main)
    }
}

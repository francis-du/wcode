use super::*;
use std::path::Component;

pub(super) const MAX_CONTRACT_CONFIGS: usize = 64;
const MAX_CONTRACT_BRIDGES: usize = 128;
const MAX_CONTRACT_DIAGNOSTICS: usize = 32;
const MAX_CONTRACT_FRESHNESS_ADVISORIES: usize = 32;
const MAX_CONTRACT_CONFIG_BYTES: u64 = 512 * 1024;

const STATIC_CONTRACT_CONFIGS: &[&str] = &[
    "codegen.yml",
    "codegen.yaml",
    "codegen.json",
    ".graphqlrc.yml",
    ".graphqlrc.yaml",
    ".graphqlrc.json",
    "buf.gen.yaml",
    "buf.gen.yml",
    "buf.yaml",
    "buf.yml",
    "openapi-generator.yaml",
    "openapi-generator.yml",
    "openapi-generator.json",
    "openapi-generator-config.yaml",
    "openapi-generator-config.yml",
    "openapi-generator-config.json",
    "openapi-generator-batch.yaml",
    "openapi-generator-batch.yml",
    "openapi-generator-batch.json",
];

const DYNAMIC_GRAPHQL_CONFIGS: &[&str] = &[
    "codegen.ts",
    "codegen.js",
    "codegen.mts",
    "codegen.cts",
    "codegen.mjs",
    "codegen.cjs",
];

pub(super) fn discover_contract_topology_from_paths(
    workspace_root: &Path,
    islands: &[ProjectIsland],
    config_paths: &[PathBuf],
) -> ProjectContractTopology {
    let mut topology = ProjectContractTopology {
        bridges: Vec::new(),
        diagnostics: Vec::new(),
        truncated: false,
        provider: "contract-config",
        precision: "structural",
    };
    let mut configs = config_paths.to_vec();
    if configs.len() > MAX_CONTRACT_CONFIGS {
        configs.truncate(MAX_CONTRACT_CONFIGS);
        topology.truncated = true;
    }

    for config in configs {
        let name = config
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let relative = portable_relative(workspace_root, &config);
        if DYNAMIC_GRAPHQL_CONFIGS.contains(&name) {
            push_diagnostic(
                &mut topology,
                relative,
                "dynamic_graphql_config_not_executed",
            );
            continue;
        }
        if matches!(name, "buf.yaml" | "buf.yml") {
            continue;
        }
        let Some(content) = read_static_config(&config) else {
            push_diagnostic(
                &mut topology,
                relative,
                "static_config_unreadable_or_too_large",
            );
            continue;
        };
        let parsed = if name.ends_with(".json") {
            serde_json::from_str::<Value>(&content).ok()
        } else {
            serde_yaml::from_str::<Value>(&content).ok()
        };
        let Some(value) = parsed else {
            push_diagnostic(&mut topology, relative, "invalid_static_config");
            continue;
        };
        let before = topology.bridges.len();
        if name.starts_with("codegen.") || name.starts_with(".graphqlrc.") {
            graphql_codegen_bridges(workspace_root, &config, &value, islands, &mut topology);
        } else if matches!(name, "buf.gen.yaml" | "buf.gen.yml") {
            buf_codegen_bridges(workspace_root, &config, &value, islands, &mut topology);
        } else if name.starts_with("openapi-generator") {
            openapi_codegen_bridges(workspace_root, &config, &value, islands, &mut topology);
        }
        if topology.bridges.len() == before && !matches!(name, "buf.yaml" | "buf.yml") {
            push_diagnostic(&mut topology, relative, "no_local_contract_bridge");
        }
        if topology.bridges.len() >= MAX_CONTRACT_BRIDGES {
            topology.bridges.truncate(MAX_CONTRACT_BRIDGES);
            topology.truncated = true;
            break;
        }
    }

    topology.bridges.sort_by(|left, right| {
        (
            &left.source,
            left.source_kind,
            &left.consumer_island,
            &left.output,
            left.kind,
            &left.evidence,
        )
            .cmp(&(
                &right.source,
                right.source_kind,
                &right.consumer_island,
                &right.output,
                right.kind,
                &right.evidence,
            ))
    });
    topology.bridges.dedup_by(|left, right| {
        left.source == right.source
            && left.source_kind == right.source_kind
            && left.consumer_island == right.consumer_island
            && left.output == right.output
            && left.kind == right.kind
            && left.evidence == right.evidence
    });
    topology
}

pub(super) fn is_contract_config_name(name: &str) -> bool {
    STATIC_CONTRACT_CONFIGS.contains(&name) || DYNAMIC_GRAPHQL_CONFIGS.contains(&name)
}

pub(super) fn matching_contract_bridges<'a>(
    topology: &'a ProjectContractTopology,
    changed_path: &str,
) -> Vec<&'a ProjectContractBridge> {
    topology
        .bridges
        .iter()
        .filter(|bridge| bridge_source_matches(bridge, changed_path))
        .collect()
}

#[derive(Clone, Debug)]
pub(crate) struct ContractFreshnessAdvisory {
    pub(crate) source: String,
    pub(crate) config: String,
    pub(crate) output: String,
    pub(crate) consumer_island: String,
    pub(crate) kind: &'static str,
    pub(crate) evidence: String,
}

pub(crate) fn contract_freshness_advisories(
    workspace_root: &Path,
    topology: &ProjectContractTopology,
    changed_paths: &BTreeSet<String>,
) -> Vec<ContractFreshnessAdvisory> {
    let mut advisories = topology
        .bridges
        .iter()
        .filter_map(|bridge| {
            let config = bridge_config_path(bridge);
            let upstream_changed = changed_paths
                .iter()
                .any(|path| bridge_source_matches(bridge, path) || path == &config);
            if !upstream_changed
                || changed_paths
                    .iter()
                    .any(|path| path_matches_output(&bridge.output, path))
                || !existing_generated_output(workspace_root, &bridge.output)
            {
                return None;
            }
            Some(ContractFreshnessAdvisory {
                source: bridge.source.clone(),
                config,
                output: bridge.output.clone(),
                consumer_island: bridge.consumer_island.clone(),
                kind: bridge.kind,
                evidence: bridge.evidence.clone(),
            })
        })
        .collect::<Vec<_>>();
    advisories.sort_by(|left, right| {
        (
            &left.output,
            &left.source,
            &left.consumer_island,
            left.kind,
            &left.evidence,
        )
            .cmp(&(
                &right.output,
                &right.source,
                &right.consumer_island,
                right.kind,
                &right.evidence,
            ))
    });
    advisories.dedup_by(|left, right| {
        left.output == right.output
            && left.source == right.source
            && left.consumer_island == right.consumer_island
            && left.kind == right.kind
            && left.evidence == right.evidence
    });
    advisories.truncate(MAX_CONTRACT_FRESHNESS_ADVISORIES);
    advisories
}

fn bridge_config_path(bridge: &ProjectContractBridge) -> String {
    bridge
        .evidence
        .split_once(':')
        .map(|(path, _)| path)
        .unwrap_or(bridge.evidence.as_str())
        .to_owned()
}

fn path_matches_output(output: &str, path: &str) -> bool {
    path == output
        || path
            .strip_prefix(output)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn existing_generated_output(workspace_root: &Path, output: &str) -> bool {
    std::fs::symlink_metadata(workspace_root.join(output)).is_ok_and(|metadata| {
        !metadata.file_type().is_symlink() && (metadata.is_file() || metadata.is_dir())
    })
}

fn graphql_codegen_bridges(
    workspace_root: &Path,
    config: &Path,
    value: &Value,
    islands: &[ProjectIsland],
    topology: &mut ProjectContractTopology,
) {
    let Some(generates) = value.get("generates").and_then(Value::as_object) else {
        return;
    };
    let global_schema = value
        .get("schema")
        .map(|value| local_sources(workspace_root, config, value))
        .unwrap_or_default();
    let global_documents = value
        .get("documents")
        .map(|value| local_sources(workspace_root, config, value))
        .unwrap_or_default();

    for (output, config_value) in generates {
        let Some(output) = local_output(workspace_root, config, output) else {
            continue;
        };
        let Some(consumer_island) = owning_island(islands, &output) else {
            continue;
        };
        let schema = config_value
            .get("schema")
            .map(|value| local_sources(workspace_root, config, value))
            .filter(|sources| !sources.is_empty())
            .unwrap_or_else(|| global_schema.clone());
        let documents = config_value
            .get("documents")
            .map(|value| local_sources(workspace_root, config, value))
            .filter(|sources| !sources.is_empty())
            .unwrap_or_else(|| global_documents.clone());
        for (source, source_kind) in schema {
            push_bridge(
                topology,
                ProjectContractBridge {
                    source,
                    source_kind,
                    output: output.clone(),
                    consumer_island: consumer_island.clone(),
                    kind: "graphql_schema_codegen",
                    evidence: format!(
                        "{}:generates[{output}].schema",
                        portable_relative(workspace_root, config)
                    ),
                    provider: "contract-config",
                    precision: "structural",
                },
            );
        }
        for (source, source_kind) in documents {
            push_bridge(
                topology,
                ProjectContractBridge {
                    source,
                    source_kind,
                    output: output.clone(),
                    consumer_island: consumer_island.clone(),
                    kind: "graphql_documents_codegen",
                    evidence: format!(
                        "{}:generates[{output}].documents",
                        portable_relative(workspace_root, config)
                    ),
                    provider: "contract-config",
                    precision: "structural",
                },
            );
        }
    }
}

fn openapi_codegen_bridges(
    workspace_root: &Path,
    config: &Path,
    value: &Value,
    islands: &[ProjectIsland],
    topology: &mut ProjectContractTopology,
) {
    let mut candidates = Vec::new();
    candidates.push(value);
    if let Some(object) = value.as_object() {
        candidates.extend(object.values().filter(|value| value.is_object()));
    }
    for candidate in candidates {
        let Some(input) = candidate.get("inputSpec").and_then(Value::as_str) else {
            continue;
        };
        let Some(output) = candidate.get("outputDir").and_then(Value::as_str) else {
            continue;
        };
        if candidate
            .get("generatorName")
            .and_then(Value::as_str)
            .is_none()
        {
            continue;
        }
        let Some((source, source_kind)) = local_source(workspace_root, config, input) else {
            continue;
        };
        let Some(output) = local_output(workspace_root, config, output) else {
            continue;
        };
        let Some(consumer_island) = owning_island(islands, &output) else {
            continue;
        };
        push_bridge(
            topology,
            ProjectContractBridge {
                source,
                source_kind,
                output,
                consumer_island,
                kind: "openapi_codegen",
                evidence: format!(
                    "{}:inputSpec+outputDir",
                    portable_relative(workspace_root, config)
                ),
                provider: "contract-config",
                precision: "structural",
            },
        );
    }
}

fn buf_codegen_bridges(
    workspace_root: &Path,
    config: &Path,
    value: &Value,
    islands: &[ProjectIsland],
    topology: &mut ProjectContractTopology,
) {
    let outputs = value
        .get("plugins")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|plugin| plugin.get("out").and_then(Value::as_str))
        .filter_map(|output| local_output(workspace_root, config, output))
        .collect::<Vec<_>>();
    if outputs.is_empty() {
        return;
    }
    let mut sources = value
        .get("inputs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|input| input.get("directory").and_then(Value::as_str))
        .filter_map(|directory| local_source(workspace_root, config, directory))
        .collect::<Vec<_>>();
    if sources.is_empty() {
        sources = buf_module_sources(workspace_root, config);
    }
    for output in outputs {
        let Some(consumer_island) = owning_island(islands, &output) else {
            continue;
        };
        for (source, source_kind) in &sources {
            push_bridge(
                topology,
                ProjectContractBridge {
                    source: source.clone(),
                    source_kind,
                    output: output.clone(),
                    consumer_island: consumer_island.clone(),
                    kind: "protobuf_codegen",
                    evidence: format!(
                        "{}:plugins[].out",
                        portable_relative(workspace_root, config)
                    ),
                    provider: "contract-config",
                    precision: "structural",
                },
            );
        }
    }
}

fn buf_module_sources(workspace_root: &Path, config: &Path) -> Vec<(String, &'static str)> {
    let Some(parent) = config.parent() else {
        return Vec::new();
    };
    for name in ["buf.yaml", "buf.yml"] {
        let path = parent.join(name);
        let Some(content) = read_static_config(&path) else {
            continue;
        };
        let Ok(value) = serde_yaml::from_str::<Value>(&content) else {
            continue;
        };
        let mut sources = value
            .get("modules")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|module| module.get("path").and_then(Value::as_str))
            .filter_map(|path| local_source(workspace_root, &path_to_config(parent, name), path))
            .collect::<Vec<_>>();
        if sources.is_empty() {
            if let Some(roots) = value
                .get("build")
                .and_then(|build| build.get("roots"))
                .and_then(Value::as_array)
            {
                sources.extend(roots.iter().filter_map(Value::as_str).filter_map(|path| {
                    local_source(workspace_root, &path_to_config(parent, name), path)
                }));
            }
        }
        if !sources.is_empty() {
            return sources;
        }
    }
    Vec::new()
}

fn path_to_config(parent: &Path, name: &str) -> PathBuf {
    parent.join(name)
}

fn local_sources(
    workspace_root: &Path,
    config: &Path,
    value: &Value,
) -> Vec<(String, &'static str)> {
    let mut raw = Vec::new();
    collect_pointer_strings(value, &mut raw);
    let mut sources = raw
        .into_iter()
        .filter_map(|pointer| local_source(workspace_root, config, pointer))
        .collect::<Vec<_>>();
    sources.sort();
    sources.dedup();
    sources
}

fn collect_pointer_strings<'a>(value: &'a Value, output: &mut Vec<&'a str>) {
    match value {
        Value::String(value) => output.push(value),
        Value::Array(values) => {
            for value in values {
                collect_pointer_strings(value, output);
            }
        }
        Value::Object(values) => output.extend(values.keys().map(String::as_str)),
        _ => {}
    }
}

fn local_source(workspace_root: &Path, config: &Path, raw: &str) -> Option<(String, &'static str)> {
    let raw = raw.trim();
    if rejected_pointer(raw) {
        return None;
    }
    let base = config_base(workspace_root, config)?;
    let (candidate, source_kind) = if let Some(index) = raw.find(['*', '?', '[', '{']) {
        let prefix = raw[..index]
            .rfind('/')
            .map(|index| &raw[..index])
            .unwrap_or("");
        if prefix.is_empty() {
            (base.clone(), "directory")
        } else {
            (base.join(prefix), "directory")
        }
    } else {
        (base.join(raw), "file")
    };
    let normalized = normalize_workspace_relative(&candidate)?;
    if normalized == "." {
        return None;
    }
    if has_symlink_component(workspace_root, &normalized) {
        return None;
    }
    let metadata = std::fs::symlink_metadata(workspace_root.join(&normalized)).ok()?;
    if metadata.file_type().is_symlink() {
        return None;
    }
    if source_kind == "directory" {
        metadata.is_dir().then_some((normalized, source_kind))
    } else if metadata.is_dir() {
        Some((normalized, "directory"))
    } else {
        metadata.is_file().then_some((normalized, source_kind))
    }
}

fn local_output(workspace_root: &Path, config: &Path, raw: &str) -> Option<String> {
    let raw = raw.trim();
    if rejected_pointer(raw) || raw.contains(['*', '?', '[', '{']) {
        return None;
    }
    let base = config_base(workspace_root, config)?;
    let normalized = normalize_workspace_relative(&base.join(raw))?;
    (!has_symlink_component(workspace_root, &normalized)).then_some(normalized)
}

fn config_base(workspace_root: &Path, config: &Path) -> Option<PathBuf> {
    let parent = config.parent()?;
    let relative = parent.strip_prefix(workspace_root).ok()?;
    Some(relative.to_path_buf())
}

fn rejected_pointer(raw: &str) -> bool {
    raw.is_empty()
        || raw.starts_with('/')
        || raw.contains("://")
        || raw.starts_with("http:")
        || raw.starts_with("https:")
        || raw.starts_with("git:")
        || raw.starts_with("npm:")
        || raw.contains("${")
}

fn normalize_workspace_relative(path: &Path) -> Option<String> {
    let mut normalized = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(component) => {
                normalized.push(component.to_string_lossy().to_string())
            }
            Component::ParentDir => {
                normalized.pop()?;
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(if normalized.is_empty() {
        ".".to_owned()
    } else {
        normalized.join("/")
    })
}

fn has_symlink_component(workspace_root: &Path, relative: &str) -> bool {
    let mut current = workspace_root.to_path_buf();
    for component in Path::new(relative).components() {
        let Component::Normal(component) = component else {
            continue;
        };
        current.push(component);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => return true,
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => return true,
        }
    }
    false
}

fn owning_island(islands: &[ProjectIsland], path: &str) -> Option<String> {
    islands
        .iter()
        .filter(|island| {
            island.root == "."
                || path == island.root
                || path
                    .strip_prefix(&island.root)
                    .is_some_and(|suffix| suffix.starts_with('/'))
        })
        .max_by_key(|island| {
            if island.root == "." {
                0
            } else {
                island.root.split('/').count()
            }
        })
        .map(|island| island.id.clone())
}

fn bridge_source_matches(bridge: &ProjectContractBridge, path: &str) -> bool {
    if bridge.source_kind == "file" {
        return path == bridge.source;
    }
    path == bridge.source
        || path
            .strip_prefix(&bridge.source)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn read_static_config(path: &Path) -> Option<String> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_CONTRACT_CONFIG_BYTES
    {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

fn push_bridge(topology: &mut ProjectContractTopology, bridge: ProjectContractBridge) {
    if topology.bridges.len() < MAX_CONTRACT_BRIDGES {
        topology.bridges.push(bridge);
    } else {
        topology.truncated = true;
    }
}

fn push_diagnostic(topology: &mut ProjectContractTopology, path: String, reason: &'static str) {
    if topology.diagnostics.len() < MAX_CONTRACT_DIAGNOSTICS {
        topology
            .diagnostics
            .push(ProjectContractDiagnostic { path, reason });
    } else {
        topology.truncated = true;
    }
}

fn portable_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .ok()
        .map(|path| {
            path.components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        })
        .filter(|path| !path.is_empty())
        .unwrap_or_else(|| ".".to_owned())
}

pub(super) fn contract_excluded_directory(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".wcode"
            | "target"
            | "node_modules"
            | ".venv"
            | "venv"
            | "dist"
            | ".next"
            | ".cache"
            | "vendor"
    )
}

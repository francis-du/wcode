use serde_json::Value;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkloadResources {
    pub workspace: String,
    pub reads: Vec<PathBuf>,
    pub writes: Vec<PathBuf>,
    pub creates: Vec<PathBuf>,
    pub moves_from: Vec<PathBuf>,
    pub moves_to: Vec<PathBuf>,
    pub deletes: Vec<PathBuf>,
}

impl WorkloadResources {
    fn mutations(&self) -> impl Iterator<Item = &PathBuf> {
        self.writes
            .iter()
            .chain(&self.creates)
            .chain(&self.moves_from)
            .chain(&self.moves_to)
            .chain(&self.deletes)
    }

    fn all_paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.reads.iter().chain(self.mutations())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DependencyGraph {
    pub predecessors: Vec<BTreeSet<usize>>,
}

impl DependencyGraph {
    pub fn layers(&self, active: &BTreeSet<usize>) -> Result<Vec<Vec<usize>>, String> {
        let mut remaining = active.clone();
        let mut completed = BTreeSet::new();
        let mut layers = Vec::new();
        while !remaining.is_empty() {
            let ready = remaining
                .iter()
                .copied()
                .filter(|index| {
                    self.predecessors
                        .get(*index)
                        .is_none_or(|dependencies| dependencies.is_subset(&completed))
                })
                .collect::<Vec<_>>();
            if ready.is_empty() {
                return Err("scheduler dependency graph contains a cycle".to_owned());
            }
            for index in &ready {
                remaining.remove(index);
                completed.insert(*index);
            }
            layers.push(ready);
        }
        Ok(layers)
    }
}

pub type CoalesceResult = (
    Vec<Value>,
    HashMap<usize, Vec<(usize, String)>>,
    HashSet<usize>,
);

pub fn coalesce_apply_edits(
    default_workspace: &str,
    items: &[Value],
) -> Result<CoalesceResult, String> {
    let mut prepared = items.to_vec();
    let mut first_by_path = HashMap::<String, (String, usize)>::new();
    let mut aliases = HashMap::<usize, Vec<(usize, String)>>::new();
    let mut skipped = HashSet::new();

    for (index, item) in items.iter().enumerate() {
        if item.get("tool").and_then(Value::as_str) != Some("apply_edits") {
            continue;
        }
        let args = item
            .get("arguments")
            .and_then(Value::as_object)
            .ok_or("parallel apply_edits arguments must be an object")?;
        let path = args
            .get("path")
            .and_then(Value::as_str)
            .ok_or("parallel apply_edits requires path")?;
        let normalized = normalize_path(path)?;
        let expected = args
            .get("expected_sha256")
            .and_then(Value::as_str)
            .ok_or("parallel apply_edits requires expected_sha256")?;
        let workspace = args
            .get("workspace")
            .and_then(Value::as_str)
            .unwrap_or(default_workspace);
        let path_key = format!("{workspace}\0{}", portable_path(&normalized));
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("task-{}", index + 1));

        if let Some((first_expected, first)) = first_by_path.get(&path_key).cloned() {
            if first_expected != expected {
                return Err(format!(
                    "parallel apply_edits for the same file require the same expected_sha256; tasks {} and {} use different revisions",
                    first + 1,
                    index + 1
                ));
            }
            let extra = item
                .pointer("/arguments/edits")
                .and_then(Value::as_array)
                .ok_or("parallel apply_edits requires edits")?
                .clone();
            let target = prepared[first]
                .pointer_mut("/arguments/edits")
                .and_then(Value::as_array_mut)
                .ok_or("coalesced apply_edits target is missing edits")?;
            validate_merge(target, &extra)?;
            target.extend(extra);
            aliases.entry(first).or_default().push((index, id));
            skipped.insert(index);
        } else {
            first_by_path.insert(path_key, (expected.to_owned(), index));
        }
    }
    Ok((prepared, aliases, skipped))
}

pub fn resource_model(
    default_workspace: &str,
    tool: &str,
    args: &Value,
) -> Result<WorkloadResources, String> {
    if !args.is_object() {
        return Err("parallel task arguments must be an object".to_owned());
    }
    let mut resources = WorkloadResources {
        workspace: args
            .get("workspace")
            .and_then(Value::as_str)
            .unwrap_or(default_workspace)
            .to_owned(),
        ..WorkloadResources::default()
    };
    let push = |target: &mut Vec<PathBuf>, value: &str| -> Result<(), String> {
        target.push(normalize_path(value)?);
        Ok(())
    };
    match tool {
        "read_file" | "path_info" | "file_outline" => {
            push(&mut resources.reads, required_path(args, "path")?)?;
        }
        "read_files" => {
            for value in required_array(args, "paths", "parallel read_files requires paths")? {
                push(
                    &mut resources.reads,
                    value
                        .as_str()
                        .ok_or("parallel read_files path must be a string")?,
                )?;
            }
        }
        "list_files" | "search_code" | "search_many" | "find_symbol" => {
            push(
                &mut resources.reads,
                args.get("path").and_then(Value::as_str).unwrap_or("."),
            )?;
        }
        "replace_text" | "apply_edits" | "write_file" => {
            push(&mut resources.writes, required_path(args, "path")?)?;
        }
        "apply_file_edits" => {
            for value in required_array(args, "files", "parallel file batch requires files")? {
                push(
                    &mut resources.writes,
                    value
                        .get("path")
                        .and_then(Value::as_str)
                        .ok_or("parallel file batch item requires path")?,
                )?;
            }
        }
        "create_directory" | "create_file" => {
            push(&mut resources.creates, required_path(args, "path")?)?;
        }
        "create_files" => {
            for value in required_array(args, "files", "parallel file batch requires files")? {
                push(
                    &mut resources.creates,
                    value
                        .get("path")
                        .and_then(Value::as_str)
                        .ok_or("parallel file batch item requires path")?,
                )?;
            }
        }
        "move_path" => {
            push(&mut resources.moves_from, required_path(args, "source")?)?;
            push(&mut resources.moves_to, required_path(args, "destination")?)?;
        }
        "move_paths" => {
            for value in required_array(args, "moves", "parallel move_paths requires moves")? {
                push(
                    &mut resources.moves_from,
                    value
                        .get("source")
                        .and_then(Value::as_str)
                        .ok_or("parallel move item requires source")?,
                )?;
                push(
                    &mut resources.moves_to,
                    value
                        .get("destination")
                        .and_then(Value::as_str)
                        .ok_or("parallel move item requires destination")?,
                )?;
            }
        }
        "delete_path" => {
            push(&mut resources.deletes, required_path(args, "path")?)?;
        }
        _ => {
            // Design, graph, semantic and context reads can inspect the whole workspace.
            resources.reads.push(PathBuf::new());
        }
    }
    Ok(resources)
}

pub fn dependency_graph(
    workloads: &[(usize, WorkloadResources)],
    task_count: usize,
) -> DependencyGraph {
    let mut predecessors = vec![BTreeSet::new(); task_count];
    for left_offset in 0..workloads.len() {
        for right_offset in left_offset + 1..workloads.len() {
            let (left_index, left) = &workloads[left_offset];
            let (right_index, right) = &workloads[right_offset];
            if left.workspace != right.workspace || !resources_conflict(left, right) {
                continue;
            }
            if creates_parent_of(left, right) && !creates_parent_of(right, left) {
                predecessors[*right_index].insert(*left_index);
            } else if creates_parent_of(right, left) && !creates_parent_of(left, right) {
                predecessors[*left_index].insert(*right_index);
            } else {
                predecessors[*right_index].insert(*left_index);
            }
        }
    }
    DependencyGraph { predecessors }
}

fn resources_conflict(left: &WorkloadResources, right: &WorkloadResources) -> bool {
    let left_mutations = left.mutations().collect::<Vec<_>>();
    let right_mutations = right.mutations().collect::<Vec<_>>();
    if left_mutations.is_empty() && right_mutations.is_empty() {
        return false;
    }
    left_mutations.iter().any(|left_path| {
        right
            .all_paths()
            .any(|right_path| paths_overlap(left_path, right_path))
    }) || right_mutations.iter().any(|right_path| {
        left.reads
            .iter()
            .any(|left_path| paths_overlap(left_path, right_path))
    })
}

fn creates_parent_of(creator: &WorkloadResources, other: &WorkloadResources) -> bool {
    creator.creates.iter().any(|created| {
        other.all_paths().any(|path| {
            created != path && !created.as_os_str().is_empty() && path.starts_with(created)
        })
    })
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    left.as_os_str().is_empty()
        || right.as_os_str().is_empty()
        || left == right
        || left.starts_with(right)
        || right.starts_with(left)
}

fn normalize_path(path: &str) -> Result<PathBuf, String> {
    if path.contains('\0') || path.contains(['\n', '\r']) {
        return Err("parallel task path contains forbidden control characters".to_owned());
    }
    if path.trim().is_empty() || path == "." {
        return Ok(PathBuf::new());
    }
    let candidate = PathBuf::from(path);
    if candidate.is_absolute() {
        return Err("parallel task paths must stay workspace-relative".to_owned());
    }
    let mut normalized = PathBuf::new();
    for component in candidate.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => normalized.push(value),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err("parallel task path traversal is not allowed".to_owned());
            }
        }
    }
    Ok(normalized)
}

fn required_path<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("parallel task requires {key}"))
}

fn required_array<'a>(
    args: &'a Value,
    key: &str,
    error: &'static str,
) -> Result<&'a Vec<Value>, String> {
    args.get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| error.to_owned())
}

fn portable_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn validate_merge(existing: &[Value], extra: &[Value]) -> Result<(), String> {
    for (left_index, left) in existing.iter().enumerate() {
        for (right_index, right) in extra.iter().enumerate() {
            let left_old = left
                .get("old_text")
                .and_then(Value::as_str)
                .ok_or("coalesced apply_edits entry requires old_text")?;
            let right_old = right
                .get("old_text")
                .and_then(Value::as_str)
                .ok_or("coalesced apply_edits entry requires old_text")?;
            let left_range = edit_line_range(left)?;
            let right_range = edit_line_range(right)?;
            if ranges_overlap(left_range, right_range) {
                return Err(format!(
                    "same-file apply_edits contain overlapping line ranges (existing edit {}, incoming edit {})",
                    left_index + 1,
                    right_index + 1
                ));
            }
            if left_old == right_old && (left_range.is_none() || right_range.is_none()) {
                return Err("same-file apply_edits contain ambiguous duplicate old_text without disjoint line bounds".to_owned());
            }
        }
    }
    Ok(())
}

fn edit_line_range(edit: &Value) -> Result<Option<(u64, u64)>, String> {
    let start = edit.get("start_line").and_then(Value::as_u64);
    let end = edit.get("end_line").and_then(Value::as_u64);
    match (start, end) {
        (None, None) => Ok(None),
        (Some(start), Some(end)) if start > 0 && end >= start => Ok(Some((start, end))),
        (Some(_), Some(_)) => Err("coalesced edit line range is invalid".to_owned()),
        _ => Err("coalesced edit start_line and end_line must be supplied together".to_owned()),
    }
}

fn ranges_overlap(left: Option<(u64, u64)>, right: Option<(u64, u64)>) -> bool {
    match (left, right) {
        (Some((left_start, left_end)), Some((right_start, right_end))) => {
            left_start <= right_end && right_start <= left_end
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn model(tool: &str, arguments: Value) -> WorkloadResources {
        resource_model("demo", tool, &arguments).unwrap()
    }

    fn layers(workloads: Vec<WorkloadResources>) -> Vec<Vec<usize>> {
        let indexed = workloads.into_iter().enumerate().collect::<Vec<_>>();
        dependency_graph(&indexed, indexed.len())
            .layers(&(0..indexed.len()).collect())
            .unwrap()
    }

    #[test]
    fn independent_reads_and_writes_fan_out() {
        assert_eq!(
            layers(vec![
                model("read_file", json!({"path":"a.rs"})),
                model("read_file", json!({"path":"b.rs"})),
            ]),
            vec![vec![0, 1]]
        );
        assert_eq!(
            layers(vec![
                model("create_file", json!({"path":"a.rs"})),
                model("create_file", json!({"path":"b.rs"})),
            ]),
            vec![vec![0, 1]]
        );
    }

    #[test]
    fn same_path_read_write_and_parent_child_serialize() {
        assert_eq!(
            layers(vec![
                model("read_file", json!({"path":"src/lib.rs"})),
                model("write_file", json!({"path":"src/lib.rs"})),
            ]),
            vec![vec![0], vec![1]]
        );
        assert_eq!(
            layers(vec![
                model("write_file", json!({"path":"src"})),
                model("create_file", json!({"path":"src/domain/a.rs"})),
            ]),
            vec![vec![0], vec![1]]
        );
    }

    #[test]
    fn move_delete_and_directory_creation_dependencies_are_ordered() {
        assert_eq!(
            layers(vec![
                model("move_path", json!({"source":"a.rs","destination":"b.rs"})),
                model("write_file", json!({"path":"b.rs"})),
            ]),
            vec![vec![0], vec![1]]
        );
        assert_eq!(
            layers(vec![
                model("read_file", json!({"path":"src/domain/a.rs"})),
                model("delete_path", json!({"path":"src/domain"})),
            ]),
            vec![vec![0], vec![1]]
        );
        assert_eq!(
            layers(vec![
                model("create_file", json!({"path":"src/domain/a.rs"})),
                model("create_directory", json!({"path":"src/domain"})),
            ]),
            vec![vec![1], vec![0]],
            "parent directory creation must precede the child create even when submitted later"
        );
    }

    #[test]
    fn same_file_same_sha_coalesces_and_conflicts_are_rejected() {
        let items = vec![
            json!({"id":"first","tool":"apply_edits","arguments":{"path":"shared.txt","expected_sha256":"same","edits":[{"old_text":"same","new_text":"FIRST","start_line":1,"end_line":1}]}}),
            json!({"id":"last","tool":"apply_edits","arguments":{"path":"./shared.txt","expected_sha256":"same","edits":[{"old_text":"same","new_text":"LAST","start_line":3,"end_line":3}]}}),
        ];
        let (prepared, aliases, skipped) = coalesce_apply_edits("demo", &items).unwrap();
        assert_eq!(
            prepared[0]["arguments"]["edits"].as_array().unwrap().len(),
            2
        );
        assert_eq!(aliases.values().map(Vec::len).sum::<usize>(), 1);
        assert!(skipped.contains(&1));

        let different_sha = vec![
            json!({"tool":"apply_edits","arguments":{"path":"shared.txt","expected_sha256":"one","edits":[{"old_text":"a","new_text":"A","start_line":1,"end_line":1}]}}),
            json!({"tool":"apply_edits","arguments":{"path":"shared.txt","expected_sha256":"two","edits":[{"old_text":"b","new_text":"B","start_line":2,"end_line":2}]}}),
        ];
        assert!(coalesce_apply_edits("demo", &different_sha)
            .unwrap_err()
            .contains("different revisions"));

        let overlap = vec![
            json!({"tool":"apply_edits","arguments":{"path":"shared.txt","expected_sha256":"one","edits":[{"old_text":"a","new_text":"A","start_line":1,"end_line":3}]}}),
            json!({"tool":"apply_edits","arguments":{"path":"shared.txt","expected_sha256":"one","edits":[{"old_text":"b","new_text":"B","start_line":3,"end_line":4}]}}),
        ];
        assert!(coalesce_apply_edits("demo", &overlap)
            .unwrap_err()
            .contains("overlapping line ranges"));

        let ambiguous = vec![
            json!({"tool":"apply_edits","arguments":{"path":"shared.txt","expected_sha256":"one","edits":[{"old_text":"same","new_text":"A"}]}}),
            json!({"tool":"apply_edits","arguments":{"path":"shared.txt","expected_sha256":"one","edits":[{"old_text":"same","new_text":"B"}]}}),
        ];
        assert!(coalesce_apply_edits("demo", &ambiguous)
            .unwrap_err()
            .contains("ambiguous duplicate old_text"));
    }
}

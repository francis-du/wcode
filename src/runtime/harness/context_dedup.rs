use super::*;
use std::collections::BTreeMap;

#[derive(Debug)]
struct SourceMetadata {
    range: Option<(u64, u64)>,
}

pub(super) fn valid_sha256(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|sha| sha.len() == 64 && sha.as_bytes().iter().all(u8::is_ascii_hexdigit))
}

fn current_source_metadata(value: &Value) -> BTreeMap<(String, String), SourceMetadata> {
    let mut current = BTreeMap::new();
    for source in value["hot_source"].as_array().into_iter().flatten() {
        let complete_body = source["body"]["truncated"] == false
            && source["body"]["redacted"] == false
            && source["body"]["content"]
                .as_str()
                .is_some_and(|text| !text.is_empty());
        let current_file =
            valid_sha256(&source["sha256"])
                && value["files"].as_array().into_iter().flatten().any(|file| {
                    file["path"] == source["path"] && file["sha256"] == source["sha256"]
                });
        let Some((path, id)) = source["path"].as_str().zip(source["id"].as_str()) else {
            continue;
        };
        if !complete_body || !current_file {
            continue;
        }
        current.insert(
            (path.to_owned(), id.to_owned()),
            SourceMetadata {
                range: source["body"]["start_line"]
                    .as_u64()
                    .zip(source["body"]["end_line"].as_u64()),
            },
        );
    }
    current
}

fn item_key(item: &Value) -> Option<(String, String)> {
    let (path, id) = item["path"].as_str().zip(item["id"].as_str())?;
    Some((
        path.to_owned(),
        id.strip_prefix("symbol:").unwrap_or(id).to_owned(),
    ))
}

pub(super) fn deduplicate_source_backed_repo_map(value: &mut Value) -> bool {
    compact_duplicate_symbol_metadata(value, false)
}

pub(super) fn compact_duplicate_symbol_metadata(value: &mut Value, compact_metadata: bool) -> bool {
    let current = current_source_metadata(value);
    let mut changed = false;

    for pointer in ["/targets", "/repo_map/items"] {
        let repo_item = pointer == "/repo_map/items";
        for item in value
            .pointer_mut(pointer)
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
        {
            let metadata = item_key(item).and_then(|key| current.get(&key));
            let range_matches =
                metadata
                    .and_then(|source| source.range)
                    .is_some_and(|(start, end)| {
                        item["start_line"].as_u64() == Some(start)
                            && item["end_line"].as_u64() == Some(end)
                    });
            let Some(object) = item.as_object_mut() else {
                continue;
            };
            if metadata.is_some() && (compact_metadata || repo_item) {
                changed |= object.remove("signature").is_some();
            }
            if repo_item && range_matches {
                changed |= object.remove("start_line").is_some();
                changed |= object.remove("end_line").is_some();
            }
            if compact_metadata {
                // Scores explain ordering; they are not relationship evidence.
                changed |= object.remove("score").is_some();
                changed |= object.remove("degree").is_some();
            }
        }
    }

    if !compact_metadata {
        return changed;
    }

    // Under pressure, a complete source-backed relation-free row may yield.
    let targets = value["targets"].as_array().cloned().unwrap_or_default();
    if let Some(items) = value
        .pointer_mut("/repo_map/items")
        .and_then(Value::as_array_mut)
    {
        let before = items.len();
        items.retain(|item| {
            let Some(key) = item_key(item) else {
                return true;
            };
            let (path, id) = (&key.0, &key.1);
            !(current.contains_key(&key)
                && targets.iter().any(|target| {
                    target["path"].as_str() == Some(path) && target["id"].as_str() == Some(id)
                })
                && item["relationships"].as_array().is_some_and(Vec::is_empty))
        });
        changed |= before != items.len();
    }

    for file in value
        .get_mut("files")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        if let Some(object) = file.as_object_mut() {
            changed |= object.remove("size").is_some();
            changed |= object.remove("reasons").is_some();
        }
    }
    changed
}

//! Bounded, source-derived experiment proposals. Nothing here executes a mutant
//! or decides correctness; an independent contract oracle and isolated runner do.
use crate::code_index::SyntaxSearchRequest;
use crate::harness::{ChangeReviewReport, ToolHarness};
use crate::workspace::Workspace;
use rayon::prelude::*;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

const MAX_FILES: usize = 6;
const MAX_NODES_PER_FILE: usize = 32;
const MAX_CANDIDATES: usize = 3;
const MAX_SOURCE_BYTES: u64 = 256 * 1024;

#[derive(Clone, Debug, Serialize)]
pub struct BoundaryProbe {
    pub binding: String,
    // Strings preserve integer precision across JSON/JavaScript clients.
    pub values: Vec<String>,
    pub domain: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct CounterexampleCandidate {
    pub id: String,
    pub kind: &'static str,
    pub status: &'static str,
    pub path: String,
    pub sha256: String,
    pub language: String,
    pub node_kind: String,
    pub start_line: usize,
    pub end_line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
    pub original: String,
    pub replacement: String,
    pub priority: &'static str,
    pub cost_rank: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boundary_probe: Option<BoundaryProbe>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CandidateScanIssue {
    pub path: String,
    pub reason: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct CounterexampleSearch {
    pub provider: &'static str,
    pub precision: &'static str,
    pub status: &'static str,
    pub scope: &'static str,
    pub ranking: &'static str,
    pub executed: bool,
    pub oracle_required: bool,
    pub files_considered: usize,
    pub files_scanned: usize,
    pub files_failed: usize,
    pub files_skipped: usize,
    pub files_omitted: usize,
    pub candidates_considered: usize,
    pub candidates: Vec<CounterexampleCandidate>,
    pub issues: Vec<CandidateScanIssue>,
    pub truncated: bool,
    pub limitations: Vec<&'static str>,
}

pub(super) fn build(
    harness: &ToolHarness,
    workspace: &Workspace,
    review: &ChangeReviewReport,
) -> CounterexampleSearch {
    let security: BTreeSet<_> = review
        .findings
        .iter()
        .filter(|finding| finding.code == "security-sensitive-change")
        .flat_map(|finding| finding.paths.iter().map(String::as_str))
        .collect();
    let mut eligible = BTreeMap::new();
    if !review.clean && review.source_changed {
        for file in &review.files {
            if file.category == "source" && !file.binary && file.status != "deleted" {
                eligible.insert(file.path.clone(), security.contains(file.path.as_str()));
            }
        }
    }
    let files_considered = eligible.len();
    let mut files = eligible.into_iter().collect::<Vec<_>>();
    files.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    files.truncate(MAX_FILES);
    // Only independent known files fan out. The canonical AST/Workspace paths
    // retain authorization, redaction, source limits and index freshness checks.
    let inspected: Vec<_> = files
        .par_iter()
        .map(|(path, sensitive)| {
            (
                path.clone(),
                inspect(harness, workspace, &review.workspace, path, *sensitive),
            )
        })
        .collect();
    let mut result = CounterexampleSearch {
        provider: "tree-sitter-counterexample-candidates/v1",
        precision: "syntax",
        status: "no_candidates",
        scope: "changed-source-files-not-diff-hunks",
        ranking: "security-first-then-heuristic-cost-then-path-and-byte",
        executed: false,
        oracle_required: true,
        files_considered,
        files_scanned: 0,
        files_failed: 0,
        files_skipped: 0,
        files_omitted: files_considered.saturating_sub(files.len()),
        candidates_considered: 0,
        candidates: vec![],
        issues: vec![],
        truncated: review.truncated || files_considered > files.len(),
        limitations: vec![
            "Proposals only: no command, source edit, test, stage, or Evidence result was executed.",
            "Confirm the active contract and input domain; syntax cannot prove parameter types or expected outputs.",
            "Run a nonempty passing baseline and typecheck mutations in an isolated copy, never the shared worktree.",
            "A survivor needs equivalence and reachability review; build errors, timeouts and absent tests are not mutation kills.",
            "One killed mutant or no candidate does not close an entire QA claim or prove correctness.",
            "Cost ranks are static heuristics, not measured latency or defect probabilities.",
        ],
    };
    for (path, inspection) in inspected {
        match inspection {
            Ok((candidates, partial)) => {
                result.files_scanned += 1;
                result.truncated |= partial;
                result.candidates.extend(candidates);
            }
            Err(reason) => {
                if matches!(reason, "source-over-byte-limit" | "unsupported-source") {
                    result.files_skipped += 1;
                } else {
                    result.files_failed += 1;
                }
                result.issues.push(CandidateScanIssue { path, reason });
                result.truncated = true;
            }
        }
    }
    result.candidates.sort_by(|left, right| {
        (
            left.priority != "security",
            left.cost_rank,
            &left.path,
            left.start_byte,
            left.end_byte,
        )
            .cmp(&(
                right.priority != "security",
                right.cost_rank,
                &right.path,
                right.start_byte,
                right.end_byte,
            ))
    });
    let mut seen = BTreeSet::new();
    result
        .candidates
        .retain(|candidate| seen.insert(candidate.id.clone()));
    result.candidates_considered = result.candidates.len();
    result.truncated |= result.candidates.len() > MAX_CANDIDATES;
    result.candidates.truncate(MAX_CANDIDATES);
    result.status = if result.truncated {
        "partial"
    } else if result.candidates.is_empty() {
        "no_candidates"
    } else {
        "proposed"
    };
    result
}

fn inspect(
    harness: &ToolHarness,
    workspace: &Workspace,
    workspace_id: &str,
    path: &str,
    sensitive: bool,
) -> Result<(Vec<CounterexampleCandidate>, bool), &'static str> {
    let stamp = workspace
        .source_metadata_stamp(path)
        .map_err(|_| "source-unavailable")?;
    if stamp.len() > MAX_SOURCE_BYTES {
        return Err("source-over-byte-limit");
    }
    if crate::semantic_provider::language_for_path(path).is_none() {
        return Err("unsupported-source");
    }
    let search = harness
        .search_syntax(
            workspace_id,
            workspace,
            SyntaxSearchRequest {
                path: path.to_owned(),
                node_kinds: [
                    "boolean_literal",
                    "true",
                    "false",
                    "binary_expression",
                    "comparison_operator",
                ]
                .into_iter()
                .map(str::to_owned)
                .collect(),
                text_regex: None,
                include_comments: false,
                bug_patterns: Vec::new(),
                max_files: 1,
                max_results: MAX_NODES_PER_FILE,
            },
        )
        .map_err(|_| "syntax-search-unavailable")?;
    if search["files_considered"].as_u64() == Some(0) {
        return Err("unsupported-source");
    }
    if search["files_failed"].as_u64() != Some(0) {
        return Err("syntax-source-failed");
    }
    let matches = search["matches"]
        .as_array()
        .ok_or("invalid-syntax-result")?;
    let source = workspace
        .load_source(path)
        .map_err(|_| "source-unavailable")?;
    if source.content.len() as u64 > MAX_SOURCE_BYTES {
        return Err("source-over-byte-limit");
    }
    if matches.is_empty() {
        // An empty node list does not prove a successful parse. Reuse the warm
        // outline's file-level status instead of inferring success from no hits.
        let outline = harness
            .code_index
            .file_outline(workspace_id, workspace, path, 1)
            .map_err(|_| "syntax-source-failed")?;
        if outline["parse_errors"].as_bool() != Some(false) {
            return Err("source-parse-errors");
        }
        if outline["sha256"].as_str() != Some(source.sha256.as_str()) {
            return Err("source-revision-changed");
        }
    }
    // Do not combine the AST from one revision with raw bytes from another.
    if matches.iter().any(|item| {
        item["sha256"].as_str() != Some(source.sha256.as_str())
            || item["path"].as_str() != Some(path)
    }) {
        return Err("source-revision-changed");
    }
    if matches
        .iter()
        .any(|item| item["parse_errors"].as_bool() != Some(false))
    {
        return Err("source-parse-errors");
    }
    let mut candidates = Vec::new();
    let mut partial = search["truncated"].as_bool() != Some(false);
    for item in matches {
        if item["redacted"].as_bool() != Some(false)
            || item["text_truncated"].as_bool() != Some(false)
        {
            partial = true;
            continue;
        }
        if let Some(candidate) = from_match(path, &source.sha256, &source.content, item, sensitive)
        {
            candidates.push(candidate);
        }
    }
    Ok((candidates, partial))
}

fn byte_at(source: &str, line: usize, column: usize) -> Option<usize> {
    if line == 0 || column == 0 {
        return None;
    }
    let mut offset = 0usize;
    for (index, text) in source.split_inclusive('\n').enumerate() {
        if index + 1 == line {
            let delta = column - 1;
            if delta > text.trim_end_matches('\n').len() {
                return None;
            }
            let absolute = offset.checked_add(delta)?;
            return source.is_char_boundary(absolute).then_some(absolute);
        }
        offset += text.len();
    }
    (column == 1 && source.ends_with('\n') && line == source.lines().count() + 1)
        .then_some(source.len())
}

fn from_match(
    path: &str,
    sha: &str,
    source: &str,
    item: &serde_json::Value,
    sensitive: bool,
) -> Option<CounterexampleCandidate> {
    if item["path"].as_str() != Some(path)
        || item["sha256"].as_str() != Some(sha)
        || item["redacted"].as_bool() != Some(false)
        || item["text_truncated"].as_bool() != Some(false)
        || item["parse_errors"].as_bool() != Some(false)
        || !matches!(
            item["node_kind"].as_str()?,
            "boolean_literal" | "true" | "false" | "binary_expression" | "comparison_operator"
        )
    {
        return None;
    }
    let range = &item["range"];
    if range["end_exclusive"].as_bool() != Some(true) {
        return None;
    }
    let number = |key: &str| usize::try_from(range[key].as_u64()?).ok();
    let start_line = number("start_line")?;
    let end_line = number("end_line")?;
    let start = byte_at(source, start_line, number("start_column")?)?;
    let end = byte_at(source, end_line, number("end_column")?)?;
    let original = source.get(start..end)?;
    if original != item["text"].as_str()? {
        return None;
    }
    let (replacement, boundary_probe, cost_rank) = proposal(original)?;
    let mut hash = Sha256::new();
    for value in [
        path,
        sha,
        original,
        &replacement,
        &start.to_string(),
        &end.to_string(),
    ] {
        hash.update((value.len() as u64).to_le_bytes());
        hash.update(value.as_bytes());
    }
    Some(CounterexampleCandidate {
        id: format!("ce:{:x}", hash.finalize()),
        kind: "mutation",
        status: "proposed-not-typechecked",
        path: path.to_owned(),
        sha256: sha.to_owned(),
        language: item["language"].as_str()?.to_owned(),
        node_kind: item["node_kind"].as_str()?.to_owned(),
        start_line,
        end_line,
        start_byte: start,
        end_byte: end,
        original: original.to_owned(),
        replacement,
        priority: if sensitive { "security" } else { "normal" },
        cost_rank,
        boundary_probe,
    })
}

fn proposal(original: &str) -> Option<(String, Option<BoundaryProbe>, u8)> {
    let flipped = match original {
        "true" => Some("false"),
        "false" => Some("true"),
        "True" => Some("False"),
        "False" => Some("True"),
        _ => None,
    };
    if let Some(flipped) = flipped {
        return Some((flipped.to_owned(), None, 2));
    }
    // AST establishes a real node; this deliberately narrow shape refuses
    // strings, comments, side effects, compound operands and chained comparisons.
    static COMPARISON: OnceLock<regex::Regex> = OnceLock::new();
    let pattern = COMPARISON.get_or_init(|| regex::Regex::new(
        r"\A(?P<left>[A-Za-z_][A-Za-z0-9_]*|-?[0-9]{1,19})\s*(?P<op>===|!==|==|!=|<=|>=|<|>)\s*(?P<right>[A-Za-z_][A-Za-z0-9_]*|-?[0-9]{1,19})\z"
    ).expect("fixed comparison expression is valid"));
    let captures = pattern.captures(original)?;
    let operator = captures.name("op")?;
    let replacement_op = match operator.as_str() {
        "==" => "!=",
        "!=" => "==",
        "===" => "!==",
        "!==" => "===",
        "<" => "<=",
        "<=" => "<",
        ">" => ">=",
        ">=" => ">",
        _ => return None,
    };
    let mut replacement = original.to_owned();
    replacement.replace_range(operator.range(), replacement_op);
    let left = captures.name("left")?.as_str();
    let right = captures.name("right")?.as_str();
    let binding = [(left, right), (right, left)]
        .into_iter()
        .find_map(|(name, number)| {
            name.as_bytes()
                .first()
                .filter(|c| c.is_ascii_alphabetic() || **c == b'_')?;
            Some((name, number.parse::<i64>().ok()?))
        });
    let boundary = binding.map(|(binding, value)| BoundaryProbe {
        binding: binding.to_owned(),
        values: [value.checked_sub(1), Some(value), value.checked_add(1)]
            .into_iter()
            .flatten()
            .map(|n| n.to_string())
            .collect(),
        domain: "requires-type-and-input-binding-validation",
    });
    Some((replacement, boundary, 1))
}

#[cfg(test)]
#[path = "../../../../tests/unit/runtime/harness/counterexamples.rs"]
mod tests;

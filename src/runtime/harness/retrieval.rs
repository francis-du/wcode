use crate::graph::{NodeKind, SoftwareGraphSnapshot};
use crate::intelligence::SoftwareContext;
use crate::workspace::{SearchMode, SearchReport, SearchRequest, Workspace};
use anyhow::Result;
use serde_json::{json, Value};
use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::{HashSet, VecDeque};

#[derive(Clone, Debug)]
pub(super) struct RepoMapCandidate {
    pub(super) id: String,
    pub(super) path: String,
    pub(super) name: String,
    pub(super) qualified_name: String,
    pub(super) kind: String,
    pub(super) relevance: f64,
    pub(super) direct: bool,
    pub(super) exact_direct: bool,
    pub(super) design_path: bool,
    pub(super) query_hits: usize,
    pub(super) experience_weight: u16,
    pub(super) degree: usize,
    pub(super) rank: f64,
}

pub(super) fn compare_repo_candidates(
    left: &RepoMapCandidate,
    right: &RepoMapCandidate,
) -> Ordering {
    // Graph popularity cannot evict the symbol explicitly named by the task.
    right
        .exact_direct
        .cmp(&left.exact_direct)
        .then_with(|| right.rank.total_cmp(&left.rank))
        .then_with(|| right.direct.cmp(&left.direct))
        .then_with(|| left.qualified_name.cmp(&right.qualified_name))
        .then_with(|| left.path.cmp(&right.path))
        .then_with(|| left.id.cmp(&right.id))
}

pub(super) fn select_repo_candidates(candidates: &mut Vec<RepoMapCandidate>, limit: usize) {
    if limit == 0 {
        candidates.clear();
        return;
    }
    // Partition all candidates in linear time; sort only the returned prefix.
    // The canonical ID tie-break preserves deterministic membership and order.
    if candidates.len() > limit {
        candidates.select_nth_unstable_by(limit, compare_repo_candidates);
        candidates.truncate(limit);
    }
    candidates.sort_unstable_by(compare_repo_candidates);
}

pub(super) fn retain_repo_candidates_with_task_evidence(
    candidates: &mut Vec<RepoMapCandidate>,
    neighbors: &[Vec<usize>],
    intent: RepoMapIntent,
) {
    // Keep only bounded graph reachability from actual task evidence. Walking
    // the full connected component turns one relevant symbol into arbitrary
    // transitive noise and lets graph popularity consume a tight context.
    // Relationship-oriented tasks need one extra hop for wrappers/adapters;
    // ordinary context/test discovery stays on the immediate neighborhood.
    let max_hops = match intent {
        RepoMapIntent::TraceToCode
        | RepoMapIntent::CommentToContext
        | RepoMapIntent::FailureTraceToCode
        | RepoMapIntent::EditToRipple => 2,
        RepoMapIntent::CodeToTest | RepoMapIntent::Context => 1,
    };
    let has_exact_direct = candidates.iter().any(|candidate| candidate.exact_direct);
    let mut distance = vec![usize::MAX; candidates.len()];
    let mut pending = VecDeque::new();
    for (index, candidate) in candidates.iter().enumerate() {
        let direct_anchor = candidate.exact_direct || (!has_exact_direct && candidate.direct);
        let routing_anchor =
            intent == RepoMapIntent::CodeToTest && test_path(&candidate.path, &candidate.kind);
        // Once the task has an exact symbol anchor, fuzzy/query-term matches
        // remain ranking evidence only. Promoting them to fresh zero-hop seeds
        // would reset graph distance and admit transitive noise beyond the
        // bounded relationship horizon. Without an exact target, query hits
        // still provide the exploratory anchors natural-language tasks need.
        let query_anchor = !has_exact_direct && candidate.query_hits > 0;
        if direct_anchor
            || routing_anchor
            || query_anchor
            || candidate.design_path
            || candidate.experience_weight > 0
        {
            distance[index] = 0;
            pending.push_back(index);
        }
    }
    if pending.is_empty() {
        // With no task anchor, retain the bounded exploratory repository map.
        return;
    }
    while let Some(index) = pending.pop_front() {
        let next_distance = distance[index].saturating_add(1);
        if next_distance > max_hops {
            continue;
        }
        for &neighbor in &neighbors[index] {
            if neighbor < distance.len() && next_distance < distance[neighbor] {
                distance[neighbor] = next_distance;
                pending.push_back(neighbor);
            }
        }
    }
    let mut index = 0;
    candidates.retain(|_| {
        let keep = distance[index] <= max_hops;
        index += 1;
        keep
    });
}

pub(super) fn retain_returnable_repo_candidates(candidates: &mut Vec<RepoMapCandidate>) {
    candidates.retain(|candidate| {
        candidate.kind != "module"
            || candidate.direct
            || candidate.query_hits > 0
            || candidate.design_path
            || candidate.experience_weight > 0
    });
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RepoMapIntent {
    TraceToCode,
    CodeToTest,
    CommentToContext,
    FailureTraceToCode,
    EditToRipple,
    Context,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct RepoMapRouting {
    pub(super) intent: RepoMapIntent,
    pub(super) specialized: bool,
    pub(super) reason: &'static str,
}

impl RepoMapRouting {
    pub(super) fn name(self) -> &'static str {
        match self.intent {
            RepoMapIntent::TraceToCode => "trace_to_code",
            RepoMapIntent::CodeToTest => "code_to_test",
            RepoMapIntent::CommentToContext => "comment_to_context",
            RepoMapIntent::FailureTraceToCode => "failure_trace_to_code",
            RepoMapIntent::EditToRipple => "edit_to_ripple",
            RepoMapIntent::Context => "balanced_context",
        }
    }
}

// Keep code identifiers available to retrieval, but do not interpret their
// embedded words (for example `get_latest_snapshot`) as natural-language intent.
fn intent_query(query: &str) -> String {
    let separator = |ch: char| !ch.is_ascii_alphanumeric() && !matches!(ch, '_' | ':' | '.');
    let mut result = String::with_capacity(query.len());
    for chunk in query.split_inclusive(separator) {
        let word = chunk.trim_end_matches(separator);
        let identifier = word.trim_matches([':', '.']);
        let code_shaped = identifier.contains('_')
            || identifier.contains("::")
            || identifier.contains('.')
            || identifier
                .as_bytes()
                .windows(2)
                .any(|pair| pair[0].is_ascii_lowercase() && pair[1].is_ascii_uppercase());
        if code_shaped {
            result.push(' ');
        } else {
            result.push_str(&word.to_ascii_lowercase());
        }
        result.push_str(&chunk[word.len()..]);
    }
    result
}

pub(super) fn classify_repo_map_intent(query: &str) -> RepoMapRouting {
    let evidence = query.to_ascii_lowercase();
    let query = intent_query(query);
    let trace = contains_any(
        &query,
        &[
            "requirement",
            "acceptance",
            "traceability",
            "implemented by",
            "req-",
            "ac-",
            "需求",
            "验收",
            "追踪",
            "实现在哪",
        ],
    );
    let tests = contains_any(
        &query,
        &[
            "test",
            "tests",
            "regression",
            "verification",
            "verify",
            "coverage",
            "测试",
            "回归",
            "验证",
            "覆盖",
        ],
    );
    let comment_context = comment_context_signal(&evidence);
    let failure_trace = failure_trace_signal(&evidence);
    let ripple = contains_any(
        &query,
        &[
            "impact",
            "caller",
            "callers",
            "callee",
            "callees",
            "references",
            "rename",
            "ripple",
            "affected",
            "影响范围",
            "调用方",
            "被调用",
            "引用",
            "重命名",
            "波及",
        ],
    );
    if failure_trace && !comment_context {
        return RepoMapRouting {
            intent: RepoMapIntent::FailureTraceToCode,
            specialized: true,
            reason: "reproduced_failure_trace_signal",
        };
    }
    match (trace, tests, comment_context, ripple) {
        (true, false, false, false) => RepoMapRouting {
            intent: RepoMapIntent::TraceToCode,
            specialized: true,
            reason: "requirement_or_traceability_signal",
        },
        (false, true, false, false) => RepoMapRouting {
            intent: RepoMapIntent::CodeToTest,
            specialized: true,
            reason: "test_or_verification_signal",
        },
        (false, false, true, false) => RepoMapRouting {
            intent: RepoMapIntent::CommentToContext,
            specialized: true,
            reason: "review_comment_needs_additional_context",
        },
        (false, false, false, true) => RepoMapRouting {
            intent: RepoMapIntent::EditToRipple,
            specialized: true,
            reason: "impact_or_relationship_signal",
        },
        (false, false, false, false) => RepoMapRouting {
            intent: RepoMapIntent::Context,
            specialized: false,
            reason: "no_specific_retrieval_signal",
        },
        _ => RepoMapRouting {
            intent: RepoMapIntent::Context,
            specialized: false,
            reason: "ambiguous_retrieval_signals",
        },
    }
}

pub(super) fn explicit_query_paths(context: &SoftwareContext, query: &str) -> Vec<String> {
    let literals = crate::intelligence::code_query_literals(query)
        .into_iter()
        .collect::<HashSet<_>>();
    if literals.is_empty() {
        return Vec::new();
    }
    context
        .symbols
        .iter()
        .filter(|symbol| {
            let name = symbol["name"]
                .as_str()
                .unwrap_or_default()
                .to_ascii_lowercase();
            let qualified = symbol["qualified_name"]
                .as_str()
                .unwrap_or_default()
                .to_ascii_lowercase();
            literals.contains(&name) || literals.contains(&qualified)
        })
        .filter_map(|symbol| symbol["path"].as_str().map(str::to_owned))
        .collect()
}

pub(super) fn query_requests_semantic_rename(query: &str) -> bool {
    let query = intent_query(query);
    (query.contains("rename")
        && (query.contains(" to ")
            || query.contains(" as ")
            || query.contains("->")
            || query.contains('→')))
        || (query.contains("重命名")
            && (query.contains('为') || query.contains('成') || query.contains('到')))
}

pub(super) fn query_requests_quick_fix(query: &str) -> bool {
    let query = intent_query(query);
    [
        "quick fix",
        "code action",
        "fix diagnostic",
        "fix this diagnostic",
        "fix compiler diagnostic",
        "快速修复",
        "代码操作",
        "修复诊断",
    ]
    .iter()
    .any(|needle| query.contains(needle))
}

pub(super) fn query_requests_organize_imports(query: &str) -> bool {
    let query = intent_query(query);
    [
        "organize imports",
        "organise imports",
        "sort imports",
        "optimize imports",
        "optimise imports",
        "整理导入",
        "整理 imports",
        "排序导入",
        "优化导入",
    ]
    .iter()
    .any(|needle| query.contains(needle))
}

pub(super) fn query_needs_semantic_relationships(query: &str) -> bool {
    if query_requests_organize_imports(query) || query_requests_quick_fix(query) {
        return true;
    }
    let query = intent_query(query);
    [
        "reference",
        "references",
        "caller",
        "callers",
        "callee",
        "callees",
        "implementation",
        "implementations",
        "implementor",
        "usages",
        "rename",
        "call site",
        "call graph",
        "call hierarchy",
        "cross-file",
        "cross file",
        "impact",
        "引用",
        "调用方",
        "被调用",
        "实现",
        "重命名",
        "调用点",
        "调用链",
        "调用关系",
        "跨文件",
        "影响范围",
    ]
    .iter()
    .any(|needle| query.contains(needle))
}

pub(super) fn augment_relationship_graph<'a>(
    harness: &super::ToolHarness,
    workspace_id: &str,
    workspace: &Workspace,
    query: &str,
    context: &SoftwareContext,
    base: &'a SoftwareGraphSnapshot,
) -> Result<Cow<'a, SoftwareGraphSnapshot>> {
    let routing = classify_repo_map_intent(query);
    let relationship_requested = query_needs_semantic_relationships(query) || routing.specialized;
    // Code-to-test relationships commonly cross from a localized source scope
    // into top-level tests/. Always run the bounded exact-text supplement for
    // that intent, even when the base graph itself looks complete. Other
    // relationship intents only need the supplement when the base graph was
    // narrowed or its source scan was truncated.
    let targeted_scan = relationship_requested
        && (routing.intent == RepoMapIntent::CodeToTest || base.path != "." || base.scan_truncated);
    let priority_recovery = base.truncated;
    if !targeted_scan && !priority_recovery {
        return Ok(Cow::Borrowed(base));
    }

    let literals = crate::intelligence::code_query_literals(query);
    let priority_symbols = context
        .symbols
        .iter()
        .filter(|symbol| {
            let name = symbol
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let qualified = symbol
                .get("qualified_name")
                .and_then(Value::as_str)
                .unwrap_or(name);
            literals.is_empty()
                || literals.iter().any(|literal| {
                    name.eq_ignore_ascii_case(literal) || qualified.eq_ignore_ascii_case(literal)
                })
        })
        .take(4)
        .collect::<Vec<_>>();
    let priority_symbol_ids = priority_symbols
        .iter()
        .filter_map(|symbol| symbol.get("id").and_then(Value::as_str))
        .map(str::to_owned)
        .collect::<HashSet<_>>();
    let priority_symbol_names = priority_symbols
        .iter()
        .filter_map(|symbol| symbol.get("name").and_then(Value::as_str))
        .map(str::to_owned)
        .collect::<HashSet<_>>();
    let mut supplemental_paths = Vec::new();
    let mut seen_paths = HashSet::new();
    for path in context
        .symbols
        .iter()
        .filter_map(|symbol| symbol.get("path").and_then(Value::as_str))
    {
        if seen_paths.insert(path.to_owned()) {
            supplemental_paths.push(path.to_owned());
        }
    }

    let mut supplemental_scan_truncated = false;
    if targeted_scan {
        let mut relation_queries = context
            .symbols
            .iter()
            .filter_map(|symbol| {
                let name = symbol.get("name").and_then(Value::as_str)?;
                let qualified = symbol
                    .get("qualified_name")
                    .and_then(Value::as_str)
                    .unwrap_or(name);
                (literals.is_empty()
                    || literals.iter().any(|literal| {
                        name.eq_ignore_ascii_case(literal)
                            || qualified.eq_ignore_ascii_case(literal)
                    }))
                .then(|| name.to_owned())
            })
            .collect::<Vec<_>>();
        if relation_queries.is_empty() {
            relation_queries.extend(
                context
                    .symbols
                    .iter()
                    .filter_map(|symbol| symbol.get("name").and_then(Value::as_str))
                    .take(2)
                    .map(str::to_owned),
            );
        }
        let mut seen_queries = HashSet::new();
        relation_queries.retain(|name| seen_queries.insert(name.to_ascii_lowercase()));
        relation_queries.truncate(4);
        if !relation_queries.is_empty() {
            let request = SearchRequest {
                queries: relation_queries,
                path: ".".to_owned(),
                mode: SearchMode::Exact,
                context_lines: 0,
                include_comments: true,
                max_results: super::REPO_MAP_MAX_FILES,
                offset: 0,
                output_mode: "files_with_matches".to_owned(),
            };
            let report: SearchReport = workspace.search_report(&request)?;
            let report = report.into_value(workspace_id, &request, false);
            supplemental_scan_truncated = report["truncated"].as_bool().unwrap_or(false);
            for path in report["files"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|file| file.get("path").and_then(Value::as_str))
            {
                if seen_paths.insert(path.to_owned()) {
                    supplemental_paths.push(path.to_owned());
                }
            }
        }
    }

    supplemental_paths.truncate(super::REPO_MAP_MAX_FILES.saturating_add(4));
    if supplemental_paths.is_empty() {
        return Ok(Cow::Borrowed(base));
    }

    let mut supplemental = harness.code_index.software_graph_from_paths(
        workspace,
        supplemental_paths,
        supplemental_scan_truncated,
        super::REPO_MAP_MAX_SYMBOLS,
        &priority_symbol_ids,
        &priority_symbol_names,
    )?;
    supplemental.workspace = workspace_id.to_owned();
    supplemental.path = ".".to_owned();
    merge_relationship_graph(base, supplemental).map(Cow::Owned)
}

pub(super) fn merge_relationship_graph(
    base: &SoftwareGraphSnapshot,
    supplemental: SoftwareGraphSnapshot,
) -> Result<SoftwareGraphSnapshot> {
    let mut graph = base.clone();
    for (id, node) in supplemental.graph.nodes {
        match graph.graph.nodes.entry(id) {
            std::collections::btree_map::Entry::Occupied(existing) => {
                if existing.get().provenance != node.provenance {
                    anyhow::bail!(
                        "source changed during repository graph augmentation; retry the request"
                    );
                }
            }
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(node);
            }
        }
    }
    // Borrow canonical keys during the merge, then append only genuinely new
    // edges. Identity includes full provenance, not just the endpoints/kind.
    let mut seen = graph
        .graph
        .edges
        .iter()
        .map(|edge| {
            (
                edge.from.as_str(),
                edge.to.as_str(),
                edge.kind,
                edge.provenance.provider.as_str(),
                edge.provenance.precision,
                edge.provenance.revision.as_str(),
            )
        })
        .collect::<HashSet<_>>();
    let additions = supplemental
        .graph
        .edges
        .iter()
        .filter(|edge| {
            seen.insert((
                edge.from.as_str(),
                edge.to.as_str(),
                edge.kind,
                edge.provenance.provider.as_str(),
                edge.provenance.precision,
                edge.provenance.revision.as_str(),
            ))
        })
        .cloned()
        .collect::<Vec<_>>();
    drop(seen);
    graph.graph.edges.extend(additions);
    graph.files_indexed = graph
        .graph
        .nodes
        .values()
        .filter(|node| node.kind == NodeKind::File)
        .count();
    graph.node_count = graph.graph.nodes.len();
    graph.edge_count = graph.graph.edges.len();
    graph.scan_truncated |= supplemental.scan_truncated;
    graph.truncated |= supplemental.truncated;
    graph.graph.validate()?;
    Ok(graph)
}

pub(super) fn routing_value(routing: RepoMapRouting) -> Value {
    json!({
        "intent": routing.name(),
        "specialized": routing.specialized,
        "abstained_from_specialization": !routing.specialized,
        "reason": routing.reason,
        "provider": "query-intent-rules-v1",
        "precision": "heuristic",
    })
}

pub(super) fn design_boost(intent: RepoMapIntent, design_path: bool) -> f64 {
    if !design_path {
        return 0.0;
    }
    match intent {
        RepoMapIntent::TraceToCode => 48.0,
        RepoMapIntent::CodeToTest => 32.0,
        RepoMapIntent::CommentToContext => 38.0,
        RepoMapIntent::FailureTraceToCode => 28.0,
        RepoMapIntent::EditToRipple => 30.0,
        RepoMapIntent::Context => 35.0,
    }
}

pub(super) fn direct_seed_boost(
    intent: RepoMapIntent,
    design_path: bool,
    test_path: bool,
    experience_weight: u16,
) -> f64 {
    match intent {
        RepoMapIntent::TraceToCode if design_path => 24.0,
        RepoMapIntent::CodeToTest if test_path => 30.0,
        RepoMapIntent::EditToRipple => experience_boost(intent, experience_weight),
        _ => 0.0,
    }
}

pub(super) fn test_boost(intent: RepoMapIntent, test_path: bool) -> f64 {
    if !test_path {
        return 0.0;
    }
    match intent {
        RepoMapIntent::CodeToTest => 30.0,
        RepoMapIntent::CommentToContext => 8.0,
        _ => 0.0,
    }
}

pub(super) fn experience_boost(intent: RepoMapIntent, weight: u16) -> f64 {
    let cap = match intent {
        RepoMapIntent::EditToRipple => 16.0,
        RepoMapIntent::FailureTraceToCode => 12.0,
        RepoMapIntent::CommentToContext | RepoMapIntent::Context => 10.0,
        RepoMapIntent::CodeToTest => 8.0,
        RepoMapIntent::TraceToCode => 6.0,
    };
    (weight as f64 * (cap / 1_000.0)).min(cap)
}

pub(super) fn relationship_boost(intent: RepoMapIntent, relations: &[Value]) -> f64 {
    let matches = relations.iter().any(|relation| {
        let relation = relation
            .get("relation")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match intent {
            RepoMapIntent::TraceToCode => matches!(
                relation,
                "implemented_by_direct"
                    | "implements_direct"
                    | "dependency_of_direct"
                    | "depends_on_direct"
            ),
            RepoMapIntent::CodeToTest => false,
            RepoMapIntent::CommentToContext
            | RepoMapIntent::FailureTraceToCode
            | RepoMapIntent::EditToRipple => relation != "related_to_direct",
            RepoMapIntent::Context => false,
        }
    });
    if !matches {
        return 0.0;
    }
    match intent {
        RepoMapIntent::TraceToCode => 14.0,
        RepoMapIntent::CommentToContext => 16.0,
        RepoMapIntent::FailureTraceToCode => 20.0,
        RepoMapIntent::EditToRipple => 18.0,
        _ => 0.0,
    }
}

pub(super) fn exact_query_match(name: &str, qualified_name: &str, query_tokens: &[String]) -> bool {
    let name = name.to_ascii_lowercase();
    let qualified_name = qualified_name.to_ascii_lowercase();
    query_tokens
        .iter()
        .any(|token| token == &name || token == &qualified_name)
}

pub(super) fn test_path(path: &str, kind: &str) -> bool {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    kind.eq_ignore_ascii_case("test")
        || normalized.starts_with("tests/")
        || normalized.starts_with("test/")
        || normalized.starts_with("__tests__/")
        || normalized.contains("/tests/")
        || normalized.contains("/test/")
        || normalized.contains("/__tests__/")
        || normalized.ends_with("_test.go")
        || normalized.ends_with("_test.rs")
        || normalized.ends_with(".test.js")
        || normalized.ends_with(".test.ts")
        || normalized.ends_with(".spec.js")
        || normalized.ends_with(".spec.ts")
        || normalized
            .rsplit('/')
            .next()
            .is_some_and(|name| name.starts_with("test_"))
}

pub(super) fn query_requests_comment_context(query: &str) -> bool {
    comment_context_signal(&query.to_ascii_lowercase())
}

pub(super) fn query_contains_failure_trace(query: &str) -> bool {
    failure_trace_signal(&query.to_ascii_lowercase())
}

fn failure_trace_signal(query: &str) -> bool {
    contains_any(
        query,
        &[
            "error[",
            "panicked at",
            "stack backtrace",
            "backtrace:",
            "traceback (most recent call last)",
            "assertion failed",
            "assertionerror",
            "exception:",
            "caused by:",
            "segmentation fault",
            "segfault",
            "堆栈",
            "回溯",
            "断言失败",
            "异常:",
            "异常：",
            "崩溃",
        ],
    )
}

fn comment_context_signal(query: &str) -> bool {
    contains_any(
        query,
        &[
            "review comment",
            "code review",
            "reviewer",
            "nit:",
            "should this",
            "should we",
            "consistent with",
            "consistently with",
            "same as",
            "same way",
            "similar implementation",
            "existing implementation",
            "elsewhere",
            "审查意见",
            "评审意见",
            "这里是否",
            "是否应该",
            "保持一致",
            "一致处理",
            "类似实现",
            "现有实现",
            "其他地方",
        ],
    )
}

fn contains_any(query: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| query.contains(needle))
}

impl super::ToolHarness {
    pub fn search_syntax(
        &self,
        workspace_id: &str,
        workspace: &crate::workspace::Workspace,
        request: crate::code_index::SyntaxSearchRequest,
    ) -> anyhow::Result<Value> {
        self.code_index
            .search_ast_nodes(workspace_id, workspace, &request)
    }
}

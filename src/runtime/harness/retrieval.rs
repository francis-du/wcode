use serde_json::{json, Value};

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

pub(super) fn classify_repo_map_intent(query: &str) -> RepoMapRouting {
    let query = query.to_ascii_lowercase();
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
    let comment_context = comment_context_signal(&query);
    let failure_trace = failure_trace_signal(&query);
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

use super::*;

pub(super) fn compact_go_code(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_whitespace()).collect()
}

pub(super) fn redacted_guard_excerpt(text: &str) -> String {
    let excerpt = text.chars().take(240).collect::<String>();
    redact_sensitive_text(excerpt.trim()).0
}

pub(super) fn early_exit_guard(prefix: &str, condition: &str) -> bool {
    let Some(position) = prefix.rfind(condition) else {
        return false;
    };
    prefix[position + condition.len()..]
        .chars()
        .take(240)
        .collect::<String>()
        .contains("{return")
}

pub(super) fn go_structural_analysis(node: Node<'_>, source: &str) -> Option<Value> {
    let raw = source.get(node.byte_range()).unwrap_or_default();
    if node.kind() == "index_expression" {
        let open = raw.rfind('[')?;
        let close = raw[open + 1..].find(']')? + open + 1;
        let base = raw[..open].trim();
        let index = raw[open + 1..close].trim().parse::<usize>().ok()?;
        if base.is_empty() {
            return None;
        }
        let required_len = index.saturating_add(1);
        let base_compact = compact_go_code(base);
        let len_expr = format!("len({base_compact})");
        let mut guarded = false;
        let mut saw_guard = false;
        let mut evidence = Vec::new();
        let mut function_start = 0usize;
        let mut ancestor = node.parent();
        while let Some(parent) = ancestor {
            if parent.kind() == "if_statement" {
                if let Some(condition) = parent.child_by_field_name("condition") {
                    let condition_text = source.get(condition.byte_range()).unwrap_or_default();
                    let compact = compact_go_code(condition_text);
                    if compact.contains(&len_expr) {
                        saw_guard = true;
                        if compact.contains(&format!("{len_expr}>{index}"))
                            || compact.contains(&format!("{len_expr}>={required_len}"))
                            || compact.contains(&format!("{required_len}<={len_expr}"))
                            || compact.contains(&format!("{index}<{len_expr}"))
                        {
                            guarded = true;
                        }
                        evidence.push(redacted_guard_excerpt(condition_text));
                    }
                }
            }
            if matches!(
                parent.kind(),
                "function_declaration" | "method_declaration" | "func_literal"
            ) {
                function_start = parent.start_byte();
                break;
            }
            ancestor = parent.parent();
        }
        let prefix = source
            .get(function_start..node.start_byte())
            .map(compact_go_code)
            .unwrap_or_default();
        if prefix.contains(&len_expr) {
            saw_guard = true;
            let safe_conditions = [
                format!("if{len_expr}<{required_len}"),
                format!("if{len_expr}<={index}"),
            ];
            if safe_conditions
                .iter()
                .any(|condition| early_exit_guard(&prefix, condition))
                || (required_len == 1 && early_exit_guard(&prefix, &format!("if{len_expr}==0")))
            {
                guarded = true;
            }
        }
        let guard_mismatch = saw_guard && !guarded;
        let mut patterns = Vec::new();
        if guard_mismatch {
            patterns.push("index_mismatch");
        }
        if !guarded && base.contains('.') {
            patterns.push("unguarded_subscript");
        }
        return Some(json!({
            "guarded": guarded,
            "guard_kind": "length",
            "guard_mismatch": guard_mismatch,
            "required_len": required_len,
            "guard_evidence": evidence,
            "bug_patterns": patterns,
            "pattern_propagation": {"node_kind":"index_expression","required_len":required_len}
        }));
    }
    if node.kind() == "unary_expression" && raw.trim_start().starts_with('*') {
        let operand = raw.trim().trim_start_matches('*').trim();
        if operand.is_empty() {
            return None;
        }
        let operand_compact = compact_go_code(operand);
        let mut guarded = false;
        let mut evidence = Vec::new();
        let mut function_start = 0usize;
        let mut ancestor = node.parent();
        while let Some(parent) = ancestor {
            if parent.kind() == "if_statement" {
                if let Some(condition) = parent.child_by_field_name("condition") {
                    let condition_text = source.get(condition.byte_range()).unwrap_or_default();
                    let compact = compact_go_code(condition_text);
                    if compact.contains(&format!("{operand_compact}!=nil")) {
                        guarded = true;
                        evidence.push(redacted_guard_excerpt(condition_text));
                    }
                }
            }
            if matches!(
                parent.kind(),
                "function_declaration" | "method_declaration" | "func_literal"
            ) {
                function_start = parent.start_byte();
                break;
            }
            ancestor = parent.parent();
        }
        let prefix = source
            .get(function_start..node.start_byte())
            .map(compact_go_code)
            .unwrap_or_default();
        if early_exit_guard(&prefix, &format!("if{operand_compact}==nil")) {
            guarded = true;
        }
        return Some(json!({
            "guarded": guarded,
            "guard_kind": "nil",
            "bug_patterns": if guarded { Vec::<&str>::new() } else { vec!["nil_deref"] },
            "guard_evidence": evidence
        }));
    }
    if node.kind() == "assignment_statement" {
        let compact = compact_go_code(raw);
        if compact.starts_with("_=") && raw.contains('(') {
            return Some(json!({
                "guarded": false,
                "bug_patterns": ["err_swallowed"],
                "pattern_confidence": "heuristic",
                "pattern_propagation": {"node_kind":"assignment_statement","pattern":"err_swallowed"}
            }));
        }
    }
    if node.kind() == "call_expression"
        && raw.contains(".Run(")
        && raw.contains("func(")
        && ![
            "assert.",
            "require.",
            ".Error(",
            ".Errorf(",
            ".Fatal(",
            ".Fatalf(",
            ".Fail(",
            ".FailNow(",
        ]
        .iter()
        .any(|marker| raw.contains(marker))
    {
        return Some(json!({
            "guarded": false,
            "bug_patterns": ["empty_test"],
            "pattern_confidence": "heuristic",
            "pattern_propagation": {"node_kind":"call_expression","pattern":"empty_test"}
        }));
    }
    None
}

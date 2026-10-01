use super::*;

#[test]
fn credential_redaction_covers_ci_machine_credentials_without_erasing_identifiers() {
    let credential = format!("wci_{}", "a1".repeat(32));
    let input = format!("机器({credential})\nnext");
    let (safe, redacted) = crate::workspace::redact_sensitive_text(&input);
    assert!(redacted);
    assert_eq!(safe, "机器([REDACTED])\nnext");
    for input in [
        format!("wci_{}", "g".repeat(64)),
        format!("wci_{}", "a".repeat(63)),
        format!("prefix_{credential}"),
    ] {
        assert_eq!(redact_credential_tokens(&input), (input, false));
    }
}

#[test]
fn credential_redaction_native_opaque_credentials_require_exact_hex_shape() {
    let credential = format!("wct_{}", "a1".repeat(32));
    let input = format!("海({credential}) {credential}🌊");
    assert_eq!(
        redact_credential_tokens(&input),
        ("海([REDACTED]) [REDACTED]🌊".into(), true)
    );
    for value in [
        format!("wct_{}", "x".repeat(64)),
        format!("wct_{}", "a".repeat(63)),
        format!("wct_{}", "a".repeat(65)),
        format!("prefix_{credential}"),
    ] {
        assert_eq!(redact_credential_tokens(&value), (value, false));
    }
    let (safe, redacted) =
        crate::workspace::redact_sensitive_text(&format!("begin\nreceived {credential}\nend"));
    assert!(redacted);
    assert!(!safe.contains(&credential));
    assert_eq!(safe.lines().count(), 3);
}

#[test]
fn credential_redaction_recognizes_all_prefixes_unicode_and_multiple_tokens() {
    for prefix in ["github_pat_", "ghp_", "gho_", "ghu_", "ghs_", "ghr_"] {
        let credential = format!("{prefix}{}", "x".repeat(20));
        let input = format!("海浪({credential}) {credential}🌊");
        let (safe, redacted) = redact_credential_tokens(&input);
        assert!(redacted);
        assert_eq!(safe, "海浪([REDACTED]) [REDACTED]🌊");
    }
}

#[test]
fn credential_redaction_preserves_short_prefixes_identifiers_and_nonsecret_source() {
    for value in [
        "pub fn ordinary() {}",
        "ghp_short",
        "github_pat_",
        "海🌊",
        "",
        "\r\n",
    ] {
        assert_eq!(redact_credential_tokens(value), (value.to_owned(), false));
    }
    let identifier = format!("test_ghp_{}", "x".repeat(20));
    assert_eq!(redact_credential_tokens(&identifier), (identifier, false));
}

#[test]
fn credential_redaction_canonical_source_and_command_text_keep_rows_and_safe_content() {
    let credential = format!("{}{}", "ghp_", "x".repeat(20));
    for input in [
        format!("// credential: {credential}\npub fn safe() {{}}"),
        format!("build started\nreceived {credential}\nbuild finished"),
    ] {
        let (safe, redacted) = crate::workspace::redact_sensitive_text(&input);
        assert!(redacted);
        assert!(!safe.contains(&credential));
        assert_eq!(safe.lines().count(), input.lines().count());
        assert_eq!(safe.lines().last(), input.lines().last());
        assert_eq!(crate::workspace::redact_sensitive_text(&safe).0, safe);
    }
    assert_eq!(
        crate::scopes::source_scope("src/workspace/operations/credential_redaction.rs"),
        Some(crate::scopes::ProductScope::Workspace)
    );
}

// Recognizable bearer credentials can occur in comments or raw command output,
// without a named assignment. Redact the complete token while preserving rows.
pub(super) fn redact_credential_tokens(line: &str) -> (String, bool) {
    let prefixes = [
        "github_pat_",
        "ghp_",
        "gho_",
        "ghu_",
        "ghs_",
        "ghr_",
        "wct_",
        "wci_",
    ];
    let bytes = line.as_bytes();
    let token_byte = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    let mut cursor = 0;
    let mut copied = 0;
    let mut output = String::new();
    while cursor < bytes.len() {
        let boundary = cursor == 0 || !token_byte(bytes[cursor - 1]);
        let prefix = boundary
            .then(|| {
                prefixes
                    .iter()
                    .find(|prefix| bytes[cursor..].starts_with(prefix.as_bytes()))
            })
            .flatten();
        if let Some(prefix) = prefix {
            let end = bytes[cursor..]
                .iter()
                .position(|byte| !token_byte(*byte))
                .map_or(bytes.len(), |length| cursor + length);
            let recognized = if matches!(*prefix, "wct_" | "wci_") {
                end - cursor == 68 && bytes[cursor + 4..end].iter().all(u8::is_ascii_hexdigit)
            } else {
                end - cursor >= prefix.len() + 20
            };
            if recognized {
                output.push_str(&line[copied..cursor]);
                output.push_str("[REDACTED]");
                copied = end;
                cursor = end;
                continue;
            }
        }
        cursor += 1;
    }
    if copied == 0 {
        return (line.to_owned(), false);
    }
    output.push_str(&line[copied..]);
    (output, true)
}

#[cfg(test)]
#[path = "../../../tests/unit/workspace/credential_redaction.rs"]
mod tests;

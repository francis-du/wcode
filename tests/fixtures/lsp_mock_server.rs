use std::io::{self, BufRead, Read, Write};

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    let mut opened = 0u64;
    let mut changed = 0u64;
    let mut closed = 0u64;

    while let Some(body) = read_message(&mut input)? {
        let method = json_string_field(&body, "method").unwrap_or_default();
        let id = json_u64_field(&body, "id");
        match method.as_str() {
            "initialize" => {
                let Some(id) = id else { continue };
                respond(
                    &mut output,
                    &format!(
                        r#"{{"jsonrpc":"2.0","id":{id},"result":{{"capabilities":{{"positionEncoding":"utf-8","textDocumentSync":{{"openClose":true,"change":2}},"documentSymbolProvider":true,"definitionProvider":true,"referencesProvider":true,"implementationProvider":true,"hoverProvider":true,"callHierarchyProvider":true}}}}}}"#
                    ),
                )?;
            }
            "textDocument/didOpen" => opened += 1,
            "textDocument/didChange" => changed += 1,
            "textDocument/didClose" => closed += 1,
            "textDocument/hover" => {
                if let Some(id) = id {
                    respond(
                        &mut output,
                        &format!(
                            r#"{{"jsonrpc":"2.0","id":{id},"result":{{"contents":"mock-hover"}}}}"#
                        ),
                    )?;
                }
            }
            "mock/state" => {
                if let Some(id) = id {
                    respond(
                        &mut output,
                        &format!(
                            r#"{{"jsonrpc":"2.0","id":{id},"result":{{"opened":{opened},"changed":{changed},"closed":{closed}}}}}"#
                        ),
                    )?;
                }
            }
            "shutdown" => {
                if let Some(id) = id {
                    respond(
                        &mut output,
                        &format!(r#"{{"jsonrpc":"2.0","id":{id},"result":null}}"#),
                    )?;
                }
            }
            _ => {
                if let Some(id) = id {
                    respond(
                        &mut output,
                        &format!(r#"{{"jsonrpc":"2.0","id":{id},"result":null}}"#),
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn read_message(input: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let bytes = input.read_line(&mut line)?;
        if bytes == 0 {
            return Ok(None);
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            if content_length.is_some() {
                break;
            }
            continue;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
            );
        }
    }
    let length = content_length
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing Content-Length"))?;
    let mut body = vec![0u8; length];
    input.read_exact(&mut body)?;
    String::from_utf8(body)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn respond(output: &mut impl Write, body: &str) -> io::Result<()> {
    write!(output, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
    output.flush()
}

fn json_string_field(body: &str, field: &str) -> Option<String> {
    let marker = format!(r#""{field}":""#);
    let start = body.find(&marker)? + marker.len();
    let tail = &body[start..];
    let end = tail.find('"')?;
    Some(tail[..end].to_owned())
}

fn json_u64_field(body: &str, field: &str) -> Option<u64> {
    let marker = format!(r#""{field}":"#);
    let start = body.find(&marker)? + marker.len();
    let digits = body[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    (!digits.is_empty()).then(|| digits.parse().ok()).flatten()
}

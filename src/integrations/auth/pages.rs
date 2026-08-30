use super::*;

pub(super) fn authorize_landing_html(public_url: &str) -> String {
    let mcp_url = format!("{public_url}/mcp");
    simple_auth_html(
        "OAuth is ready",
        "Add the MCP endpoint below to a compatible AI client and choose OAuth. The client will return here to complete the authorization flow.",
        Some(&mcp_url),
    )
}

pub(super) fn authorize_error_html(error: &str) -> String {
    simple_auth_html(
        "Authorization request rejected",
        &format!("The MCP client sent an invalid OAuth request: {}. Remove the connection and add the MCP endpoint again.", html_escape(error)),
        None,
    )
}

pub(super) fn simple_auth_html(title: &str, message: &str, endpoint: Option<&str>) -> String {
    let endpoint_html = endpoint
        .map(|value| format!(r#"<div class="endpoint">{}</div>"#, html_escape(value)))
        .unwrap_or_default();
    format!(
        r##"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="color-scheme" content="dark"><title>{title} · wcode</title><style>*{{box-sizing:border-box}}body{{margin:0;min-height:100vh;display:grid;place-items:center;background:#09090b;color:#f5f5f6;font:15px/1.55 ui-sans-serif,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;padding:24px}}main{{width:min(100%,520px)}}.card{{padding:28px;border:1px solid #29292f;border-radius:18px;background:#111114;box-shadow:0 28px 80px #0008}}h1{{margin:0 0 8px;font-size:24px}}p{{margin:0;color:#a1a1aa}}.endpoint{{margin-top:20px;padding:13px 15px;border:1px solid #323239;border-radius:12px;background:#09090b;color:#fafafa;font:12px ui-monospace,SFMono-Regular,Menlo,monospace;overflow:auto}}.links{{display:flex;gap:16px;flex-wrap:wrap;margin-top:18px}}a{{display:inline-block;color:#fff}}</style></head><body><main><section class="card"><h1>{title}</h1><p>{message}</p>{endpoint_html}<div class="links"><a href="{project_url}" target="_blank" rel="noreferrer">Project ↗</a><a href="{author_url}" target="_blank" rel="noreferrer">{author_handle} ↗</a></div></section></main></body></html>"##,
        title = html_escape(title),
        message = message,
        project_url = PROJECT_URL,
        author_url = AUTHOR_URL,
        author_handle = AUTHOR_HANDLE,
    )
}

pub(super) fn authorize_html(query: &AuthorizeQuery, error: Option<&str>) -> String {
    let error_html = error
        .map(|message| {
            format!(
                r#"<div class="error"><span>!</span>{}</div>"#,
                html_escape(message)
            )
        })
        .unwrap_or_default();
    format!(
        r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="color-scheme" content="dark">
<title>Authorize · wcode</title>
<style>
:root{{--bg:#09090b;--panel:#111114;--panel2:#17171b;--line:#29292f;--text:#f5f5f6;--muted:#9b9ba7;--accent:#ffffff;--danger:#ff6b6b}}
*{{box-sizing:border-box}}body{{margin:0;min-height:100vh;display:grid;place-items:center;background:radial-gradient(900px 500px at 50% -15%,#25252d 0%,var(--bg) 62%);color:var(--text);font:15px/1.55 ui-sans-serif,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;padding:24px}}
.shell{{width:min(100%,460px)}}.brand{{display:flex;align-items:center;gap:12px;margin:0 0 18px 4px}}.mark{{width:34px;height:34px;border:1px solid #3a3a42;border-radius:10px;display:grid;place-items:center;background:linear-gradient(145deg,#24242a,#101012);font:700 15px ui-monospace,SFMono-Regular,Menlo,monospace;box-shadow:0 8px 30px #0008}}.brand strong{{font-size:15px;letter-spacing:.1px}}.brand span{{display:block;color:var(--muted);font-size:12px}}
.card{{background:linear-gradient(180deg,#151519,#101013);border:1px solid var(--line);border-radius:18px;padding:28px;box-shadow:0 28px 80px #0009}}h1{{font-size:24px;line-height:1.2;margin:0 0 8px;letter-spacing:-.45px}}p{{margin:0;color:var(--muted)}}.scope{{display:flex;gap:10px;align-items:flex-start;margin:22px 0;padding:13px 14px;border:1px solid #27272d;background:#0c0c0f;border-radius:12px}}.scope svg{{flex:0 0 auto;margin-top:2px}}.scope b{{display:block;font-size:13px;margin-bottom:2px}}.scope span{{font-size:12px;color:var(--muted)}}
label{{display:block;font-size:12px;font-weight:650;color:#d7d7dc;margin:0 0 7px}}input.code{{width:100%;height:54px;border-radius:12px;border:1px solid #323239;background:#09090b;color:#fff;outline:none;padding:0 16px;font:600 22px/1 ui-monospace,SFMono-Regular,Menlo,monospace;letter-spacing:8px;text-align:center;transition:.15s border,.15s box-shadow}}input.code:focus{{border-color:#74747f;box-shadow:0 0 0 3px #ffffff12}}input.code::placeholder{{font:400 14px ui-sans-serif,-apple-system,sans-serif;letter-spacing:0;color:#666670}}
button{{width:100%;height:48px;margin-top:12px;border:0;border-radius:12px;background:var(--accent);color:#09090b;font-weight:750;font-size:14px;cursor:pointer;transition:.15s transform,.15s opacity}}button:hover{{opacity:.9}}button:active{{transform:translateY(1px)}}.error{{display:flex;align-items:center;gap:9px;margin:0 0 14px;padding:10px 12px;border:1px solid #5a2929;background:#251313;color:#ffb4b4;border-radius:10px;font-size:12px}}.error span{{display:grid;place-items:center;width:18px;height:18px;border-radius:50%;background:#ff6b6b;color:#160606;font-weight:900}}
.foot{{display:flex;justify-content:space-between;align-items:center;margin-top:16px;padding:0 4px;font-size:12px;color:#73737d}}a{{color:#b8b8c0;text-decoration:none}}a:hover{{color:#fff}}.dot{{width:7px;height:7px;border-radius:50%;background:#5ee28a;box-shadow:0 0 12px #5ee28a99;display:inline-block;margin-right:7px}}
</style>
</head>
<body><main class="shell">
<div class="brand"><div class="mark">WC</div><div><strong>wcode</strong><span>Software Intelligence Runtime</span></div></div>
<section class="card"><h1>Authorize model access</h1><p>Allow this model or agent to use the Software Intelligence Runtime for the configured local workspaces.</p>
<div class="scope"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="#bdbdc6" stroke-width="1.7"><path d="M12 3l8 4v5c0 5-3.4 8.3-8 9-4.6-.7-8-4-8-9V7l8-4z"/><path d="M9 12l2 2 4-4"/></svg><div><b>Workspace-scoped access</b><span>Paths remain limited to the configured roots. Write and command permissions follow the CLI flags.</span></div></div>
{error_html}
<form method="post" action="/authorize">
<input type="hidden" name="client_id" value="{}"><input type="hidden" name="redirect_uri" value="{}"><input type="hidden" name="state" value="{}"><input type="hidden" name="code_challenge" value="{}">
<input type="hidden" name="resource" value="{}"><input type="hidden" name="scope" value="{}">
<label for="pairing_code">6-digit pairing code</label><input class="code" id="pairing_code" name="pairing_code" inputmode="numeric" pattern="[0-9]{{6}}" maxlength="6" autocomplete="one-time-code" placeholder="Enter code" required autofocus spellcheck="false">
<button type="submit">Authorize connection</button></form></section>
<div class="foot"><span><i class="dot"></i>OAuth 2.1 · PKCE</span><span><a href="{project_url}" target="_blank" rel="noreferrer">Project ↗</a> · <a href="{author_url}" target="_blank" rel="noreferrer">{author_handle}</a></span></div>
</main></body></html>"##,
        html_escape(&query.client_id),
        html_escape(&query.redirect_uri),
        html_escape(&query.state),
        html_escape(&query.code_challenge),
        html_escape(query.resource.as_deref().unwrap_or_default()),
        html_escape(query.scope.as_deref().unwrap_or_default()),
        project_url = PROJECT_URL,
        author_url = AUTHOR_URL,
        author_handle = AUTHOR_HANDLE,
    )
}

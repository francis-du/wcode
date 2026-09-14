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
        r##"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover"><meta name="color-scheme" content="dark light"><meta name="theme-color" content="#0b0812"><title>{title} · wcode</title><style>*{{box-sizing:border-box}}body{{margin:0;min-height:100vh;min-height:100svh;min-height:100dvh;display:grid;place-items:center;background:radial-gradient(620px 360px at 78% -8%,rgba(102,92,255,.15),transparent 72%),#0b0812;color:#faf7ff;font-family:"Inter","Noto Sans SC","Noto Sans",sans-serif;font-size:16px;font-weight:450;line-height:1.58;font-synthesis:none;text-rendering:optimizeLegibility;-webkit-font-smoothing:antialiased;padding:max(16px,env(safe-area-inset-top)) max(16px,env(safe-area-inset-right)) max(16px,env(safe-area-inset-bottom)) max(16px,env(safe-area-inset-left));overflow-x:hidden;-webkit-text-size-adjust:100%;text-size-adjust:100%}}main{{width:min(100%,560px)}}.brand-logo{{display:block;width:124px;height:auto;margin:0 0 16px;filter:drop-shadow(0 8px 22px rgba(102,92,255,.12))}}.card{{padding:24px;border:1px solid #342a40;border-radius:22px;background:linear-gradient(150deg,rgba(28,21,41,.86),rgba(21,16,32,.78));backdrop-filter:blur(20px);box-shadow:0 16px 48px rgba(0,0,0,.16)}}h1{{margin:0 0 8px;font-size:24px;line-height:1.18;font-weight:720;letter-spacing:-.02em}}p{{margin:0;color:#a89db7;overflow-wrap:anywhere}}.endpoint{{margin-top:16px;padding:11px 12px;border:1px solid #2a2234;border-radius:13px;background:#120d1c;color:#d7cfdf;font:13px/1.55 "JetBrains Mono","Noto Sans Mono",monospace;overflow:auto;overflow-wrap:anywhere;word-break:break-word;user-select:all;-webkit-user-select:all;-webkit-overflow-scrolling:touch}}.links{{display:flex;gap:6px 14px;flex-wrap:wrap;margin-top:12px}}a{{display:inline-flex;align-items:center;min-height:34px;color:#8b7cff;text-decoration:none}}a:hover{{color:#faf7ff}}@media(prefers-color-scheme:light){{body{{background:radial-gradient(620px 360px at 78% -8%,rgba(102,92,255,.11),transparent 72%),#f8f6fc;color:#21182b}}.card{{border-color:#ded5e7;background:rgba(255,255,255,.88);box-shadow:0 16px 48px rgba(55,35,72,.08)}}p{{color:#71627f}}.endpoint{{border-color:#ded5e7;background:#f3eef8;color:#4e405c}}a:hover{{color:#21182b}}}}@media(max-width:520px){{body{{place-items:start center}}.card{{padding:16px}}a{{min-height:44px}}}}</style></head><body><main><img class="brand-logo" src="/intelligence/logo.svg" alt="wcode"><section class="card"><h1>{title}</h1><p>{message}</p>{endpoint_html}<div class="links"><a href="{project_url}" target="_blank" rel="noreferrer">Project ↗</a><a href="{author_url}" target="_blank" rel="noreferrer">{author_handle} ↗</a></div></section></main></body></html>"##,
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
<meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover">
<meta name="color-scheme" content="dark light">
<meta name="theme-color" content="#0b0812">
<title>Authorize · wcode</title>
<style>
:root{{--bg:#0b0812;--panel:#151020;--raised:#1c1529;--input:#120d1c;--line:#2f2739;--line2:#493d58;--text:#faf7ff;--muted:#a89db7;--soft:#cec5da;--faint:#746982;--accent:#8b7cff;--danger:#c27788;--good:#879583;--fontSans:"Inter","Noto Sans SC","Noto Sans",sans-serif;--fontMono:"JetBrains Mono","Noto Sans Mono",monospace}}
*{{box-sizing:border-box}}body{{margin:0;min-height:100vh;min-height:100svh;min-height:100dvh;display:grid;place-items:center;background:radial-gradient(620px 360px at 78% -8%,rgba(102,92,255,.15),transparent 72%),var(--bg);color:var(--text);font-family:var(--fontSans);font-size:16px;font-weight:450;line-height:1.58;font-synthesis:none;text-rendering:optimizeLegibility;-webkit-font-smoothing:antialiased;padding:max(16px,env(safe-area-inset-top)) max(16px,env(safe-area-inset-right)) max(16px,env(safe-area-inset-bottom)) max(16px,env(safe-area-inset-left));overflow-x:hidden;-webkit-text-size-adjust:100%;text-size-adjust:100%}}
.shell{{width:min(100%,480px)}}.brand{{display:flex;align-items:center;gap:12px;margin:0 0 16px}}.brand-logo{{display:block;width:124px;height:auto;max-width:54vw;filter:drop-shadow(0 8px 22px rgba(102,92,255,.12))}}.brand span{{display:block;color:var(--muted);font-size:12px}}
.card{{background:linear-gradient(150deg,rgba(28,21,41,.86),rgba(21,16,32,.78));border:1px solid var(--line);border-radius:22px;padding:24px;backdrop-filter:blur(20px);box-shadow:0 16px 48px rgba(0,0,0,.16)}}h1{{font-size:24px;line-height:1.18;margin:0 0 8px;letter-spacing:-.02em;font-weight:720}}p{{margin:0;color:var(--muted)}}.scope{{display:flex;gap:9px;align-items:flex-start;margin:16px 0;padding:9px 10px;border:1px solid var(--line);background:transparent;border-radius:14px}}.scope svg{{flex:0 0 auto;margin-top:2px;stroke:var(--soft)}}.scope b{{display:block;font-size:13px;margin-bottom:2px}}.scope span{{font-size:12px;color:var(--muted)}}
label{{display:block;font-size:13px;font-weight:650;color:var(--soft);margin:0 0 7px}}input.code{{width:100%;height:46px;border-radius:12px;border:1px solid var(--line2);background:var(--input);color:var(--text);outline:none;padding:0 12px;font:600 19px/1 var(--fontMono);letter-spacing:7px;text-align:center}}input.code:focus{{border-color:var(--accent);box-shadow:0 0 0 2px #8b7cff20}}input.code::placeholder{{font:400 12px var(--fontSans);letter-spacing:0;color:var(--faint)}}
button{{width:100%;height:44px;margin-top:10px;border:1px solid color-mix(in srgb,var(--accent) 52%,var(--line2));border-radius:13px;background:linear-gradient(180deg,color-mix(in srgb,var(--accent) 30%,var(--raised)),color-mix(in srgb,var(--accent) 12%,var(--raised)));color:var(--text);font-weight:680;font-size:14px;cursor:pointer}}button:hover{{border-color:var(--accent)}}button:active{{transform:translateY(1px)}}.error{{display:flex;align-items:center;gap:8px;margin:0 0 10px;padding:8px 9px;border:1px solid var(--danger);background:transparent;color:var(--danger);border-radius:13px;font-size:12px}}.error span{{display:grid;place-items:center;width:16px;height:16px;border-radius:50%;border:1px solid var(--danger);color:var(--danger);font-weight:900}}
.foot{{display:flex;justify-content:space-between;align-items:center;gap:7px 12px;flex-wrap:wrap;margin-top:10px;font-size:12px;color:var(--muted)}}a{{display:inline-flex;align-items:center;min-height:34px;color:var(--soft);text-decoration:none}}a:hover{{color:var(--text)}}.dot{{width:6px;height:6px;border-radius:50%;background:var(--good);display:inline-block;margin-right:6px}}@media(prefers-color-scheme:light){{:root{{--bg:#f8f6fc;--panel:#ffffff;--raised:#f0ebf6;--input:#f3eef8;--line:#ded5e7;--line2:#cbbfd7;--text:#21182b;--muted:#71627f;--soft:#4e405c;--faint:#8b7f98;--accent:#665cff;--danger:#a65368;--good:#62755f}}.card{{background:linear-gradient(150deg,rgba(255,255,255,.92),rgba(248,246,252,.88));box-shadow:0 16px 48px rgba(55,35,72,.08)}}}}@media(max-width:520px){{body{{place-items:start center}}.card{{padding:16px}}h1{{font-size:20px}}.scope{{margin:13px 0;padding:9px}}input.code{{height:48px;padding:0 9px;font-size:19px;letter-spacing:6px}}button,a{{min-height:44px}}.foot{{align-items:flex-start;justify-content:flex-start}}}}
</style>
</head>
<body><main class="shell">
<div class="brand"><img class="brand-logo" src="/intelligence/logo.svg" alt="wcode"><span>Engineering Control Plane</span></div>
<section class="card"><h1>Authorize model access</h1><p>Allow this model or agent to use the governed wcode engineering runtime for the configured local workspaces.</p>
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

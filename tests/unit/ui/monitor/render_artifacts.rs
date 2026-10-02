use super::*;
use std::fmt::Write as _;

fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn color(color: Color, fallback: &str) -> String {
    match color {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        _ => fallback.to_owned(),
    }
}

fn frame_svg(buffer: &ratatui::buffer::Buffer, title: &str) -> String {
    let width = usize::from(buffer.area.width);
    let height = usize::from(buffer.area.height);
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\" role=\"img\" aria-labelledby=\"title desc\"><title id=\"title\">{}</title><desc id=\"desc\">Production Ratatui renderer with explicit test fixture data. No live connection or authorization was executed.</desc><rect width=\"100%\" height=\"100%\" fill=\"#0b1020\"/><g font-family=\"Menlo,Consolas,monospace\" font-size=\"14\">",
        width * 9,
        height * 20 + 32,
        width * 9,
        height * 20 + 32,
        xml(title)
    );
    for (y, row) in buffer.content.chunks(width).enumerate() {
        let mut x = 0;
        while x < row.len() {
            let cell = &row[x];
            let columns = Span::raw(cell.symbol()).width().max(1);
            write!(
                svg,
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"20\" fill=\"{}\"/><text x=\"{}\" y=\"{}\" fill=\"{}\" xml:space=\"preserve\"{}>{}</text>",
                x * 9,
                y * 20,
                columns * 9,
                color(cell.bg, "#0b1020"),
                x * 9,
                y * 20 + 15,
                color(cell.fg, "#e5eaf2"),
                if cell.modifier.contains(Modifier::BOLD) { " font-weight=\"700\"" } else { "" },
                xml(cell.symbol())
            )
            .unwrap();
            x += columns;
        }
    }
    write!(svg, "<text x=\"12\" y=\"{}\" fill=\"#a4b0c4\">Demo · test fixture · {width} × {height}</text></g></svg>", height * 20 + 22).unwrap();
    svg
}

#[test]
fn production_dashboard_render_review_exports_all_operator_surfaces() {
    let (_root, workspaces) = monitor_test_workspaces(&["backend", "frontend"]);
    let config = monitor_test_config(workspaces);
    let monitor = TaskMonitor::new(["backend".to_owned(), "frontend".to_owned()]);
    monitor.mark_mcp_initialized();
    seed_engineering_state(&monitor, "backend");
    let running = monitor.queue("backend", "read_file", "src/lib.rs", 42);
    running.start();
    let _queued = monitor.queue("frontend", "search", "button", 24);
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/wcode-tui-review");
    std::fs::create_dir_all(&directory).unwrap();
    let mut records = Vec::new();
    for language in [UiLanguage::En, UiLanguage::ZhCn] {
        for (name, width, height) in [
            ("wide", 120, 34),
            ("compact", 80, 24),
            ("tiny", 40, 8),
            ("help", 100, 26),
            ("commands", 100, 26),
            ("project-details", 120, 34),
            ("full-access", 100, 26),
            ("workspace-input", 100, 26),
            ("command-authorization", 100, 26),
            ("human-decision", 100, 26),
            ("operation-feedback", 100, 26),
        ] {
            let mut ui = DashboardState {
                language,
                workspace_focus_id: Some("backend".to_owned()),
                ..DashboardState::default()
            };
            let expected: &str = match name {
                "help" => {
                    ui.help_open = true;
                    &config.local_health_url
                }
                "commands" => {
                    ui.commands_open = true;
                    language.tr("SUPPORTED COMMANDS")
                }
                "project-details" => {
                    ui.intelligence_open = true;
                    language.tr("PROJECT DETAILS")
                }
                "full-access" => {
                    ui.full_access_confirm = true;
                    language.tr("FULL ACCESS")
                }
                "workspace-input" => {
                    ui.workspace_input = Some("/workspace/new-project".to_owned());
                    "/workspace/new-project"
                }
                "command-authorization" | "human-decision" => {
                    let mut request = monitor_test_request();
                    if name == "human-decision" {
                        request.kind = crate::authorization::AuthorizationKind::HumanDecision;
                        request.summary = "Review fixture acceptance decision".to_owned();
                        request.program = None;
                    }
                    ui.pending_authorizations.push(request);
                    "AUTH-00000001"
                }
                "operation-feedback" => {
                    ui.workspace_message =
                        Some("Fixture operation failed; retry is available".to_owned());
                    "Fixture operation failed"
                }
                "tiny" => " ^C ",
                _ => "read_file",
            };
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| draw_dashboard(frame, &monitor.snapshot(), &config, 0, &ui))
                .unwrap();
            let buffer = terminal.backend().buffer();
            let mut text = String::new();
            for row in buffer.content.chunks(usize::from(width)) {
                let mut x = 0;
                while x < row.len() {
                    text.push_str(row[x].symbol());
                    x += Span::raw(row[x].symbol()).width().max(1);
                }
                text.push('\n');
            }
            assert!(
                text.contains(expected),
                "missing {expected} in {name}/{language:?}: {text}"
            );
            let locale = if language == UiLanguage::En {
                "en"
            } else {
                "zh-CN"
            };
            let file = format!("{name}-{locale}.svg");
            std::fs::write(
                directory.join(&file),
                frame_svg(buffer, &format!("wcode {name} · {locale}")),
            )
            .unwrap();
            records.push(serde_json::json!({"surface": name, "language": locale, "width": width, "height": height, "file": file, "fixture": true}));
        }
    }
    std::fs::write(
        directory.join("manifest.json"),
        serde_json::to_vec_pretty(&records).unwrap(),
    )
    .unwrap();
    assert_eq!(records.len(), 22);
}

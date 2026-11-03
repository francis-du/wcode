use super::*;

pub(super) fn print_setup_guide(
    workspaces: &Workspaces,
    local_url: &str,
    public_url: &str,
    pairing_code: &str,
    intelligence_url: &str,
    options: SetupGuideOptions,
) {
    let SetupGuideOptions {
        local_only,
        max_parallel_tools,
        max_cpu_percent,
        max_memory_mb,
        input_token_price_per_million_usd,
        security,
    } = options;
    println!(
        "\n╭─ wcode {} ─────────────────────────────────────────",
        env!("CARGO_PKG_VERSION")
    );
    println!("│");
    println!("│  MCP        {public_url}/mcp");
    println!("│  Local      {local_url}");
    println!("│  Dashboard  {intelligence_url}");
    println!("│  Verify code  {pairing_code}");
    let resources = resource::snapshot();
    let execution_limit = ToolHarness::execution_limit(max_parallel_tools);
    println!(
        "│  Capacity   TOOL {max_parallel_tools} · EXEC {execution_limit} · PROC {} · GIT {}",
        resources.child_queue.limit, resources.probe_queue.limit
    );
    println!("│  Resources  BG {max_cpu_percent:.1}% CPU target · burst-friendly · {max_memory_mb} MiB soft RSS");
    println!("│  Token EST  ~4 bytes/token · ${input_token_price_per_million_usd:.2}/M input");
    println!(
        "│  Security   semantic {} · risky-exec {} · destructive {} · overlap {} · broad {}",
        if security.allow_semantic_exec {
            "auto"
        } else {
            "off"
        },
        if security.allow_risky_exec {
            "on"
        } else {
            "off"
        },
        if security.allow_destructive_writes {
            "on"
        } else {
            "off"
        },
        if security.allow_overlapping_workspaces {
            "on"
        } else {
            "off"
        },
        if security.allow_broad_workspace {
            "on"
        } else {
            "off"
        },
    );
    println!("│  Fan-out    parallel_tools / review_changes / verify_project");
    println!("│");
    println!("│  Workspaces");
    for (id, root) in workspaces.roots() {
        let default = if id == workspaces.default_id() {
            "  default"
        } else {
            ""
        };
        println!("│    • {id:<16} {}{default}", root.display());
    }
    println!("│");
    if local_only {
        println!("│  Local-only mode: remote MCP clients cannot reach this endpoint.");
        println!("│  Restart without --no-tunnel or pass --public-url.");
    } else {
        println!("│  Connect");
        println!("│    1  Open the wcode setup hub: {public_url}");
        println!("│    2  Pick Grok, Claude, ChatGPT, Mistral, or Other MCP client");
        println!("│    3  The shared MCP URL is {public_url}/mcp");
        println!("│    4  Auth  OAuth · enter pairing code {pairing_code}");
    }
    println!("│");
    println!("│  Project    {PROJECT_URL}");
    println!("│  Author     {AUTHOR_HANDLE} · {AUTHOR_URL}");
    println!("╰─ Ctrl-C to stop ─────────────────────────────────────\n");
}

use super::*;
use crate::evidence_store::workspace_state_directory;

#[derive(Clone, Debug, Serialize)]
pub struct SemanticProviderInstallPlan {
    pub provider: String,
    pub strategy: &'static str,
    pub manager: &'static str,
    pub model_can_install: bool,
    pub requires_approval: bool,
    pub program: Option<String>,
    pub args: Vec<String>,
    pub destination: Option<String>,
    pub post_install_action: &'static str,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct SemanticProviderInstallResult {
    pub language: SemanticLanguage,
    pub provider: String,
    pub plan: SemanticProviderInstallPlan,
    pub executed: bool,
    pub success: bool,
    pub available: bool,
    pub executable: Option<String>,
    pub message: String,
}

pub(super) fn canonical_provider(language: SemanticLanguage) -> Option<ProviderCandidate> {
    PROVIDERS
        .iter()
        .copied()
        .find(|provider| provider.canonical && provider.languages.contains(&language))
}

fn tool_root(workspace: &Workspace, provider: ProviderCandidate) -> Result<PathBuf> {
    Ok(workspace_state_directory(workspace)?
        .join("language-tools")
        .join(provider.id))
}

fn npm_plan(
    workspace: &Workspace,
    provider: ProviderCandidate,
    packages: &[&str],
) -> Result<SemanticProviderInstallPlan> {
    let destination = tool_root(workspace, provider)?.join("npm");
    let mut args = vec![
        "install".to_owned(),
        "--prefix".to_owned(),
        destination.display().to_string(),
        "--no-audit".to_owned(),
        "--no-fund".to_owned(),
        "--ignore-scripts".to_owned(),
    ];
    args.extend(packages.iter().map(|package| (*package).to_owned()));
    Ok(SemanticProviderInstallPlan {
        provider: provider.id.to_owned(),
        strategy: "wcode_managed",
        manager: "npm",
        model_can_install: true,
        requires_approval: true,
        program: Some("npm".to_owned()),
        args,
        destination: Some(destination.display().to_string()),
        post_install_action: "semantic_provider_refresh",
        reason: "Install the canonical language server into the wcode-owned state directory; the repository tree and global npm prefix remain unchanged.".to_owned(),
    })
}

pub(super) fn install_plan(
    workspace: &Workspace,
    language: SemanticLanguage,
) -> Result<SemanticProviderInstallPlan> {
    let provider = canonical_provider(language).ok_or_else(|| {
        anyhow!(
            "no canonical semantic provider is registered for {}",
            language.as_str()
        )
    })?;
    match provider.id {
        "rust-analyzer" => Ok(SemanticProviderInstallPlan {
            provider: provider.id.to_owned(),
            strategy: "toolchain",
            manager: "rustup",
            model_can_install: true,
            requires_approval: true,
            program: Some("rustup".to_owned()),
            args: vec![
                "component".to_owned(),
                "add".to_owned(),
                "rust-analyzer".to_owned(),
                "rust-src".to_owned(),
            ],
            destination: None,
            post_install_action: "semantic_provider_refresh",
            reason: "Use rustup's signed toolchain component channel so rust-analyzer matches the active Rust toolchain.".to_owned(),
        }),
        "gopls" => Ok(SemanticProviderInstallPlan {
            provider: provider.id.to_owned(),
            strategy: "toolchain",
            manager: "go",
            model_can_install: true,
            requires_approval: true,
            program: Some("go".to_owned()),
            args: vec![
                "install".to_owned(),
                "golang.org/x/tools/gopls@latest".to_owned(),
            ],
            destination: None,
            post_install_action: "semantic_provider_refresh",
            reason: "Use the Go toolchain's canonical gopls install path; wcode discovery already checks GOBIN, GOPATH/bin and ~/go/bin.".to_owned(),
        }),
        "bash-language-server" => npm_plan(workspace, provider, &["bash-language-server"]),
        "vscode-css-language-server" | "vscode-html-language-server" => {
            npm_plan(workspace, provider, &["vscode-langservers-extracted"])
        }
        "typescript-language-server" => npm_plan(
            workspace,
            provider,
            &["typescript-language-server", "typescript@6"],
        ),
        "pyright" => npm_plan(workspace, provider, &["pyright"]),
        "csharp-ls" => {
            let destination = tool_root(workspace, provider)?.join("dotnet");
            Ok(SemanticProviderInstallPlan {
                provider: provider.id.to_owned(),
                strategy: "wcode_managed",
                manager: "dotnet",
                model_can_install: true,
                requires_approval: true,
                program: Some("dotnet".to_owned()),
                args: vec![
                    "tool".to_owned(),
                    "install".to_owned(),
                    "--tool-path".to_owned(),
                    destination.display().to_string(),
                    "csharp-ls".to_owned(),
                ],
                destination: Some(destination.display().to_string()),
                post_install_action: "semantic_provider_refresh",
                reason: "Install the canonical .NET tool into a wcode-owned tool directory rather than the user's global tool set.".to_owned(),
            })
        }
        "ocamllsp" => Ok(SemanticProviderInstallPlan {
            provider: provider.id.to_owned(),
            strategy: "toolchain",
            manager: "opam",
            model_can_install: true,
            requires_approval: true,
            program: Some("opam".to_owned()),
            args: vec![
                "install".to_owned(),
                "ocaml-lsp-server".to_owned(),
                "--yes".to_owned(),
            ],
            destination: None,
            post_install_action: "semantic_provider_refresh",
            reason: "OCaml-LSP must match the active opam switch; install it into that switch and then refresh semantic providers.".to_owned(),
        }),
        "ruby-lsp" => Ok(SemanticProviderInstallPlan {
            provider: provider.id.to_owned(),
            strategy: "toolchain",
            manager: "gem",
            model_can_install: true,
            requires_approval: true,
            program: Some("gem".to_owned()),
            args: vec!["install".to_owned(), "ruby-lsp".to_owned()],
            destination: None,
            post_install_action: "semantic_provider_refresh",
            reason: "Install the canonical Ruby LSP through RubyGems for the active Ruby environment.".to_owned(),
        }),
        "clangd" => manual_plan(provider, "llvm", "Install clangd from the platform LLVM toolchain/package manager, then refresh semantic providers."),
        "dart-language-server" => manual_plan(provider, "dart-sdk", "The Dart language server ships with the Dart SDK; install or repair the Dart SDK, then refresh semantic providers."),
        "sourcekit-lsp" => manual_plan(provider, "swift-toolchain", "sourcekit-lsp ships with the Swift toolchain; install or repair Swift, then refresh semantic providers."),
        "jdtls" => manual_plan(provider, "java-tooling", "Install Eclipse JDT Language Server using the platform/toolchain package flow, then refresh semantic providers."),
        "elixir-ls" => manual_plan(provider, "elixir-tooling", "Install ElixirLS for the active Elixir/OTP toolchain, then refresh semantic providers."),
        "lua-language-server" => manual_plan(provider, "lua-tooling", "Install Lua Language Server from the platform package manager or official release, then refresh semantic providers."),
        "phpactor" => manual_plan(provider, "php-tooling", "Install Phpactor for the active PHP environment, then refresh semantic providers."),
        "r-languageserver" => manual_plan(provider, "r-tooling", "Install the R languageserver package for the active R library, then refresh semantic providers."),
        other => manual_plan(
            provider,
            "language-toolchain",
            &format!("Install the canonical provider {other} for the active language toolchain, then refresh semantic providers."),
        ),
    }
}

fn manual_plan(
    provider: ProviderCandidate,
    manager: &'static str,
    reason: &str,
) -> Result<SemanticProviderInstallPlan> {
    Ok(SemanticProviderInstallPlan {
        provider: provider.id.to_owned(),
        strategy: "manual",
        manager,
        model_can_install: false,
        requires_approval: true,
        program: None,
        args: Vec::new(),
        destination: None,
        post_install_action: "semantic_provider_refresh",
        reason: reason.to_owned(),
    })
}

pub(super) fn managed_executable_candidates(workspace: &Workspace, name: &str) -> Vec<PathBuf> {
    let Some(provider) = PROVIDERS
        .iter()
        .copied()
        .find(|provider| provider.executables.contains(&name))
    else {
        return Vec::new();
    };
    let Ok(root) = tool_root(workspace, provider) else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    let npm_bin = root.join("npm").join("node_modules").join(".bin");
    candidates.push(npm_bin.join(name));
    #[cfg(windows)]
    {
        candidates.push(npm_bin.join(format!("{name}.cmd")));
        candidates.push(npm_bin.join(format!("{name}.exe")));
    }
    let dotnet = root.join("dotnet");
    candidates.push(dotnet.join(executable_name_for_host(name)));
    candidates
}

fn executable_name_for_host(name: &str) -> String {
    if cfg!(windows) && !name.to_ascii_lowercase().ends_with(".exe") {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

pub(super) fn invalidate_discovery_cache() {
    super::discovery::invalidate_rustup_component_cache();
}

pub async fn install(
    workspace: &Workspace,
    language: SemanticLanguage,
) -> Result<SemanticProviderInstallResult> {
    let provider = canonical_provider(language).ok_or_else(|| {
        anyhow!(
            "no canonical semantic provider is registered for {}",
            language.as_str()
        )
    })?;
    if let Some(executable) = provider
        .executables
        .iter()
        .find_map(|name| find_executable(workspace, name))
    {
        return Ok(SemanticProviderInstallResult {
            language,
            provider: provider.id.to_owned(),
            plan: install_plan(workspace, language)?,
            executed: false,
            success: true,
            available: true,
            executable: Some(executable.display().to_string()),
            message: "canonical LSP is already available".to_owned(),
        });
    }

    let plan = install_plan(workspace, language)?;
    if !plan.model_can_install {
        return Ok(SemanticProviderInstallResult {
            language,
            provider: provider.id.to_owned(),
            plan,
            executed: false,
            success: false,
            available: false,
            executable: None,
            message: "this provider requires an operator/toolchain-specific installation; use the returned plan instead of inventing an installer".to_owned(),
        });
    }
    let program = plan
        .program
        .as_deref()
        .ok_or_else(|| anyhow!("model-installable LSP plan is missing a program"))?;
    let operation = format!(
        "semantic_provider_install\0{}\0{}\0{}",
        language.as_str(),
        provider.id,
        plan.args.join("\0")
    );
    workspace.authorize_risky_operation(
        AuthorizationKind::RiskyExecution,
        &operation,
        &format!(
            "install canonical LSP {} for {} using {}",
            provider.id,
            language.as_str(),
            plan.manager
        ),
    )?;
    if let Some(destination) = plan.destination.as_deref() {
        std::fs::create_dir_all(destination)
            .with_context(|| format!("cannot create LSP tool directory {destination}"))?;
    }
    let result = workspace
        .run_trusted_runtime_command(program, &plan.args, ".", 600)
        .await?;
    if !result.success {
        return Ok(SemanticProviderInstallResult {
            language,
            provider: provider.id.to_owned(),
            plan,
            executed: true,
            success: false,
            available: false,
            executable: None,
            message: format!(
                "LSP installer failed with exit {:?}: {}",
                result.exit_code,
                result.stderr.lines().last().unwrap_or("no diagnostics")
            ),
        });
    }
    invalidate_discovery_cache();
    let executable = provider
        .executables
        .iter()
        .find_map(|name| find_executable(workspace, name));
    let available = executable.is_some();
    Ok(SemanticProviderInstallResult {
        language,
        provider: provider.id.to_owned(),
        plan,
        executed: true,
        success: available,
        available,
        executable: executable.map(|path| path.display().to_string()),
        message: if available {
            "canonical LSP installed and rediscovered; refresh semantic providers next".to_owned()
        } else {
            "installer completed but the canonical LSP is still not discoverable; keep syntax fallback and inspect provider status".to_owned()
        },
    })
}

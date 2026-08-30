use super::SemanticLanguage;

#[derive(Clone, Copy)]
pub(super) struct ProviderCandidate {
    pub(super) id: &'static str,
    pub(super) executables: &'static [&'static str],
    pub(super) args: &'static [&'static str],
    pub(super) languages: &'static [SemanticLanguage],
    pub(super) canonical: bool,
}

const BASH: &[SemanticLanguage] = &[SemanticLanguage::Bash];
const C_FAMILY: &[SemanticLanguage] = &[SemanticLanguage::C, SemanticLanguage::Cpp];
const CSHARP: &[SemanticLanguage] = &[SemanticLanguage::CSharp];
const CSS: &[SemanticLanguage] = &[SemanticLanguage::Css];
const DART: &[SemanticLanguage] = &[SemanticLanguage::Dart];
const ELIXIR: &[SemanticLanguage] = &[SemanticLanguage::Elixir];
const GO: &[SemanticLanguage] = &[SemanticLanguage::Go];
const HTML: &[SemanticLanguage] = &[SemanticLanguage::Html];
const JAVA: &[SemanticLanguage] = &[SemanticLanguage::Java];
const JS_TS: &[SemanticLanguage] = &[
    SemanticLanguage::JavaScript,
    SemanticLanguage::TypeScript,
    SemanticLanguage::Tsx,
];
const LUA: &[SemanticLanguage] = &[SemanticLanguage::Lua];
const OCAML: &[SemanticLanguage] = &[SemanticLanguage::Ocaml, SemanticLanguage::OcamlInterface];
const PHP: &[SemanticLanguage] = &[SemanticLanguage::Php];
const PYTHON: &[SemanticLanguage] = &[SemanticLanguage::Python];
const R_LANG: &[SemanticLanguage] = &[SemanticLanguage::R];
const RUBY: &[SemanticLanguage] = &[SemanticLanguage::Ruby];
const RUST: &[SemanticLanguage] = &[SemanticLanguage::Rust];
const SWIFT: &[SemanticLanguage] = &[SemanticLanguage::Swift];

pub(super) fn automatic_provider(provider: ProviderCandidate) -> bool {
    provider.id == "rust-analyzer"
}

pub(super) const PROVIDERS: &[ProviderCandidate] = &[
    ProviderCandidate {
        id: "bash-language-server",
        executables: &["bash-language-server"],
        args: &["start"],
        languages: BASH,
        canonical: true,
    },
    ProviderCandidate {
        id: "clangd",
        executables: &["clangd"],
        args: &[],
        languages: C_FAMILY,
        canonical: true,
    },
    ProviderCandidate {
        id: "csharp-ls",
        executables: &["csharp-ls"],
        args: &[],
        languages: CSHARP,
        canonical: true,
    },
    ProviderCandidate {
        id: "vscode-css-language-server",
        executables: &["vscode-css-language-server"],
        args: &["--stdio"],
        languages: CSS,
        canonical: true,
    },
    ProviderCandidate {
        id: "dart-language-server",
        executables: &["dart"],
        args: &["language-server", "--protocol=lsp"],
        languages: DART,
        canonical: true,
    },
    ProviderCandidate {
        id: "elixir-ls",
        executables: &["language_server.sh", "language_server", "elixir-ls"],
        args: &[],
        languages: ELIXIR,
        canonical: true,
    },
    ProviderCandidate {
        id: "gopls",
        executables: &["gopls"],
        args: &["serve"],
        languages: GO,
        canonical: true,
    },
    ProviderCandidate {
        id: "vscode-html-language-server",
        executables: &["vscode-html-language-server"],
        args: &["--stdio"],
        languages: HTML,
        canonical: true,
    },
    ProviderCandidate {
        id: "jdtls",
        executables: &["jdtls"],
        args: &[],
        languages: JAVA,
        canonical: true,
    },
    ProviderCandidate {
        id: "typescript-language-server",
        executables: &["typescript-language-server"],
        args: &["--stdio"],
        languages: JS_TS,
        canonical: true,
    },
    ProviderCandidate {
        id: "lua-language-server",
        executables: &["lua-language-server"],
        args: &[],
        languages: LUA,
        canonical: true,
    },
    ProviderCandidate {
        id: "ocamllsp",
        executables: &["ocamllsp"],
        args: &[],
        languages: OCAML,
        canonical: true,
    },
    ProviderCandidate {
        id: "phpactor",
        executables: &["phpactor"],
        args: &["language-server"],
        languages: PHP,
        canonical: true,
    },
    ProviderCandidate {
        id: "intelephense",
        executables: &["intelephense"],
        args: &["--stdio"],
        languages: PHP,
        canonical: false,
    },
    ProviderCandidate {
        id: "pyright",
        executables: &["pyright-langserver"],
        args: &["--stdio"],
        languages: PYTHON,
        canonical: true,
    },
    ProviderCandidate {
        id: "pylsp",
        executables: &["pylsp"],
        args: &[],
        languages: PYTHON,
        canonical: false,
    },
    ProviderCandidate {
        id: "r-languageserver",
        executables: &["R"],
        args: &["--no-echo", "-e", "languageserver::run()"],
        languages: R_LANG,
        canonical: true,
    },
    ProviderCandidate {
        id: "ruby-lsp",
        executables: &["ruby-lsp"],
        args: &[],
        languages: RUBY,
        canonical: true,
    },
    ProviderCandidate {
        id: "solargraph",
        executables: &["solargraph"],
        args: &["stdio"],
        languages: RUBY,
        canonical: false,
    },
    ProviderCandidate {
        id: "rust-analyzer",
        executables: &["rust-analyzer"],
        args: &[],
        languages: RUST,
        canonical: true,
    },
    ProviderCandidate {
        id: "sourcekit-lsp",
        executables: &["sourcekit-lsp"],
        args: &[],
        languages: SWIFT,
        canonical: true,
    },
];

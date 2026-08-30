use super::SemanticLanguage;

#[derive(Clone, Copy)]
pub(super) struct ProviderCandidate {
    pub(super) id: &'static str,
    pub(super) executable: &'static str,
    pub(super) args: &'static [&'static str],
    pub(super) languages: &'static [SemanticLanguage],
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

pub(super) const PROVIDERS: &[ProviderCandidate] = &[
    ProviderCandidate {
        id: "bash-language-server",
        executable: "bash-language-server",
        args: &["start"],
        languages: BASH,
    },
    ProviderCandidate {
        id: "clangd",
        executable: "clangd",
        args: &[],
        languages: C_FAMILY,
    },
    ProviderCandidate {
        id: "csharp-ls",
        executable: "csharp-ls",
        args: &[],
        languages: CSHARP,
    },
    ProviderCandidate {
        id: "omnisharp",
        executable: "OmniSharp",
        args: &["-lsp"],
        languages: CSHARP,
    },
    ProviderCandidate {
        id: "vscode-css-language-server",
        executable: "vscode-css-language-server",
        args: &["--stdio"],
        languages: CSS,
    },
    ProviderCandidate {
        id: "dart-language-server",
        executable: "dart",
        args: &["language-server", "--protocol=lsp"],
        languages: DART,
    },
    ProviderCandidate {
        id: "elixir-ls",
        executable: "elixir-ls",
        args: &[],
        languages: ELIXIR,
    },
    ProviderCandidate {
        id: "elixir-ls-script",
        executable: "language_server.sh",
        args: &[],
        languages: ELIXIR,
    },
    ProviderCandidate {
        id: "gopls",
        executable: "gopls",
        args: &[],
        languages: GO,
    },
    ProviderCandidate {
        id: "vscode-html-language-server",
        executable: "vscode-html-language-server",
        args: &["--stdio"],
        languages: HTML,
    },
    ProviderCandidate {
        id: "jdtls",
        executable: "jdtls",
        args: &[],
        languages: JAVA,
    },
    ProviderCandidate {
        id: "typescript-language-server",
        executable: "typescript-language-server",
        args: &["--stdio"],
        languages: JS_TS,
    },
    ProviderCandidate {
        id: "lua-language-server",
        executable: "lua-language-server",
        args: &[],
        languages: LUA,
    },
    ProviderCandidate {
        id: "ocamllsp",
        executable: "ocamllsp",
        args: &[],
        languages: OCAML,
    },
    ProviderCandidate {
        id: "phpactor",
        executable: "phpactor",
        args: &["language-server"],
        languages: PHP,
    },
    ProviderCandidate {
        id: "intelephense",
        executable: "intelephense",
        args: &["--stdio"],
        languages: PHP,
    },
    ProviderCandidate {
        id: "pyright",
        executable: "pyright-langserver",
        args: &["--stdio"],
        languages: PYTHON,
    },
    ProviderCandidate {
        id: "pylsp",
        executable: "pylsp",
        args: &[],
        languages: PYTHON,
    },
    ProviderCandidate {
        id: "r-languageserver",
        executable: "R",
        args: &["--slave", "-e", "languageserver::run()"],
        languages: R_LANG,
    },
    ProviderCandidate {
        id: "ruby-lsp",
        executable: "ruby-lsp",
        args: &[],
        languages: RUBY,
    },
    ProviderCandidate {
        id: "solargraph",
        executable: "solargraph",
        args: &["stdio"],
        languages: RUBY,
    },
    ProviderCandidate {
        id: "rust-analyzer",
        executable: "rust-analyzer",
        args: &[],
        languages: RUST,
    },
    ProviderCandidate {
        id: "sourcekit-lsp",
        executable: "sourcekit-lsp",
        args: &[],
        languages: SWIFT,
    },
];

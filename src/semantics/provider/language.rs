use super::SemanticLanguage;
use std::path::Path;

pub fn language_for_path(path: &str) -> Option<SemanticLanguage> {
    let path = Path::new(path);
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    let extension = path
        .extension()
        .map(|value| value.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match name.as_str() {
        ".bashrc" | ".bash_profile" | ".bash_login" | ".profile" | ".zshrc" | ".zprofile"
        | ".zshenv" | ".zlogin" => Some(SemanticLanguage::Bash),
        "gemfile" | "rakefile" | "guardfile" | "podfile" | "fastfile" | "appfile"
        | "deliverfile" | "brewfile" | "vagrantfile" => Some(SemanticLanguage::Ruby),
        _ => match extension.as_str() {
            "sh" | "bash" | "zsh" | "ksh" | "command" => Some(SemanticLanguage::Bash),
            "c" | "h" => Some(SemanticLanguage::C),
            "cc" | "cpp" | "cxx" | "c++" | "hh" | "hpp" | "hxx" | "h++" | "ipp" | "tpp" | "inl" => {
                Some(SemanticLanguage::Cpp)
            }
            "cs" | "cake" => Some(SemanticLanguage::CSharp),
            "css" => Some(SemanticLanguage::Css),
            "dart" => Some(SemanticLanguage::Dart),
            "ex" | "exs" => Some(SemanticLanguage::Elixir),
            "go" => Some(SemanticLanguage::Go),
            "html" | "htm" | "xhtml" => Some(SemanticLanguage::Html),
            "java" => Some(SemanticLanguage::Java),
            "js" | "jsx" | "mjs" | "cjs" => Some(SemanticLanguage::JavaScript),
            "lua" => Some(SemanticLanguage::Lua),
            "ml" => Some(SemanticLanguage::Ocaml),
            "mli" => Some(SemanticLanguage::OcamlInterface),
            "php" | "php3" | "php4" | "php5" | "phtml" => Some(SemanticLanguage::Php),
            "py" | "pyi" => Some(SemanticLanguage::Python),
            "r" => Some(SemanticLanguage::R),
            "rb" | "rake" | "gemspec" | "ru" | "jbuilder" => Some(SemanticLanguage::Ruby),
            "rs" => Some(SemanticLanguage::Rust),
            "swift" => Some(SemanticLanguage::Swift),
            "ts" | "mts" | "cts" => Some(SemanticLanguage::TypeScript),
            "tsx" => Some(SemanticLanguage::Tsx),
            _ => None,
        },
    }
}

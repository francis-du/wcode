use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct CommandResult {
    pub program: String,
    pub args: Vec<String>,
    pub exit_code: Option<i32>,
    pub success: bool,
    pub process_queue_wait_ms: u64,
    pub stdout: String,
    pub stderr: String,
    pub truncated: bool,
    pub redacted: bool,
    pub timed_out: bool,
    pub output_incomplete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_guidance: Option<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct FileView {
    pub path: String,
    pub sha256: String,
    pub start_line: usize,
    pub end_line: usize,
    pub total_lines: usize,
    pub content: String,
    pub redacted: bool,
}

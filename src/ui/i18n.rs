#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum UiLanguage {
    #[default]
    En,
    ZhCn,
}

impl UiLanguage {
    pub(super) fn toggle(self) -> Self {
        match self {
            Self::En => Self::ZhCn,
            Self::ZhCn => Self::En,
        }
    }

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::En => "English",
            Self::ZhCn => "简体中文",
        }
    }

    pub(super) fn tr(self, key: &'static str) -> &'static str {
        match (self, key) {
            (Self::ZhCn, "workspace") => "工作区",
            (Self::ZhCn, "add") => "添加",
            (Self::ZhCn, "auth") => "授权",
            (Self::ZhCn, "authorization") => "授权",
            (Self::ZhCn, "approve") => "批准",
            (Self::ZhCn, "deny") => "拒绝",
            (Self::ZhCn, "select authorization") => "选择授权请求",
            (Self::ZhCn, "select / approve / deny authorization") => "选择 / 批准 / 拒绝授权",
            (Self::ZhCn, "setup") => "连接设置",
            (Self::ZhCn, "web") => "Web 控制台",
            (Self::ZhCn, "project") => "项目",
            (Self::ZhCn, "author") => "作者",
            (Self::ZhCn, "help") => "帮助",
            (Self::ZhCn, "language") => "语言",
            (Self::ZhCn, "stop") => "停止",
            (Self::ZhCn, "move workspace") => "切换工作区",
            (Self::ZhCn, "move one page") => "切换一页工作区",
            (Self::ZhCn, "open Connector setup") => "打开连接设置",
            (Self::ZhCn, "open Project Observatory") => "打开 Project Observatory",
            (Self::ZhCn, "open project repository") => "打开项目仓库",
            (Self::ZhCn, "open author profile") => "打开作者主页",
            (Self::ZhCn, "toggle language") => "切换语言",
            (Self::ZhCn, "open or close help") => "打开或关闭帮助",
            (Self::ZhCn, "approve selected authorization") => "批准所选授权请求",
            (Self::ZhCn, "deny selected authorization") => "拒绝所选授权请求",
            (Self::ZhCn, "stop wcode") => "停止 wcode",
            (Self::ZhCn, "HELP & LINKS") => "帮助与链接",
            (Self::ZhCn, "ESC TO CLOSE") => "ESC 关闭",
            (Self::ZhCn, "SHORTCUTS") => "快捷键",
            (Self::ZhCn, "RUNTIME") => "运行状态",
            (Self::ZhCn, "Project") => "项目",
            (Self::ZhCn, "Author") => "作者",
            (Self::ZhCn, "Setup") => "设置",
            (Self::ZhCn, "Health") => "健康检查",
            (Self::ZhCn, "Terminal needs a little more room") => "终端窗口太小",
            (Self::ZhCn, "resize the window to restore the live dashboard") => {
                "调整窗口大小以恢复实时控制台"
            }
            (Self::ZhCn, "WORKSPACE") => "工作区",
            (Self::ZhCn, "LANGUAGE") => "语言",
            (Self::ZhCn, "Workspace path") => "工作区路径",
            (Self::ZhCn, "Enter absolute path · Enter to add · Esc to cancel") => {
                "输入绝对路径 · Enter 添加 · Esc 取消"
            }
            (Self::ZhCn, "workspace add cancelled") => "已取消添加工作区",
            (Self::ZhCn, "workspace path cannot be empty") => "工作区路径不能为空",
            (Self::ZhCn, "CLOUDFLARED PROCESS EXITED") => "CLOUDFLARED 进程已退出",
            (Self::ZhCn, "cloudflared is no longer running") => "cloudflared 已停止运行",
            (Self::ZhCn, "PUBLIC URL UNAVAILABLE") => "公网地址不可用",
            (Self::ZhCn, "MCP client idle") => "MCP 客户端空闲",
            (Self::ZhCn, "MCP client connected") => "MCP 客户端已连接",
            (Self::ZhCn, "OAuth authorized") => "OAuth 已授权",
            (Self::ZhCn, "waiting for MCP handshake") => "等待 MCP 握手",
            (Self::ZhCn, "Setup required") => "需要完成设置",
            (Self::ZhCn, "press O to open Connector setup") => "按 O 打开连接设置",
            (Self::ZhCn, "OVERVIEW") => "总览",
            (Self::ZhCn, "RUN") => "运行",
            (Self::ZhCn, "WAIT") => "等待",
            (Self::ZhCn, "DONE") => "完成",
            (Self::ZhCn, "FAIL") => "失败",
            (Self::ZhCn, "ACTIVE") => "运行中",
            (Self::ZhCn, "QUEUED") => "排队中",
            (Self::ZhCn, "COMPLETED") => "已完成",
            (Self::ZhCn, "FAILED") => "失败",
            (Self::ZhCn, "TOKEN ECONOMY · TOTAL") => "Token 节省 · 总计",
            (Self::ZhCn, "THROUGHPUT") => "吞吐量",
            (Self::ZhCn, "SETUP") => "连接设置",
            (Self::ZhCn, "Open this wcode setup page") => "打开 wcode 设置页面",
            (Self::ZhCn, "Add the MCP URL and choose OAuth") => "添加 MCP URL 并选择 OAuth",
            (Self::ZhCn, "Open this setup page · press O") => "打开设置页面 · 按 O",
            (Self::ZhCn, "Choose a compatible AI client") => "选择兼容的 AI 客户端",
            (Self::ZhCn, "Add MCP URL · Auth: OAuth") => "添加 MCP URL · 授权方式：OAuth",
            (Self::ZhCn, "WORKSPACE ACTIVITY") => "工作区活动",
            (Self::ZhCn, "No workspaces configured") => "未配置工作区",
            (Self::ZhCn, "restart wcode with one or more --workspace paths") => {
                "使用一个或多个 --workspace 路径启动 wcode"
            }
            (Self::ZhCn, "SOFTWARE INTELLIGENCE") => "软件智能",
            (Self::ZhCn, "AUTHORIZE WORKSPACE") => "添加工作区",
            (Self::ZhCn, "Type an absolute or relative project path…") => {
                "输入项目绝对或相对路径…"
            }
            (Self::ZhCn, "Enter authorize · Esc cancel · hard safety boundaries still apply") => {
                "Enter 添加 · Esc 取消 · 安全边界仍然生效"
            }
            (Self::ZhCn, "STATUS") => "状态",
            (Self::ZhCn, "AUTHORIZATION REQUIRED") => "需要授权",
            (Self::ZhCn, "PENDING") => "待处理",
            (Self::ZhCn, "COMMAND") => "命令",
            (Self::ZhCn, "RISKY EXEC") => "高风险执行",
            (Self::ZhCn, "RUNTIME EXEC") => "运行时执行器",
            (Self::ZhCn, "DELETE") => "删除",
            (Self::ZhCn, "select request") => "选择请求",
            (Self::ZhCn, "approve selected") => "批准所选",
            (Self::ZhCn, "deny selected") => "拒绝所选",
            (Self::ZhCn, "retry the tool") => "重试工具",
            (Self::ZhCn, "approved") => "已批准",
            (Self::ZhCn, "denied") => "已拒绝",
            (Self::ZhCn, "authorization is no longer pending") => "授权请求已不再待处理",
            (Self::ZhCn, "move workspace focus") => "切换工作区焦点",
            (Self::ZhCn, "move one workspace page") => "切换一页工作区",
            (Self::ZhCn, "ready") => "就绪",
            (Self::ZhCn, "blocked") => "阻塞",
            (Self::ZhCn, "unknown") => "未知",
            (Self::ZhCn, "converged") => "已收敛",
            (Self::ZhCn, "active") => "进行中",
            (Self::ZhCn, "close") => "关闭",
            (Self::ZhCn, "ROOT") => "根目录",
            (Self::ZhCn, "DESIGN") => "设计",
            (Self::ZhCn, "SCOPES") => "范围",
            (Self::ZhCn, "TRACE") => "追踪",
            (Self::ZhCn, "GRAPH") => "软件图",
            (Self::ZhCn, "GRAPH Δ") => "软件图 Δ",
            (Self::ZhCn, "SEMANTICS") => "语义",
            (Self::ZhCn, "RISK") => "风险",
            (Self::ZhCn, "EVIDENCE") => "证据",
            (Self::ZhCn, "VERIFY") => "验证",
            (Self::ZhCn, "RECONCILE") => "收敛",
            (Self::ZhCn, "UPDATED") => "更新时间",
            (Self::ZhCn, "Run intelligence tools to refresh live fields.") => {
                "运行 Intelligence 工具以刷新实时字段。"
            }
            _ => key,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_toggle_defaults_to_english_and_covers_chinese() {
        assert_eq!(UiLanguage::default(), UiLanguage::En);
        assert_eq!(UiLanguage::En.toggle(), UiLanguage::ZhCn);
        assert_eq!(UiLanguage::ZhCn.toggle(), UiLanguage::En);
        assert_eq!(UiLanguage::ZhCn.tr("workspace"), "工作区");
    }
}

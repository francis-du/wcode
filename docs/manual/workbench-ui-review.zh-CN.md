---
layout: docs
title: 工作台界面检查
nav_title: 界面检查
description: 浏览器与终端的实际渲染状态、示例图片和检查边界
lang: zh-CN
alternate: /docs/workbench-ui-review/
permalink: /zh/docs/workbench-ui-review/
---

# 工作台界面检查

七个页面都保留当前工作区、源码版本和快照状态。导航记住上次打开的有效页面；本地存储不可用或值无效时安全回到总览。页面标题和浏览器标题随语言、工作区变化。键盘用户可以直接跳到当前内容，也可以通过品牌链接回到变更验收页。

观测覆盖放在原生可展开区域内，手机上先显示验收结论、任务、源码或需求内容。源码精度、过期快照、失败结果和操作者权限保留原有含义。

| 界面 | 保留的主要状态 | 检查方式 |
| --- | --- | --- |
| 变更验收 | 阻塞、缺少检查、过期证据、按证据定位 | 生产 DOM 示例和原生测试 |
| 工程架构 | 系统与组件、依赖、代码关系、源码检查、全屏 | 生产 DOM 布局示例 |
| 任务活动 | 运行、排队、不可用、执行者归属、任务结果 | 生产渲染行为和布局示例 |
| 检查结果 | 当前失败、历史、不可用验证、证据选择 | 生产渲染行为和布局示例 |
| 当前变更 | 审查不可用、工作树/暂存区/HEAD 对比、源码转义 | 生产渲染行为和布局示例 |
| 需求 | 过滤和选择、追溯、影响 | 生产渲染行为和布局示例 |
| 项目文件 | 部分源码覆盖、可搜索文件树、大文件、源码导航 | 生产渲染行为和布局示例 |
| 公开设置页 | 本地连接、已确认或不可用端点、启动选项、复制成功/手动选择、命令或端点变化、隐藏页面后的复制完成 | 真实生产脚本的延迟复制回执测试和浏览器 DOM 检查 |
| TUI | 宽屏/紧凑面板、极小窗口恢复、帮助、授权、命令和工作区浮层 | 生产 `draw_dashboard` 的 22 个状态，以及面板行为测试 |

极小终端窗口为帮助和停止快捷键预留最后一行。提示显示当前尺寸和面板实际需要的最小高度，包括设置与隧道行。渲染器放在独立模块内，使主面板保持在源码大小边界之内。运行时分发和授权操作保留原有行为。

复制反馈对应真正复制的命令或端点。修改这些值或隐藏页面会使尚未完成的回执失效，旧剪贴板请求无法把新命令标成“已复制”，也无法把焦点移到过期的手动选择区域。重复点击只保留一个待完成请求；普通手动复制仍然可用。

本地检查使用生产 HTML/CSS/JavaScript 和明确的示例数据。Chrome 154 覆盖 140 个页面、语言、主题和窗口尺寸组合，以及标签页、跳过导航、返回主页等键盘操作。原生 WebKit 覆盖 256 个组合，包括架构子视图与源码检查。公开设置页覆盖 20 个浏览器组合，剪贴板写入被模拟，反馈与手动选择使用真实 DOM。完整生产 `draw_dashboard` 导出宽屏、紧凑、极小窗口、帮助、命令、项目详情、Full Access、工作区输入、命令授权、人工决策、操作反馈共 22 个中英文画面。紧凑渲染器另有 32 个终端尺寸与语言组合；旧实现无法通过相同的恢复快捷键负控。

这些示例检查界面展示和浏览器行为，不代表真实提供方、OAuth、隧道或后端验收已通过。完整原生集成检查和跨平台构建仍需通过 PR CI；独立紧凑渲染器的结果只是补充证据。

## 当前界面图片

图片来自生产浏览器代码和生产 Ratatui 渲染器，使用明确标注的测试示例数据。连接、任务、需求、审批请求均为演示。生成图片没有连接真实提供方，也没有授予权限或执行验收操作。

| 页面 | 桌面图片 |
| --- | --- |
| 变更验收 | [当前工作区与验收](/assets/wcode-overview.png) |
| 工程架构 | [架构与源码关系](/assets/wcode-architecture.png) |
| 任务活动 | [当前任务](/assets/wcode-task-activity.png) |
| 检查结果 | [验证与证据](/assets/wcode-verification-evidence.png) |
| 当前变更 | [变更审查](/assets/wcode-current-changes.png) |
| 需求 | [需求详情与追溯](/assets/wcode-requirements.png) |
| 项目文件 | [项目文件检查](/assets/wcode-project-files.png) |
| 访问 | [工作区与操作权限](/assets/wcode-access-management.png) |
| 设置 | [连接指引与命令反馈](/assets/wcode-setup-hub.png) |

![手机上的验收工作台，使用示例数据](/assets/wcode-overview-mobile.png)

![宽屏终端面板，使用示例数据](/assets/wcode-tui.png)

![极小终端的恢复指引，使用示例数据](/assets/tui/tiny-zh-CN.png)

## 终端状态检查

示例测试调用完整生产面板渲染器，保留单元格颜色和宽字符位置，将缓冲区导出为 SVG。Chrome 再将 SVG 转成以下图片。检查范围是渲染后的状态，不证明终端模拟器中的按键分发、运行时关闭或真实后端行为。现有原生行为测试保留这些单独的合同。CI 为每个平台上传 22 张原始 SVG 及其清单。

| 界面 | 英文 | 中文 |
| --- | --- | --- |
| 宽屏 | [画面](/assets/tui/wide-en.png) | [画面](/assets/tui/wide-zh-CN.png) |
| 紧凑 | [画面](/assets/tui/compact-en.png) | [画面](/assets/tui/compact-zh-CN.png) |
| 极小窗口 | [画面](/assets/tui/tiny-en.png) | [画面](/assets/tui/tiny-zh-CN.png) |
| 帮助 | [画面](/assets/tui/help-en.png) | [画面](/assets/tui/help-zh-CN.png) |
| 命令 | [画面](/assets/tui/commands-en.png) | [画面](/assets/tui/commands-zh-CN.png) |
| 项目详情 | [画面](/assets/tui/project-details-en.png) | [画面](/assets/tui/project-details-zh-CN.png) |
| Full Access | [画面](/assets/tui/full-access-en.png) | [画面](/assets/tui/full-access-zh-CN.png) |
| 工作区输入 | [画面](/assets/tui/workspace-input-en.png) | [画面](/assets/tui/workspace-input-zh-CN.png) |
| 命令授权 | [画面](/assets/tui/command-authorization-en.png) | [画面](/assets/tui/command-authorization-zh-CN.png) |
| 人工决策 | [画面](/assets/tui/human-decision-en.png) | [画面](/assets/tui/human-decision-zh-CN.png) |
| 操作反馈 | [画面](/assets/tui/operation-feedback-en.png) | [画面](/assets/tui/operation-feedback-zh-CN.png) |

执行 `cargo test --locked --lib production_dashboard_render_review_exports_all_operator_surfaces` 可以重新导出终端画面。浏览器示例使用 `node tests/unit/ui/browser.cjs .`，发布图片保留示例数据标记。

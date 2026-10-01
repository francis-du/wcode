---
layout: docs
title: 本地验收 Policy
description: 原生 Policy 捕获、操作者批准和有界本地权威历史
lang: zh-CN
alternate: /docs/acceptance-policy/
permalink: /zh/docs/acceptance-policy/
---

# 本地验收 Policy

这层 OSS 能力把仓库可编辑的 Policy 草案与操作者批准的本地基线分开，提供原生预览、状态、激活与撤销。当前工作树进一步实现了 Policy 消费、原生 Change Acceptance Record 和基础 GitHub adapter，正在完成接线与失败路径验证；这不代表已经发布、安装 required check 或完成收费 Pilot。参阅 [Change Acceptance](../change-acceptance/)。

## 草案与原生捕获

在 `.wcode/project.yaml` 中声明可选 `acceptance_policy`。Schema 1 包含稳定 ID、项目维护的版本、默认要求、可选 docs-only 要求及范围规则。要求只能引用发现出的检查，不能嵌入任意命令。

例如，原生发现提供 `node-test` 的 Node 项目可从以下草案开始：

```yaml
schema_version: 1
name: example
acceptance_policy:
  schema_version: 1
  id: project-baseline
  version: 1
  default:
    minimum_level: full
    checks: [node-test]
    human_approval: true
```

继续使用现有 Agent 与 MCP 连接，调用 `acceptance_policy`：

```json
{"action":"preview"}
```

预览捕获完整 Code／Design 版本、原生命令绑定，并对发现流程实际读取的同一份配置字节计算 SHA256。预期文件缺失会显式记录，二进制锁文件按字节计算。组件与需求冻结 Design 声明的普通文件；符号引用保守扩展为文件范围。这是声明的归属，不是语义覆盖证明。

至少需要一个显式发现出的检查和一个存在的配置来源。未知检查、发现不完整、无效 Design、无法无损表达的根目录、危险映射与超过快照上限都会拒绝。

即使命令 argv 不变，修改 package script 或 Makefile recipe 仍会改变定义摘要。普通 README 修改不进入定义摘要。它覆盖已捕获的原生配置输入，不递归认证所有可执行程序、依赖、build script 或测试体。

## 精确操作者批准

预览响应包含 `snapshot_digest`。状态提供当前 `generation`，没有记录时为 0；损坏的权威头返回错误，绝不退回 0。

用精确摘要和代号请求激活：

```json
{"action":"activate","expected_generation":0,"snapshot_digest":"sha256:<预览摘要>"}
```

可选 `expires_at_ms` 是绝对 Unix 毫秒时间，最多向后 365 天。服务器创建两分钟有效、只能消费一次的 HumanDecision，绑定实例、请求的 MCP Owner、Workspace／根、原生版本、动作、快照摘要、预期代号与到期时间。在既有 TUI 授权面板或受保护 WebUI Access 中检查并批准这条精确请求，然后由同一 MCP Owner 重试原请求。

普通 MCP Elicitation、OAuth Bearer、Full Access、命令会话授权、调用者标签和提交的 JSON 都不能建立这项批准。服务器消费授权后重新捕获原生输入，完整摘要一致才提交；输入变化或代号竞争会消费授权，但不激活陈旧输入。

Policy 批准产生治理记录，不会生成通过的验证或 HumanApproval Evidence。操作者凭据记录实例绑定的本地请求 ID 和决定时间，不代表经过认证的 Team 成员。

## 状态与撤销

`{"action":"status"}` 返回：

| 状态 | 含义 |
| --- | --- |
| inactive | 不存在本地权威记录。 |
| active | 当前权威未过期，捕获的原生定义仍然一致。 |
| stale_definition | 配置发现失败或必需定义变化；保留历史记录。 |
| expired | 激活有效期已经结束。 |
| revoked | 最新提交代号是撤销记录。 |

这里的 `active` 只表示 Policy 权威状态，不表示可合并。历史激活版本用于追溯；正常的后续代码变化不需要与它相等。候选改动里的草案不能替换已批准基线。

用 `{"action":"revoke","expected_generation":1}` 请求撤销。撤销需要独立的精确本地批准与版本保护。仓库草案缺失或无效，不会阻止撤销完整权威记录。本切片没有单独的回滚／续期产品命令。

## 持久化与安全边界

记录位于既有受保护本地工程状态根的 `acceptance-policy` 下，不存入仓库可编辑的 Design 文件。确定性的代号文件使用排他创建完成跨进程 CAS；文件和有界提交标记同步后才返回成功。最高代号未提交或损坏，会阻止读取与修改，绝不回退旧 active。历史上限 256 条，容量满明确失败，不静默删除撤销记录；坏头恢复还需要后续显式操作者流程。

SHA256 校验和及前代摘要链可以发现损坏和意外修改，不是签名，也不是法律意义不可篡改审计。Unix 同步文件与父目录；Windows 文件同步与父目录持久性边界不同。

文件工具保护实际配置的权威根；宽命令沙箱也遮罩这些根：macOS 拒绝读写，Linux 无法遮罩则拒绝启动，无可用后端继续拒绝。普通有界 Cargo／npm 构建和测试仍以宿主用户运行仓库代码。恶意仓库进程若能访问该用户的状态目录，仍可伪造带校验和的文件；这份本地历史不构成对任意仓库代码的认证验收权威。

可信 merge gate 必须从不可信变更 Worker 之外取得 Policy，并通过独立受保护的集成核验 commit 绑定的执行。原生 Policy 消费与 CAR 实现的验证进度见 [Change Acceptance](../change-acceptance/)；受保护部署与真实 PR Pilot 仍待验收。见[安全模型](../security/)与[OSS／Commercial 边界](../oss-boundary/)。

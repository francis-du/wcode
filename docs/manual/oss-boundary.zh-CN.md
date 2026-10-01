---
layout: docs
title: OSS 与商业模块边界
description: wcode OSS 与商业层之间的源码仓、许可证、合同和权威边界
lang: zh-CN
alternate: /docs/oss-boundary/
permalink: /zh/docs/oss-boundary/
---

# OSS 与商业模块边界

wcode 是公开的 Apache-2.0 产品。规范工程事实——Design、Verification、Evidence、Policy 语义、Acceptance、Workspace 安全、MCP/CLI/TUI/WebUI 以及可复用 Provider 合同——继续留在这个公开仓库。

Team 与 Enterprise 实现已经迁到独立源码仓库。本地开发可以把它放在兄弟目录 `../wcode-commercial`，但公开 wcode 的构建、测试、打包、启动和验证都不能要求该目录存在。

## 物理仓库边界

```text
Code/Rust/
├── wcode/                 # 公开，Apache-2.0
│   ├── crates/core-types/
│   ├── src/
│   ├── tests/
│   └── docs/
└── wcode-commercial/      # 独立、面向私有商业开发
    ├── team/
    ├── enterprise/
    ├── docs/
    └── Cargo.toml
```

依赖只能单向：

```text
Enterprise -> Team -> public wcode API -> wcode-core-types
```

公开仓里不再通过 `commercial/` Cargo member、workspace exclude 或 archive 规则“藏”商业源码。现在源码控制边界本身就是边界。OSS package 继续使用显式 include allowlist；architecture test 会拒绝任何离开公开 checkout 的本地 path dependency，以及名为 `wcode-team` / `wcode-enterprise` 的反向依赖。

## 哪些能力必须继续开源

公开 wcode 本身必须完整可用，不能为了收费故意挖空本地产品。

- 规范 Design / Requirement / Component 状态；
- Software Graph、Semantic Provider 和仓库理解；
- Verification、Evidence、Risk、Reconciliation；
- 本地 Acceptance Policy 与原生 Acceptance Record；
- Workspace 隔离、受控编辑/执行与授权；
- MCP transport、OAuth、Tasks 和 Agent/Plugin 集成；
- CLI、TUI、本地 WebUI / Observatory；
- 通用 GitHub 精确候选 gate / inbox / credential 协议；
- `wcode-core-types` 中的不可变共享合同。

商业层可以围绕这些能力增加组织、部署和客户流程，但不能另造一套工程事实。

## 哪些能力属于商业仓

独立商业仓负责源码访问、部署信任或产品生命周期明显不同的能力，例如组织/成员治理、共享 Policy 分发、Team audit/history、Enterprise 组合、SSO/OIDC、tenant isolation、Hosted control plane 运维、商业恢复流程和面向客户的部署集成。

商业 wrapper 可以增加授权和工作流，但不能把 failed/stale/skipped/incomplete 的 OSS verification 改成 Ready，不能用导入数据伪造 Native Authority，也不能复制一套规范模型形成双重真相。

## 公开合同规则

商业代码只能消费公开 Rust 合同或明确发布/固定版本的 package 接口。不能因为两个仓本地相邻，就通过 path attribute、`include!`、symlink、生成源码复制或重复 store schema 去偷读 `wcode/src` 私有实现。

本地开发当前使用兄弟目录 path dependency；商业 CI / release 必须绑定精确 wcode revision 或明确兼容版本。本地 `../wcode` 只是开发便利，不是正式发布的 provenance 合同。

第一个物理 OSS 抽取 `wcode-core-types` 只拥有轻依赖不可变事实，例如 Git binding、required-check/report DTO 和平台 authority/intelligence state-root 推导。后续是否继续抽 crate，遵循[架构与模块边界](../architecture-boundaries/)：先有稳定 owner、可独立测试合同和单向依赖，再做物理抽取。

## 打包与 CI

OSS workflow 只验证 OSS，不再探测或条件构建商业源码。即使机器上完全没有商业仓，公开 source package 与 standalone verifier 也必须成功。

商业 CI 独立负责选择精确 wcode revision、用公开合同构建 Team/Enterprise、执行等价/安全/运维测试、证明商业层不能升级规范 verification truth，并为每个商业版本记录对应的 wcode revision。

本地可以同时验证两个仓，但 commercial build 变绿不代表某次 OSS verification 已通过，反过来也一样。

## 许可证边界

既有 wcode 源码继续使用 Apache-2.0。把商业文件迁出公开仓不会重新许可任何 OSS 文件。独立商业仓拥有自己的 LICENSE/NOTICE，最终商业条款仍需相应法律审查。

公开仓也不能因为 package tooling 不会打包某文件，就保存客户机密数据、私有商业实现、生产凭据或部署状态。

## 状态与权威边界

源码拆仓与运行隔离是两件事。兄弟目录本身不证明 tenant isolation。`WCODE_STATE_DIR`、Verification/Evidence store、Publisher credential 和 Team governance storage 仍需要受保护部署身份及真实文件/进程隔离。

当前 Acceptance 必须继续来自与 Revision 绑定的 OSS 原生状态。商业历史、CI 机器报告、exception 和组织角色都属于独立事实，除非公开 authority 合同明确赋予它们权威，否则不能替代原生事实。

## 完成门槛

只有公开 wcode 在没有商业 checkout 时仍可 build/test/package、OSS Cargo metadata 不存在商业依赖路径、OSS source include 不能逃出公开仓、商业消费者只使用明确公开 API、商业测试绑定精确 wcode revision，并且 release/docs/CI 不再暗示私有源码位于 OSS 仓中，才能认为边界健康。

历史上的同仓 `commercial/` 结构只属于迁移历史，不再是当前架构。

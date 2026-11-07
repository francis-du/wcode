---
layout: docs
title: 研究论文
description: Francis Du 关于有状态软件智能体与修订绑定证据的预印本。
author: Francis Du
lang: zh-CN
alternate: /docs/paper/
permalink: /zh/docs/paper/
---

# WCode：面向有状态软件智能体的修订绑定证据

**Francis Du** · **2026 年 11 月 6 日** · 预印本

WCode 将证据视为绑定到对象、生产者、权威性、覆盖范围、策略以及代码与设计修订的观察。论文给出证据准入契约，研究运行时如何通过这些区别连接有界源码交付、受保护编辑、验证复用与区分生产者的协调核对。

## 阅读论文

[英文 PDF — 10 页](/paper/2026-11-06/wcode-preprint.en.pdf) · [中文 PDF — 8 页](/paper/2026-11-06/wcode-preprint.zh-CN.pdf) · [LaTeX 源码包](/paper/2026-11-06/wcode-arxiv-source.zip)

<p><a href="/paper/2026-11-06/wcode-preprint.zh-CN.pdf" target="_blank" rel="noopener">在新标签页中打开中文 PDF</a> · <a href="/paper/2026-11-06/wcode-preprint.zh-CN.pdf" download>下载中文 PDF</a></p>

<object data="/paper/2026-11-06/wcode-preprint.zh-CN.pdf" type="application/pdf" aria-label="Francis Du 的 WCode 论文中文版" style="display:block;width:100%;height:75vh;min-height:420px">
  <p>此浏览器无法显示内嵌 PDF 阅读器。请<a href="/paper/2026-11-06/wcode-preprint.zh-CN.pdf">打开或下载中文 PDF</a>。</p>
</object>

## 研究与适用范围

无模型诊断覆盖开发者编写的 60 个合成场景、四种语言、三档响应预算、两个执行框架缓存阶段和七次重复，共得到 2,520 次实测调用。22 项选定的契约反例在同一保留可执行文件上逐项重放；这 22 项测试与十一项运行控制检查均通过。

结果区分了源码交付、编辑输入就绪状态与结构化决策记录的可见性。所需源码规模较小，重复运行使用同一组夹具。研究结果支持本次测试覆盖的运行时契约。独立真实仓库的问题修复、匹配的智能体比较和操作者研究仍有待后续评估。

## 源码与引用

[按版本保存的中英文 LaTeX 源码](https://github.com/francis-du/wcode/tree/main/docs/paper/2026-11-06/source)支持重新构建论文。另见[构建与工件说明](/paper/2026-11-06/README.md)、[BibTeX 引用](/paper/2026-11-06/citation.bib)和 [SHA-256 校验值](/paper/2026-11-06/SHA256SUMS)。

Francis Du. 2026. *WCode: Revision-Bound Evidence for Stateful Software Agents*. 预印本，2026 年 11 月 6 日。

[早期稿件](/paper/README.md)记录了较早的 360 次查询诊断快照。

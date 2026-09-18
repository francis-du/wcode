---
layout: docs
title: 技术论文
description: wcode 系统论文初稿、英文正文、中文原稿与实验依据。
lang: zh-CN
alternate: /docs/paper/
permalink: /zh/docs/paper/
---

# 技术论文

## 论文正文

**wcode：面向编码智能体的证据驱动工程控制平面与无模型适应度评测**

阅读[英文正文](/paper/paper.en.md)或[中文原稿](/paper/paper.zh-CN.md)。英文修订版 2 补充了明确的指标定义、具体的证据交付失败示例，以及更精确的基线与消融计划。实验结果沿用原始快照，不冒充一次新评测。

这是技术初稿，不是已通过同行评审或已被录用的论文。作者信息与投稿格式仍待最终确定。

## 实验与边界

原始研究包含 60 个合成开发场景、三个预算、两个缓存阶段、360 次测量查询，以及另外 180 次预热。预算单位是序列化 JSON 字节数除以四的估算，不是模型 token 计费量。证据交付与编辑输入完整不等于自主发现缺陷或生成正确补丁。

参见[英文证据记录](/paper/evidence.en.md)、[实验摘录](/paper/snapshot.json)、[参考文献](/paper/references.bib)和[修订记录](/paper/revision.en.md)。实验摘录不等于完整原始报告，也不是已冻结的源码制品。

## 复现方式

在仓库根目录运行只读的论文一致性检查：

```sh
python3 tests/paper_artifacts.py
```

检查使用 `snapshot.json` 指定的原始报告，或存在时使用字节一致的归档；它核对已有数据与论文，不运行新的 Fitness 实验。构建要求和原始实验命令见[论文目录说明](/paper/README.md)。

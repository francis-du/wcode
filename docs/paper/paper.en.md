# wcode: An Evidence-Driven Engineering Control Plane and Model-Free Fitness Evaluation for Coding Agents

*Technical paper draft · English revision 2 · September 18, 2026*

## Abstract

The engineering performance of coding agents depends not only on the language model but also on whether their tools deliver accurate, complete, revision-consistent, and actionable repository evidence. End-to-end task success, however, does not isolate failures in retrieval, context packing, edit preconditions, or verification infrastructure. Symbol-level recall alone can also overstate the availability of usable edit inputs. We present wcode, a runtime that organizes repository understanding, guarded modification, revision-bound verification, and engineering evidence into a unified workflow. We investigate how the deterministic properties of this runtime can be evaluated without invoking a model API. Our Engineering Fitness protocol separately measures path-qualified identity recall, complete source delivery, current file hashes, all-required edit inputs, budget compliance, and selected safety controls, rather than combining them into a model-capability score. A diagnostic experiment covers 60 synthetic development scenarios in Rust, Go, TypeScript, and Python across three budgets and two cache phases, yielding 360 measured queries with one observation per experimental cell. At the 2K and 4K budgets, required identity recall, complete source recall, and all-required edit-input coverage for eligible tasks are each 100%. At 1K, they are 86.73%, 68.37%, and 48.28%, respectively. In the natural-language subset, identity recall reaches 100% while complete source recall remains 50%, demonstrating that successful localization does not imply successful evidence delivery. The experiment does not measure autonomous bug discovery, generated-patch correctness, or generalization to real repositories. The results show how separately evaluating evidence identity, content completeness, and operational preconditions exposes engineering gaps obscured by a single retrieval metric. [S1–S7]

**Keywords:** coding agents; repository context; evidence-driven runtime; guarded editing; deterministic evaluation; Engineering Fitness

## 1 Introduction

Software engineering agents must locate implementations, inspect relationships, edit files, and execute checks within repositories. SWE-bench evaluates issue resolution in real repositories; SWE-agent investigates interfaces designed for agents interacting with computers; and Agentless separates localization, repair, and patch validation into distinct stages. [1–3] These approaches motivate studying software task outcomes in conjunction with the tools and interaction workflows that produce them.

This paper asks a narrower question that can be examined independently of patch generation: **How can we determine whether a coding runtime has delivered authentic, complete evidence that supports the next engineering operation?** Returning the correct function name does not establish that its body has been delivered. Delivering the body does not establish that its file hash is current. Having writable files does not establish that a subsequent edit has been verified. Collapsing these conditions into a single “found” or “ready” flag obscures important diagnostic information.

wcode treats the model as a replaceable caller while retaining repository state, operational boundaries, verification evidence, and observability in a separate runtime. [S1] We do not introduce a new code language model or propose replacing end-to-end benchmarks. Instead, we describe an implemented system and its tool-level evaluation protocol through three research questions. **RQ1:** Under a fixed budget, do symbol localization, complete source delivery, and edit-input completeness coincide? **RQ2:** How do budget and cache phase affect these quality dimensions and observable cost? **RQ3:** Do the evaluator and selected operational boundaries withstand targeted counterexample tests?

Our contributions are a systematic description of an evidence-driven engineering control plane; a model-free measurement protocol that distinguishes identity, source content, and edit preconditions; and a diagnostic experiment with corpus, evaluator, executable, and runtime-observed source fingerprints. These contributions concern an implemented system and a development-time measurement method. We make neither a priority claim that this is the first such system nor a superiority claim over other coding products.

## 2 Background and Research Positioning

### 2.1 Repository retrieval and engineering context

RepoCoder studies iterative retrieval and generation for repository-level code completion. Repoformer investigates selective retrieval, recognizing that additional context does not invariably improve generation. [4,5] We share their interest in context selection but evaluate a different object: whether the delivered context contains required source evidence and engineering preconditions, rather than whether a completion is correct. A shorter response is a candidate improvement only when the relevant evidence remains available.

### 2.2 Protocol interfaces and runtime responsibilities

The Model Context Protocol (MCP) standardizes connections between model applications and external context or tools. Its interface contract is not, by itself, a guarantee of correctness, safety, or evidence completeness for a particular repository implementation. [6] wcode exposes engineering capabilities through MCP and implements workspace boundaries, hash preconditions, and verification workflows in its runtime. [S1] We cite the protocol to clarify this division of responsibility, not to claim that wcode has passed conformance tests for every protocol version or transport behavior.

### 2.3 The scope of model-free evaluation

End-to-end evaluation asks whether a model–tool combination solves a task. Tool-level evaluation asks whether a runtime supplies specified engineering evidence and respects operational constraints. The former cannot be inferred from the latter, and an isolated model success does not establish the latter. Research on evaluation leakage and task quality also motivates scrutinizing benchmarks and their graders. [7] We therefore keep Gold annotations on the evaluator side, outside each measured workspace. This separation is not an independent holdout and does not eliminate overfitting caused by repeated developer exposure to the corpus.

## 3 Problem Formulation

Let $R$ denote repository state, $P$ the workspace policy, $q$ a task request, and $B$ the requested budget. The runtime returns an evidence package $O$. For discussion, we decompose $O$ into symbol identities $I$, source bodies $C$, file metadata $F$, relationship evidence $G$, and verification or readiness information $V$. This abstraction does not require every response to contain every field explicitly.

A source-evidence item should identify its path and qualified symbol, original content, file SHA-256, covered line range, and any truncation or redaction. Syntax-derived relationships, live language-server results, design declarations, and historical co-change heuristics have different evidential strength and must not be presented as interchangeable. [S1,S3]

We distinguish three conditions. **Localization** means observing the correct path-qualified symbol. **Source delivery** means receiving the complete annotated fragment with content, line bounds, and hash that can be checked against the original source. **Edit-input completeness** means that every required identity, source fragment, current hash, and write condition is present. The last condition still does not establish patch correctness or successful post-edit verification. [S5]

The implementation uses an engineering proxy for response size:

$$
\widehat{T}(O)=\left\lceil\frac{|\operatorname{JSON}(O)|_{\mathrm{bytes}}}{4}\right\rceil.
$$

Accordingly, 1K, 2K, and 4K denote budgets of 1,000, 2,000, and 4,000 proxy units. They are neither counts from a model-specific tokenizer nor provider-reported billing usage. [S2,S6]

## 4 System Design

### 4.1 Separating understanding, modification, and verification

wcode organizes engineering work around understanding, modification, verification, and evidence retention. Operator interfaces expose state and authorization requests. Desired design state, actual source, syntax indexes, available semantic providers, and historical evidence inform task context; guarded file operations perform modifications; and the verification layer produces results bound to repository revisions. [S1,S3,S8]

| Layer | Primary responsibility | Does not establish |
|:---|:---|:---|
| Repository understanding | Combine design, source, symbols, relationships, and risk | Correct model understanding of the task |
| Context delivery | Supply evidence and preconditions within a budget | Completeness of all potentially relevant information |
| Guarded modification | Check boundaries, write permissions, and hashes | Semantic correctness of an edit |
| Verification and evidence | Record execution, failures, skips, and revisions | Permanent validity of a past pass |
| Operator observability | Expose engineering state and authorization controls | Proof merely by displaying a status |

This separation does not require a language model to retain the runtime’s entire history. The model proposes actions; the runtime evaluates whether the observable state permits those actions and what evidence supports their outcomes.

### 4.2 Task-oriented, bounded context

`agent_context` first loads project configuration, then performs task-context resolution and repository-convention discovery concurrently. It assembles target symbols, a repository map (RepoMap), source bodies, file metadata, and verification entry points. Source delivery prioritizes explicit targets and executable definitions. For suitable budgets and relationship-oriented requests, it performs bounded expansion to related bodies that have not yet been delivered. Graph-based ranking and direct source loading can also proceed concurrently. [S3]

The final response is constrained by serialized size. Budget reduction must update readiness and truncation state, not merely remove content. Source truncation retains an original UTF-8 prefix and the line bounds actually delivered rather than inserting explanatory ellipses into copyable code. The objective is not to return as many symbols as possible, but to maintain consistency between source evidence and operational preconditions under a fixed budget. [S3]

This is a bounded candidate-selection, ranking, and packing strategy, not a proof of optimal context selection. The evaluation observes final delivery: a missing output identity does not establish that the internal retrieval stage never discovered it.

### 4.3 Precision and operational boundaries

wcode explicitly distinguishes Tree-sitter syntax evidence from Language Server Protocol (LSP) semantic capabilities. An unavailable semantic provider must not be represented as completed semantic analysis. File modification is constrained by workspace scope and current SHA preconditions; commands use argument arrays rather than concatenated shell strings. [S1,S3]

The controls executed in this study cover only part of that boundary: rejection of out-of-workspace access, read-only edits, stale-hash edits, and competing writes based on the same observed hash, for which only one may commit. [S4] Passing these tests provides implementation evidence under their specific conditions, not a security proof for the entire execution environment. The tests do not cover every malicious dependency, process behavior, or network attack.

### 4.4 Revision-bound verification and historical experience

The verification executor records the revision and observable change set to be checked, executes checks in phases, and distinguishes failed, skipped, and reused results. Its pass condition requires at least one check, no failed checks, and no outstanding skipped checks. Cached results are identified as reuse of static evidence for an exact revision. [S8]

A historical-experience module uses verified co-change paths and context trajectories as weak hints for later retrieval, rather than training a new language model. The implementation includes bounds on stored records, age-sensitive behavior, and activation gates. [S8] We describe it as a system component but do not independently ablate it or measure its cross-task benefit in this experiment.

## 5 The Engineering Fitness Protocol

### 5.1 Corpus, Gold evidence, and experimental units

The development corpus contains 60 synthetic scenarios. Each of four languages contributes 11 scenarios from a shared lifecycle-behavior family, covering explicit symbols, natural-language requests, single targets, call chains, and impact queries. Additional Rust scenarios cover same-name path disambiguation, four explicit targets, Unicode and CRLF source, missing anchors, read-only access, ten ownership-check mutations, and a scan-pressure fixture with 640 distractor files. [S2,S4]

Gold annotations specify required evidence by path and qualified symbol and define fragments authored from the fixture source. Instantiation writes only the measured repository files into a temporary workspace; Gold objects remain in the evaluator. Tasks may also annotate evidence as useful but not required. This is a development corpus, not a hidden test set: the 60 scenarios are not 60 independent real projects, and their translations across languages are not independent task families. [S4]

An experimental cell is a scenario–budget–cache-phase combination. This diagnostic run takes one observation per cell, yielding $60\times3\times2=360$ measured queries. Each warm cell receives one separately recorded identical-query warmup, for 180 additional calls that are not included in the 360 measurements. Every budget–phase group contains 98 required evidence instances, 59 answerable tasks, and 58 tasks that are writable and have required evidence. Read-only and no-answer scenarios are excluded from the all-required edit-input denominator. [S6,S7]

### 5.2 Three core quality metrics

**Required Identity Recall** is the total number of delivered required identities divided by the total number required across attempts. An identity is a path–qualified-symbol pair and is deduplicated within a response. A matching name at the wrong path receives no credit. [S5]

**Complete Source Recall** uses the same denominator but credits an item only when its delivered content matches the original bytes, line bounds, file hash, and explicit non-redacted status and contains the complete Gold fragment. A symbol-table entry or a self-reported “body present” flag cannot substitute for the source. [S5]

**All-Required Edit-Input Rate** uses eligible writable task attempts as its denominator. A task succeeds only when every required identity, complete source fragment, current SHA, workspace write permission, and non-read-only file condition is present. The evaluator computes this result independently and retains the tool’s own `readiness` report separately. Tool self-report is therefore not treated as proof of input completeness. [S5]

For clarity, let $G_i$ be the required identities for attempt $i$, $D_i$ its delivered identities, and $c_i(g)$ indicate validated complete delivery of the Gold source for identity $g$. The two evidence-level aggregates are

$$
R_{\mathrm{id}}=\frac{\sum_i |G_i\cap D_i|}{\sum_i|G_i|},
\qquad
R_{\mathrm{src}}=\frac{\sum_i\sum_{g\in G_i}c_i(g)}{\sum_i|G_i|}.
$$

Let $\mathcal{A}$ contain the attempts whose fixtures are writable and have nonempty required evidence. Let $h_i(g)$ indicate a current file hash and $w_i(g)$ indicate both workspace write permission and a non-read-only file. The task-level decision and aggregate rate are

$$
e_i=\mathbf{1}\!\left[G_i\subseteq D_i\right]\prod_{g\in G_i}c_i(g)h_i(g)w_i(g),
\qquad
R_{\mathrm{edit}}=\frac{\sum_{i\in\mathcal{A}}e_i}{|\mathcal{A}|}.
$$

Eligibility is determined by the fixture, not by a tool's reported readiness. On a failed attempt, delivered evidence and the indicators are zero; the attempt remains in every applicable denominator. A zero denominator is undefined. At the task level, edit-input completeness implies complete required identities and source, but the differently weighted aggregate rates cannot be ordered as if they shared a denominator. These equations restate the existing scorer rather than introduce a new measurement. [S5,S6]

Here, “complete source” means complete coverage of the authored Gold fragment with verified original bytes. It does not establish completeness of the surrounding file, an arbitrary semantic unit, or every dependency needed for a real repair. A SHA match establishes byte identity relative to the fixture; it is not a proof of program correctness.

### 5.3 Ranking, additional symbols, and delivery diagnostics

Ranking metrics use deduplicated delivery order: targets first, RepoMap entries second, and hot source last. NDCG@10 assigns gain 3 to required evidence, gain 1 to useful evidence, and gain 0 to other identities. It is not a direct measurement of internal candidate ranking. [S5]

The **Non-Gold Symbol Fraction** measures the fraction of returned identities outside the required and useful annotations. The reported aggregate is the mean of per-response fractions over nonempty identity responses, not a pooled fraction over all returned symbols. Empty responses have an undefined fraction and are excluded from that mean, whereas failed answerable attempts remain in recall and NDCG denominators. This conditional denominator is why low Non-Gold fraction alone is not evidence of good retrieval. Unannotated tests or transitive relationships may still be useful, so the metric is not an irrelevant-token fraction. **Complete Gold Byte Density** divides the union of original bytes in validated complete Gold fragments by serialized response bytes. Duplicate and overlapping spans within a path are counted only once. [S5,S6]

Diagnostics separately record missing identities, identified symbols without complete source, missing current hashes, and unavailable write inputs. These dimensions may overlap and must not simply be added together as a defect count. Receiving a current hash for a file does not imply that every required symbol within that file has been delivered.

### 5.4 Failure accounting, undefined quantities, and budget discipline

Failed answerable queries remain in recall and ranking denominators, with zero hits and zero ranking gain. Undefined quantities are reported as `null` or N/A, not as 100%. The experiment owns the requested budget: the evaluator checks serialized size and rejects a response that silently raises its reported budget. Failed queries are not silently retried with a larger budget. [S2,S6]

The union of required raw-source bytes supplies only a necessary feasibility condition. If it already exceeds $4B$, the uncompressed source cannot fit within the response under the measured representation. If it does not exceed $4B$, feasibility remains unknown because JSON structure, file metadata, and policy information also consume space. Small raw-source size is therefore not proof that the complete response can fit. [S6]

### 5.5 Counterexample-driven checks

Evaluator counterexamples cover matching names at incorrect paths, duplicate results, empty denominators, stale or missing hashes, redaction, fabricated and incomplete source, incorrect line bounds, Unicode and CRLF fidelity, and failed-query accounting. Runtime counterexamples additionally require preserving nine genuine callers rather than truncating them to six, excluding disconnected filler despite nonzero node degree, and retaining previously delivered Gold bodies after cache warmup. [S2,S4–S6]

These checks constrain regressions and grader defects but are not hidden evaluation tasks. Both the main corpus and the counterexamples inform development; their pass rates are not generalization scores on an external distribution.

## 6 Experimental Setup and Results

### 6.1 Environment and evidence scope

All quantitative results in this paper come from one canonical diagnostic report generated on September 18, 2026, at 02:10:00 JST. The recorded environment is macOS on aarch64, Rust 1.98.1, a debug build, four Harness slots, and reported available parallelism of ten. Runtime-observed source snapshots match before and after the diagnostic run, but the working tree contains uncommitted changes. Consequently, package version 0.7.5 and Git HEAD alone do not uniquely identify all experimental inputs. Exact fingerprints are retained in the accompanying evidence record. [S7]

Cold means constructing a new ToolHarness, without clearing operating-system filesystem caches or process-global language configuration. Warm means reusing the same Harness after an identical-query warmup. Timing covers `agent_context` only; it excludes compilation, temporary-repository creation, Harness construction, grading, and report writing. The seven-repeat release-profile trial was not executed for this study. [S2,S6,S7]

During preparation of the original study, the focused test command recorded 40 passed, zero failed, and two ignored tests. A subsequent explicit diagnostic-snapshot command recorded one passed test. These commands ran against a changing working tree and are not combined into acceptance evidence for a single frozen binary or the whole project. English revision 2 retains that exact diagnostic snapshot. Editorial changes and document-consistency checks are not new Fitness runs and do not update the measured runtime or its experimental fingerprints. [S7]

### 6.2 RQ1: Localization and actionable evidence diverge

**Table 1. Core quality results.** Each row aggregates one observation for each of 60 scenarios. Cold and warm phases are reported separately.

| Budget | Phase | Required identities | Complete source | All edit inputs | Query errors |
|---:|:---|---:|---:|---:|---:|
| 1K | cold | 85/98 (86.73%) | 67/98 (68.37%) | 28/58 (48.28%) | 0 |
| 1K | warm | 85/98 (86.73%) | 67/98 (68.37%) | 28/58 (48.28%) | 0 |
| 2K | cold | 98/98 (100%) | 98/98 (100%) | 58/58 (100%) | 0 |
| 2K | warm | 98/98 (100%) | 98/98 (100%) | 58/58 (100%) | 0 |
| 4K | cold | 98/98 (100%) | 98/98 (100%) | 58/58 (100%) | 0 |
| 4K | warm | 98/98 (100%) | 98/98 (100%) | 58/58 (100%) | 0 |

Source: the canonical diagnostic snapshot. [S7] Every 100% value is restricted to these synthetic scenarios and the stated denominator.

At 1K, each cache phase omits 13 required identities and delivers another 18 identified symbols without complete source. Current-hash recall is 97/98. Despite the availability of most file hashes, only 28 eligible tasks receive every required edit input. Identity and file metadata are not substitutes for source delivery. [S7]

The category breakdown exposes failure modes hidden by the overall average. Across 12 natural-language scenarios at 1K, identity recall is 100%, complete source recall is 50%, and the all-required edit-input rate is 33.3%. The corresponding values are 100%, 50%, and 0% for four impact queries, and 75%, 50%, and 0% for eight relationship queries. Declaring the context sufficient for editing merely because the correct symbols were found would therefore be premature. [S7]

A concrete example is `rust-refresh-natural`, whose annotated target is `refresh_session` in `src/session.rs`. At 1K, the recorded diagnostics list that identity as present and its current file hash as available, but its complete source as missing. The required raw fragment is 93 bytes. This is an observed delivery failure, not a claim that the symbol was absent from the internal index. It illustrates why additional source retrieval can still be necessary after localization succeeds. [S7]

### 6.3 RQ2: Budget, coverage, and additional content

At 2K, all identities and complete source fragments required by the current Gold annotations are delivered. Increasing the budget to 4K does not improve any of the three core quality metrics. Mean cold-response size nevertheless rises from 7,330.12 bytes at 2K to 12,045.27 bytes at 4K. Thus, within this snapshot, 2K delivers approximately 39.15% fewer serialized bytes than 4K while retaining the same required-evidence coverage. [S7]

This does not establish a universally optimal budget. The corpus contains short functions and a limited task family, and the possible downstream value of additional context is unmeasured. The Non-Gold Symbol Fraction increases from 11.02% at 1K to 20.28% at 2K and 20.88% at 4K; those identities cannot automatically be classified as useless. Increasing the response budget is not the same as increasing delivery of required evidence. [S7]

**Table 2. Descriptive cost measurements.** Latencies are in milliseconds. Quantiles pool one measurement from each of 60 different scenarios, rather than repeated measurements of one request.

| Budget | Phase | Mean response bytes | p50 (ms) | p95 (ms) | Over budget |
|---:|:---|---:|---:|---:|---:|
| 1K | cold | 3,753.87 | 22.411 | 36.513 | 0 |
| 1K | warm | 3,751.72 | 13.238 | 23.617 | 0 |
| 2K | cold | 7,330.12 | 18.627 | 34.209 | 0 |
| 2K | warm | 7,327.20 | 10.505 | 13.608 | 0 |
| 4K | cold | 12,045.27 | 14.568 | 29.280 | 0 |
| 4K | warm | 12,042.33 | 6.656 | 8.217 | 0 |

Source: the same canonical snapshot. [S7] Shared-machine load and first-use effects were not isolated. These observations do not establish a statistically validated cache speedup, and the lower observed latencies in some 4K cells do not imply that larger budgets are intrinsically faster.

The principal quality metrics match between cold and warm phases, and no warmup errors are recorded. No aggregate quality loss after warming was observed in this run. Stronger evidence of per-item preservation comes from the dedicated counterexample checks, not merely from equality of aggregate scores. [S4,S7]

### 6.4 RQ3: Operational controls and bug-relevant evidence

The nine executed controls comprise one two-edge syntax call graph for each language, plus guarded editing and invalidation, read-only refusal, concurrent same-SHA editing, workspace boundaries, and honest reporting of partial graphs. All passed. In the concurrency control, exactly one of two competing writes based on the same SHA succeeded. [S4,S7] These results do not establish semantic accuracy for arbitrary language relationships or substitute for an independent security audit.

For the ten ownership-check mutation scenarios, required identity recall and complete source recall are both 55% at 1K, while the all-required edit-input rate is only 10%. At 2K and 4K, all three metrics are 100%. The defect patterns and Gold annotations are supplied in advance, and queries may explicitly name the target functions. The measured quantity is therefore **bug-relevant evidence delivery**, not autonomous bug-detection accuracy or a mutation-testing kill rate. [S4,S7]

## 7 Discussion

### 7.1 Tool quality requires decomposition, not another scalar score

The most informative observation is not that 2K attains full coverage. It is that natural-language and impact queries can achieve perfect identity recall while delivering only half of the required source. Optimizing recall alone may return many correct names while forcing the model to retrieve the content needed for its next action through additional calls. Identity, source completeness, hashes, and latency are more useful as separate constraints and diagnostic dimensions than as an opaque aggregate score.

An engineering failure may originate in model decisions or in insufficient evidence. Separate tool-level measurement makes the latter independently testable, but it does not excuse the former. End-to-end evaluation is still needed to determine whether models use the delivered evidence correctly.

### 7.2 Tight-budget failures cannot simply be attributed to long source

For all 59 answerable scenarios with a recorded raw-source lower bound, the union of required source bytes remains below 4,000 bytes at the 1K budget. For example, the required source in the Rust symptom-only natural-language scenario totals 171 bytes, yet complete source delivery is deficient. [S7] The observations therefore do not justify attributing the shortfall to raw source size alone.

Candidate explanations include metadata competition, target ordering, body selection, and interactions with budget packing. We have not performed stage-by-stage ablations and do not present these hypotheses as established causes. Likewise, a raw-source lower bound below the response cap is not a sufficient condition for the complete evidence package to fit.

### 7.3 A layered evaluation agenda

Future evaluation should add lexical-retrieval and syntax-only baselines under the same budget and output contract, then separately ablate source prioritization, graph filtering, and cache behavior. Corpus membership and failure denominators must remain fixed across comparisons; shrinking output or removing difficult cases can otherwise create misleading improvements.

A subsequent real-task study should fix the model, prompt, call budget, and retry policy. Only then can it estimate whether tool-level improvements translate into higher repair success, fewer tool calls, or lower cost. These experiments are proposed work, not implemented baselines or results reported here.

To make that next study falsifiable, a lexical baseline should use the same task text, workspace contents, serialized budget, and source/hash output contract, rather than receive oracle paths. A syntax-only baseline should use the same parser and packing interface while removing relationship ranking. Mechanism ablations should change one component at a time and retain failed attempts. Real repositories should be assigned to development or held-out evaluation before tuning, with correlated tasks grouped by repository or behavior family in any uncertainty analysis. No numerical improvement is predicted by this design.

## 8 Threats to Validity and Limitations

**Construct validity.** Gold completeness does not imply that a real task has sufficient information. Unannotated context may be useful, and all-required edit inputs do not guarantee complete verification mapping or patch correctness. The simple call-graph controls evaluate syntax relationships only. [S2,S4,S5]

**Internal validity.** The runtime and grader were developed together. Independent scoring logic and counterexamples reduce some failure modes but may leave shared blind spots. Matching source snapshots before and after execution does not identify every input consumed during compilation, and a binary fingerprint cannot recover uncommitted source. A single diagnostic observation per cell cannot characterize quality variability or reliable request-level tail latency. The 360 cells reuse the same 60 scenarios across budgets and cache phases; they are not 360 independent tasks. Even the 60 scenarios share a behavior family. We therefore do not attach binomial confidence intervals or significance claims that would incorrectly treat these observations as independent draws. [S6,S7]

**External validity.** The corpus shares one core behavior family and includes more Rust-specific scenarios. Per-language averages must not be interpreted as a ranking of language support. The study lacks an independent real-repository holdout, live-LSP accuracy evaluation, complete network-MCP measurement, CPU or resident-set-size measurements, and end-to-end model experiments. Its results do not support comparisons among Kimi, Claude, Codex, or other coding products. [S2,S7]

**Reproducibility limits.** The available materials provide a traceable snapshot and execution protocol, not a fully frozen source artifact. The original report resides under the repository’s `target/` directory and may be removed by build cleanup. A formal artifact release should include reviewed uncommitted changes, dependency locks, a source manifest, compilation records, and raw samples, rather than only Git HEAD and aggregate tables. [S7]

## 9 Conclusion

We presented wcode as an evidence-driven engineering control plane and used Engineering Fitness to separate tool quality into identity, source delivery, edit preconditions, and selected safety constraints. A diagnostic study over 60 synthetic development scenarios reveals a substantial descriptive gap between localization and actionable evidence. At 1K, required identity recall is 86.73%, complete source recall is 68.37%, and the all-required edit-input rate is 48.28%. The natural-language subset delivers only 50% of required source despite perfect identity recall. [S7]

Coding-tool evaluation should therefore ask not only whether the right code was found, but whether the evidence delivered is authentic, complete, revision-consistent, and sufficient for the specified next operation. This principle provides a model-free entry point for engineering diagnosis. Claims about autonomous programming ability and real-task benefit still require independent end-to-end evaluation.

## References

[1] Carlos E. Jimenez, John Yang, Alexander Wettig, Shunyu Yao, Kexin Pei, Ofir Press, and Karthik Narasimhan. *SWE-bench: Can Language Models Resolve Real-World GitHub Issues?* [arXiv:2310.06770](https://arxiv.org/abs/2310.06770), 2023.

[2] John Yang, Carlos E. Jimenez, Alexander Wettig, Kilian Lieret, Shunyu Yao, Karthik Narasimhan, and Ofir Press. *SWE-agent: Agent-Computer Interfaces Enable Automated Software Engineering.* [arXiv:2405.15793](https://arxiv.org/abs/2405.15793), 2024.

[3] Chunqiu Steven Xia, Yinlin Deng, Soren Dunn, and Lingming Zhang. *Agentless: Demystifying LLM-based Software Engineering Agents.* [arXiv:2407.01489](https://arxiv.org/abs/2407.01489), 2024.

[4] Fengji Zhang, Bei Chen, Yue Zhang, Jacky Keung, Jin Liu, Daoguang Zan, Yi Mao, Jian-Guang Lou, and Weizhu Chen. *RepoCoder: Repository-Level Code Completion Through Iterative Retrieval and Generation.* [arXiv:2303.12570](https://arxiv.org/abs/2303.12570), 2023.

[5] Di Wu, Wasi Uddin Ahmad, Dejiao Zhang, Murali Krishna Ramanathan, and Xiaofei Ma. *Repoformer: Selective Retrieval for Repository-Level Code Completion.* [arXiv:2403.10059v2](https://arxiv.org/abs/2403.10059v2), 2024.

[6] Model Context Protocol contributors. *[Model Context Protocol Specification](https://modelcontextprotocol.io/specification/2026-07-28).* Version 2026-07-28. Official protocol documentation. Accessed September 18, 2026.

[7] Pujun Zheng, Zixin Shang, Shufan Jiang, Wenhui Tian, Dongsheng Zhu, Zerun Ma, Dingbo Yuan, and Qi Zhang. *SWE-Bench Pro Verified: A Reliable Benchmark for Software Engineering Agents.* [arXiv:2609.08149v2](https://arxiv.org/abs/2609.08149v2), September 16, 2026. Preprint.

## Implementation and Experimental Sources

[S1] wcode `README.md` and `Cargo.toml`: system positioning, workflow, transports, package version, and operational boundaries.

[S2] `docs/manual/engineering-fitness.md` and `.wcode/design/requirements/fitness.yaml`: the measurement contract and unmeasured dimensions.

[S3] `src/runtime/harness/agent_context.rs`: context assembly, parallel reads, source handling, and final budget enforcement.

[S4] `tests/unit/runtime/harness/fitness/`, including `corpus.rs`, `controls.rs`, `checks.rs`, and dedicated counterexample modules: fixtures, Gold annotations, operational controls, and regression checks.

[S5] `scoring.rs` in the same directory: identity, complete-source, edit-input, ranking, and Non-Gold-symbol scoring.

[S6] `report.rs`, `delivery.rs`, and `breakdown.rs` in the same directory: the measurement matrix, failure denominators, byte unions, fingerprints, and report generation.

[S7] The canonical diagnostic report, `target/engineering-fitness-1789665000441866000-37653.json`, and its corresponding Markdown report. Exact hashes, commands, and evidence boundaries are recorded in the accompanying `evidence.en.md`; selected fields are retained in `snapshot.json`. The selected-field file is not a copy of the complete raw report.

[S8] `src/runtime/harness/quality/verification_run.rs` and `src/evidence/experience.rs`: revision-bound verification, evidence reuse, and historical co-change hints. Their independent effectiveness is not evaluated in this study.

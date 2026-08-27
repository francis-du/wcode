use super::*;
use crate::scopes;

pub(super) fn tools() -> Vec<Value> {
    vec![
        tool("workspace_info", "Show configured workspace IDs, roots, capabilities, scheduling guidance, and the active security policy. Inspect this before selecting a workspace or command strategy.", json!({"type":"object","properties":{},"additionalProperties":false}), true, false),
        tool("design_status", "Load and validate the structured Desired Software State under .wcode/. Returns project identity, requirement/component/constraint/decision/acceptance counts, and bounded diagnostics without reading implementation source into the model context.", schema(json!({}), &[]), true, false),
        tool("convention_status", "Inspect cross-language convention policies and repository architecture findings through the Harness. Reports file naming, architecture-domain classification, unclassified root source files, oversized modules, flat Rust domain growth, detected languages, bounded counts, and truncation state.", schema(json!({}), &[]), true, false),
        tool("scope_status", "Audit the current repository against wcode's canonical Product Scope registry. Returns per-scope source counts, mapped and unmapped source totals, bounded unmapped paths, and the same scope registry used by context retrieval, semantics, MCP metadata, and conventions.", schema(json!({}), &[]), true, false),
        tool("design_init", "Initialize sparse structured Design State for an uninitialized workspace. Creates .wcode/project.yaml and design/product.yaml; requirement/component/constraint/acceptance/decision collections remain absent until meaningful content exists. Existing design state is never overwritten.", schema(json!({"name":{"type":"string","minLength":1,"maxLength":200},"description":{"type":"string","maxLength":1000}}), &[]), false, false),
        tool("software_graph", "Build and persist a bounded composite Software Graph from declared Design State, Tree-sitter syntax facts, and the latest imported external semantic/runtime provider facts. Every edge retains its own provider/precision/revision; syntax facts are never promoted to compiler semantics.", schema(json!({"path":{"type":"string","default":"."},"max_files":{"type":"integer","minimum":1,"maximum":5000,"default":500},"max_symbols":{"type":"integer","minimum":1,"maximum":5000,"default":1000}}), &[]), true, false),
        tool("graph_provider_import", "Persist one bounded external Software Graph provider revision. This is the provider-neutral adapter for SCIP/LSP/compiler/runtime indexers: the external producer supplies nodes/edges plus provider, precision and revision, and wcode overlays the latest revision without pretending Tree-sitter produced semantic facts.", schema(json!({"provider_graph":{"type":"object","properties":{"provider":{"type":"string","minLength":1,"maxLength":128},"precision":{"type":"string","enum":["semantic","runtime","deterministic","heuristic"]},"revision":{"type":"string","minLength":1,"maxLength":256},"nodes":{"type":"array","maxItems":10000,"items":{"type":"object","properties":{"id":{"type":"string","minLength":1,"maxLength":512},"kind":{"type":"string","enum":["product","requirement","acceptance_criterion","constraint","decision","component","package","module","file","symbol","function","struct","trait","class","interface","api","database","queue","config","test","verification","risk","evidence"]},"label":{"type":"string","minLength":1,"maxLength":500},"attributes":{"type":"object"}},"required":["id","kind","label"],"additionalProperties":false}},"edges":{"type":"array","maxItems":50000,"items":{"type":"object","properties":{"from":{"type":"string","minLength":1,"maxLength":512},"to":{"type":"string","minLength":1,"maxLength":512},"kind":{"type":"string","enum":["contains","defines","references","calls","imports","depends_on","implements","extends","implements_requirement","constrained_by","tested_by","verified_by","guards_against","produces_evidence","runtime_calls","conflicts_with"]}},"required":["from","to","kind"],"additionalProperties":false}}},"required":["provider","precision","revision"],"additionalProperties":false}}), &["provider_graph"]), false, false),
        tool("graph_provider_status", "List the latest persisted external semantic/runtime graph provider revisions for the selected workspace, including precision, revision, node/edge counts, and import time.", schema(json!({}), &[]), true, false),
        tool("semantic_provider_status", "Auto-detect source languages and first-party LSP semantic providers for every language supported by wcode's syntax index. Reports provider availability, policy readiness, and honest syntax fallback when no semantic server is available.", schema(json!({}), &[]), true, false),
        tool("language_quality_status", "Inspect the per-language capability matrix across syntax, semantic providers, repository-declared formatter/linter/type/static/test/security providers, and advanced Property/Mutation/Fuzz/Runtime stages. Support is reported by dimension and explicit gaps rather than one boolean.", schema(json!({}), &[]), true, false),
        tool("language_quality_run", "Run one repository-declared, available, check-only language quality provider through wcode's trusted runtime authorization boundary and persist current-revision Evidence. This lane never invokes formatter fix/write modes.", schema(json!({"language":{"type":"string","enum":["bash","c","cpp","c-sharp","css","dart","elixir","go","html","java","java-script","lua","ocaml","ocaml-interface","php","python","r","ruby","rust","swift","type-script","tsx"]},"provider_id":{"type":"string","minLength":1,"maxLength":160},"timeout_seconds":{"type":"integer","minimum":1,"maximum":300,"default":120}}), &["language","provider_id"]), false, false),
        tool("semantic_provider_refresh", "Run the detected first-party LSP semantic providers for the selected workspace and persist real semantic Document Symbol / Call Hierarchy facts into the Software Graph. Requires --allow-risky-exec because language servers load repository-controlled project configuration.", schema(json!({"path":{"type":"string","default":"."},"max_files":{"type":"integer","minimum":1,"maximum":256,"default":128},"max_symbols":{"type":"integer","minimum":1,"maximum":2000,"default":1000}}), &[]), false, false),
        tool("graph_history", "List bounded persisted composite Software Graph snapshots. Identical graph content is deduplicated, so history represents meaningful graph revisions rather than read frequency.", schema(json!({"limit":{"type":"integer","minimum":1,"maximum":64,"default":20}}), &[]), true, false),
        tool("graph_query", "Query a persisted Software Graph snapshot by node id/kind/label or by incoming/outgoing relationship. Omit snapshot_id to query the latest snapshot; results remain bounded and include the snapshot/provider precision metadata.", schema(json!({"query":{"type":"object","properties":{"snapshot_id":{"type":"string","minLength":1,"maxLength":160},"node_id":{"type":"string","minLength":1,"maxLength":512},"kind":{"type":"string","enum":["product","requirement","acceptance_criterion","constraint","decision","component","package","module","file","symbol","function","struct","trait","class","interface","api","database","queue","config","test","verification","risk","evidence"]},"label_contains":{"type":"string","minLength":1,"maxLength":500},"related_to":{"type":"string","minLength":1,"maxLength":512},"edge_kind":{"type":"string","enum":["contains","defines","references","calls","imports","depends_on","implements","extends","implements_requirement","constrained_by","tested_by","verified_by","guards_against","produces_evidence","runtime_calls","conflicts_with"]},"direction":{"type":"string","enum":["incoming","outgoing","both"]},"limit":{"type":"integer","minimum":1,"maximum":500,"default":100}},"additionalProperties":false}}), &["query"]), true, false),
        tool("graph_diff", "Compare two persisted Software Graph revisions without treating provenance revision churn as delete/add noise. Node IDs and stable edge identities are aligned first; true additions/removals and changed attributes/provenance are returned separately with bounded counts. Omit IDs to compare the latest two meaningful graph snapshots.", schema(json!({"diff":{"type":"object","properties":{"from_snapshot_id":{"type":"string","minLength":1,"maxLength":160},"to_snapshot_id":{"type":"string","minLength":1,"maxLength":160},"limit":{"type":"integer","minimum":1,"maximum":200,"default":50}},"additionalProperties":false}}), &[]), true, false),
        tool("traceability_status", "Resolve Requirement → Component → implementation and Acceptance Criterion → verification chains from structured Design State. File existence is deterministic; symbol/test resolution uses Tree-sitter syntax precision; Harness check references resolve only when present in the inferred project verification profile. Returns separate coverage dimensions rather than one health score.", schema(json!({}), &[]), true, false),
        tool("drift_status", "Compare the current Git change set with Design State traceability and report bounded implementation drift and design drift findings. The result distinguishes desired-state changes that are not reflected in Actual State from design-mapped implementation changes that have no corresponding Design State change.", schema(json!({"timeout_seconds":{"type":"integer","minimum":1,"maximum":120,"default":30}}), &[]), true, false),
        tool("risk_status", "Assess the current change set, traceability gaps, and drift findings into structured Risk records and a risk-adaptive verification profile. Risk is multi-dimensional evidence for verification depth, not a single quality score.", schema(json!({"timeout_seconds":{"type":"integer","minimum":1,"maximum":120,"default":30}}), &[]), true, false),
        tool("impact_analysis", "Map the current Git change set through Design State to impacted components, requirements, acceptance criteria, declared implementation symbols, public-API signals, security boundaries, and overall risk. This is conservative impact analysis; Tree-sitter relationships remain syntax precision.", schema(json!({"timeout_seconds":{"type":"integer","minimum":1,"maximum":120,"default":30}}), &[]), true, false),
        tool("software_context", "Retrieve bounded task-oriented software intelligence: matching requirements, components, constraints, scoped confirmed semantics, syntax-level symbols, known risks, and traceability coverage. Optional scopes accept canonical wcode Product Scopes (design, graph, semantics, traceability, risk, verification, evidence, reconciliation, workspace, integrations, runtime, experience) or freeform business scopes; recognized product scopes narrow source navigation to the relevant subsystem.", schema(json!({"query":{"type":"string","minLength":1,"maxLength":1000},"intent":{"type":"string","minLength":1,"maxLength":128,"default":"inspect"},"budget":{"type":"integer","minimum":1000,"maximum":64000,"default":12000},"scopes":{"type":"array","maxItems":32,"items":{"type":"string","minLength":1,"maxLength":300}}}), &["query"]), true, false),
        tool("semantic_status", "Read the persistent workspace semantic registry. Candidate facts are non-authoritative conversation/provider/user proposals; only explicitly confirmed facts are used as authoritative query expansion, and retired facts are excluded.", schema(json!({"limit":{"type":"integer","minimum":1,"maximum":500,"default":50}}), &[]), true, false),
        tool("semantic_query", "Search the persistent semantic registry by canonical term, alias, description, scope, or relationship triple. Optional scopes now act as real filters: scoped facts must overlap a requested scope while unscoped facts remain global. Canonical wcode Product Scope aliases are normalized alongside freeform business scopes.", schema(json!({"query":{"type":"string","minLength":1,"maxLength":1000},"scopes":{"type":"array","maxItems":32,"items":{"type":"string","minLength":1,"maxLength":300}},"include_candidates":{"type":"boolean","default":true},"limit":{"type":"integer","minimum":1,"maximum":100,"default":20}}), &["query"]), true, false),
        tool("semantic_record", "Record a persistent semantic candidate without making it authoritative. Use this for user-proposed, design-derived, conversation-learned, or external-provider semantic facts; candidates never auto-promote into confirmed semantics.", schema(json!({"fact":{"type":"object","properties":{"kind":{"type":"string","enum":["concept","alias","entity","metric","dimension","relationship","rule","domain_term"]},"canonical":{"type":"string","minLength":1,"maxLength":300},"aliases":{"type":"array","maxItems":32,"items":{"type":"string","minLength":1,"maxLength":300}},"description":{"type":"string","minLength":1,"maxLength":2000},"scopes":{"type":"array","maxItems":32,"items":{"type":"string","minLength":1,"maxLength":300}},"subject":{"type":"string","minLength":1,"maxLength":512},"predicate":{"type":"string","minLength":1,"maxLength":256},"object":{"type":"string","minLength":1,"maxLength":512},"origin":{"type":"string","enum":["user","conversation","design","provider"]},"provider":{"type":"string","minLength":1,"maxLength":256},"confidence":{"type":"string","enum":["low","medium","high"]},"source":{"type":"string","minLength":1,"maxLength":1000}},"required":["kind","canonical","description","origin","confidence"],"additionalProperties":false}}), &["fact"]), false, false),
        tool("semantic_confirm", "Promote one semantic candidate to confirmed authoritative workspace semantics. Only call after explicit human confirmation; confirmed=true and an attestation identity are required. Conversation/model candidates must never self-promote.", schema(json!({"fact_id":{"type":"string","minLength":1,"maxLength":160},"attested_by":{"type":"string","minLength":1,"maxLength":256},"confirmed":{"type":"boolean","const":true}}), &["fact_id","attested_by","confirmed"]), false, false),
        tool("semantic_retire", "Retire one semantic fact through a new persistent revision. Only call after explicit human confirmation; retired facts stop affecting software_context expansion but remain auditable in history.", schema(json!({"fact_id":{"type":"string","minLength":1,"maxLength":160},"attested_by":{"type":"string","minLength":1,"maxLength":256},"confirmed":{"type":"boolean","const":true}}), &["fact_id","attested_by","confirmed"]), false, false),
        tool("verification_plan", "Create a risk-adaptive Verification Plan for the current change set. The plan selects deterministic verification depth and creates blind independent reviewer jobs without binding wcode to a model provider.", schema(json!({"timeout_seconds":{"type":"integer","minimum":1,"maximum":120,"default":30}}), &[]), false, false),
        tool("verification_claim", "Claim one queued blind Verification Job whose required capabilities match the reviewer. The job does not expose other reviewer submissions, preserving independent first-pass review, and carries bounded role-specific guidance when that role has a shared review rubric.", schema(json!({"reviewer":{"type":"string","minLength":1,"maxLength":256},"capabilities":{"type":"array","minItems":1,"maxItems":32,"items":{"type":"string","minLength":1,"maxLength":128}},"role":{"type":"string","enum":["design_compliance","correctness","maintainability","architecture","security","performance","compatibility","adversarial","test_synthesis"]}}), &["reviewer","capabilities"]), false, false),
        tool("verification_submit", "Submit a structured verdict for a claimed Verification Job. The submission is converted into persistent provenance-bearing model-review Evidence including summary, claims, risks, and model identity.", schema(json!({"job_id":{"type":"string","minLength":1,"maxLength":160},"reviewer":{"type":"string","minLength":1,"maxLength":256},"submission":{"type":"object","properties":{"verdict":{"type":"string","enum":["pass","fail","inconclusive"]},"summary":{"type":"string","minLength":1,"maxLength":2000},"claims":{"type":"array","maxItems":32,"items":{"type":"string","minLength":1,"maxLength":1000}},"risks":{"type":"array","maxItems":32,"items":{"type":"string","minLength":1,"maxLength":1000}},"model":{"type":"string","minLength":1,"maxLength":256}},"required":["verdict","summary"],"additionalProperties":false}}), &["job_id","reviewer","submission"]), false, false),
        tool("verification_executor_status", "Inspect the cross-language Property/Mutation/Fuzz/Runtime executor registry. wcode auto-discovers common framework runners and also accepts bounded no-shell executors in .wcode/executors.yaml, so every indexed language can plug into the same Verification Mesh.", schema(json!({}), &[]), true, false),
        tool("verification_execute_stages", "Execute all currently required Property/Mutation/Fuzz/Runtime stages for one Verification Plan using the first matching configured or auto-discovered executor. Each real command result becomes persistent stage Evidence. Requires --allow-risky-exec because project tests and configured executors run repository-controlled code.", schema(json!({"plan_id":{"type":"string","minLength":1,"maxLength":160}}), &["plan_id"]), false, false),
        tool("verification_stage_submit", "Submit real Property, Mutation, Fuzz, or Runtime/Canary stage evidence for a Verification Plan. This is the provider-neutral execution adapter: external test systems or agents submit their actual result and artifact digest; verification_status keeps the latest result per producer and aggregates the stage fail-closed, so another producer's later Pass cannot mask a Fail.", schema(json!({"plan_id":{"type":"string","minLength":1,"maxLength":160},"submission":{"type":"object","properties":{"stage":{"type":"string","enum":["property","mutation","fuzz","runtime_canary"]},"producer":{"type":"string","minLength":1,"maxLength":256},"verdict":{"type":"string","enum":["pass","fail","inconclusive"]},"summary":{"type":"string","minLength":1,"maxLength":2000},"artifact_digest":{"type":"string","minLength":1,"maxLength":512},"model":{"type":"string","minLength":1,"maxLength":256}},"required":["stage","producer","verdict","summary","artifact_digest"],"additionalProperties":false}}), &["plan_id","submission"]), false, false),
        tool("verification_approve", "Record explicit human approval as persistent HumanApproval Evidence for a Verification Plan that requires it. Only call this after a human has explicitly approved the plan; confirmed=true is required and models must never self-approve.", schema(json!({"plan_id":{"type":"string","minLength":1,"maxLength":160},"approver":{"type":"string","minLength":1,"maxLength":256},"statement":{"type":"string","minLength":1,"maxLength":2000},"confirmed":{"type":"boolean","const":true}}), &["plan_id","approver","statement","confirmed"]), false, false),
        tool("verification_status", "Read one Verification Plan's durable reviewer state and readiness gate: deterministic result, stage evidence, queued/claimed/submitted jobs, reviewer failures/inconclusive/disagreement, human approval, stale-revision blockers, and final ready state. The plan must belong to the selected workspace.", schema(json!({"plan_id":{"type":"string","minLength":1,"maxLength":160}}), &["plan_id"]), true, false),
        tool("verification_history", "List recent persisted Verification Plans with their current readiness, evidence-stage results, reviewer state, human approval, and blockers. This survives wcode restarts.", schema(json!({"limit":{"type":"integer","minimum":1,"maximum":100,"default":20}}), &[]), true, false),
        tool("evidence_status", "Read bounded verification Evidence accumulated in this runtime, optionally filtered by subject. Deterministic checks and model-review evidence retain producer, revision, confidence, policy, and result provenance.", schema(json!({"subject":{"type":"string","minLength":1,"maxLength":512},"limit":{"type":"integer","minimum":1,"maximum":500,"default":50}}), &[]), true, false),
        tool("reconciliation_plan", "Create and persist a bounded Reconciliation Plan from current Design State, Git Actual State, drift, transitive syntax impact, risk, Change IR intents, implementation tasks, and a risk-adaptive Verification Plan. This plans convergence; it does not automatically apply source edits.", schema(json!({"timeout_seconds":{"type":"integer","minimum":1,"maximum":120,"default":30}}), &[]), false, false),
        tool("reconciliation_status", "Load one persisted Reconciliation Plan by ID for the selected workspace. Plans survive wcode restarts and can be handed to a different model executor.", schema(json!({"plan_id":{"type":"string","minLength":1,"maxLength":160}}), &["plan_id"]), true, false),
        tool("reconciliation_history", "List the most recent persisted Reconciliation Plans for the selected workspace.", schema(json!({"limit":{"type":"integer","minimum":1,"maximum":100,"default":10}}), &[]), true, false),
        tool("reconciliation_execution_status", "Read the durable execution state for one Reconciliation Plan. Safe implementation/design/review tasks are dependency-aware and claimable by model executors; Verification and HumanApproval tasks advance only from real verification/human evidence. converged=true means every plan task completed without a failed task.", schema(json!({"plan_id":{"type":"string","minLength":1,"maxLength":160}}), &["plan_id"]), true, false),
        tool("reconciliation_claim", "Claim one currently runnable Design, Implementation, or Review task from a persisted Reconciliation execution. Dependency order is enforced and system Verification/HumanApproval tasks cannot be claimed by models.", schema(json!({"plan_id":{"type":"string","minLength":1,"maxLength":160},"executor":{"type":"string","minLength":1,"maxLength":256},"kinds":{"type":"array","maxItems":3,"items":{"type":"string","enum":["design","implementation","review"]}}}), &["plan_id","executor"]), false, false),
        tool("reconciliation_submit", "Complete or fail one claimed Reconciliation task. The executor identity must match the claimant; the result is persisted in execution history and also emitted as provenance-bearing Reconciliation Evidence.", schema(json!({"plan_id":{"type":"string","minLength":1,"maxLength":160},"task_id":{"type":"string","minLength":1,"maxLength":160},"executor":{"type":"string","minLength":1,"maxLength":256},"submission":{"type":"object","properties":{"success":{"type":"boolean"},"summary":{"type":"string","minLength":1,"maxLength":2000},"artifact_digest":{"type":"string","minLength":1,"maxLength":512}},"required":["success","summary"],"additionalProperties":false}}), &["plan_id","task_id","executor","submission"]), false, false),
        tool("reconciliation_retry", "Requeue one failed model-executable Reconciliation task. This never bypasses dependencies or retries Verification/HumanApproval system gates; it only resets a failed Design/Implementation/Review task so another executor can claim it.", schema(json!({"plan_id":{"type":"string","minLength":1,"maxLength":160},"task_id":{"type":"string","minLength":1,"maxLength":160}}), &["plan_id","task_id"]), false, false),
        tool("project_context", "Build a bounded, cached coding context for one workspace: repository guidance excerpts, detected project types and manifests, recommended quality checks, and a preferred change workflow. Call this before substantial coding work.", schema(json!({}), &[]), true, false),
        tool(
            "review_changes",
            "Review the current Git change set before verification. Runs bounded Git status, diff-check, and numstat probes in parallel; classifies changed files; adds maintainability signals for 1k-line threshold crossings, concentrated source growth, and cross-Product-Scope churn; and recommends quick or full verification.",
            schema(json!({"timeout_seconds":{"type":"integer","minimum":1,"maximum":120,"default":30}}), &[]),
            true,
            false,
        ),
        tool(
            "parallel_tools",
            "Schedule 2-128 bounded read/discovery operations or workspace file writes. Every child uses a real global semaphore slot and appears separately in the TUI. Same-file apply_edits with the same SHA are coalesced into one atomic commit; the resource dependency graph fans out independent tasks and orders overlapping read/write, parent/child, move, delete, and directory-creation dependencies.",
            json!({
                "type": "object",
                "properties": {
                    "tasks": {
                        "type": "array",
                        "minItems": 2,
                        "maxItems": MAX_PARALLEL_FANOUT_ITEMS,
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": {"type": "string"},
                                "tool": {"type": "string", "enum": PARALLEL_READ_TOOLS.iter().chain(PARALLEL_WRITE_TOOLS.iter()).copied().collect::<Vec<_>>()},
                                "arguments": {"type": "object"}
                            },
                            "required": ["tool"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["tasks"],
                "additionalProperties": false
            }),
            false,
            true,
        ),
        tool("verify_project", "Run exact Harness-inferred quality checks with bounded, phased parallelism. This dedicated verification lane may execute approved check/test/Clippy/build shapes without --allow-risky-exec; arbitrary model-facing run_command calls remain under the stricter trust policy. Independent checks in the same phase use separate semaphore slots; tests, Clippy, and builds are sequenced to reduce compiler-cache contention.", schema(json!({"level":{"type":"string","enum":["quick","full"],"default":"quick"},"timeout_seconds":{"type":"integer","minimum":1,"maximum":300,"default":120}}), &[]), false, false),
        tool("list_files", "Fast recursive file listing inside one workspace root. All regular files are visible except protected credential, repository-control, and wcode-internal paths; symlinks are not followed.", schema(json!({"path":{"type":"string"},"max_entries":{"type":"integer","minimum":1,"maximum":10000,"default":2000}}), &[]), true, false),
        tool("search_code", "Fast exact-substring search in one workspace. File scanning runs off the async runtime and uses parallel workers.", schema(json!({"query":{"type":"string"},"path":{"type":"string"},"max_results":{"type":"integer","minimum":1,"maximum":500}}), &["query"]), true, false),
        tool("search_many", "Search up to 32 exact substrings in one filesystem traversal. Prefer this over repeated search_code calls when looking for several symbols.", schema(json!({"queries":{"type":"array","minItems":1,"maxItems":32,"items":{"type":"string"}},"path":{"type":"string"},"max_results":{"type":"integer","minimum":1,"maximum":1000}}), &["queries"]), true, false),
        tool(
            "file_outline",
            "Parse one supported source file with Tree-sitter and return syntax-level definitions, qualified names, exact ranges, redacted signatures, total/returned symbol counts, parse status, and cache metadata. Supports Bash, C, C++, C#, CSS, Dart, Elixir, Go, HTML, Java, JavaScript, Lua, OCaml/interfaces, PHP, Python, R, Ruby, Rust, Swift, and TypeScript/TSX. HTML indexes id-bearing elements and custom components; CSS indexes selectors, custom properties, and keyframes.",
            schema(json!({
                "path": {"type": "string"},
                "max_symbols": {"type": "integer", "minimum": 1, "maximum": 1000, "default": 500}
            }), &["path"]),
            true,
            false,
        ),
        tool(
            "find_symbol",
            "Find syntax-level symbol definitions by name or qualified name across a file or directory. Results include opaque symbol IDs for symbol_context, provider/precision metadata, exact ranges, redacted signatures, and language. IDs are tied to the current indexed revision, so query again after edits. The index is lazy and parallel.",
            schema(json!({
                "query": {"type": "string"},
                "path": {"type": "string", "default": "."},
                "kind": {"type": "string", "description": "Optional Tree-sitter tag kind such as function, method, class, interface, module, or type."},
                "max_results": {"type": "integer", "minimum": 1, "maximum": 200, "default": 50}
            }), &["query"]),
            true,
            false,
        ),
        tool(
            "symbol_context",
            "Expand a symbol ID returned by file_outline or find_symbol into bounded source, syntax-level calls, same-file call targets, nested definitions, parse status, and in-memory AST cache metadata.",
            schema(json!({
                "symbol_id": {"type": "string"},
                "max_body_lines": {"type": "integer", "minimum": 1, "maximum": 500, "default": 200}
            }), &["symbol_id"]),
            true,
            false,
        ),
        tool("read_file", "Read one UTF-8 file with line bounds and receive its SHA-256 edit precondition.", schema(json!({"path":{"type":"string"},"start_line":{"type":"integer","minimum":1},"end_line":{"type":"integer","minimum":1}}), &["path"]), true, false),
        tool("read_files", "Read up to 32 UTF-8 files in one MCP round trip. Reads run in parallel and each file reports success or failure independently.", schema(json!({"paths":{"type":"array","minItems":1,"maxItems":32,"items":{"type":"string"}},"start_line":{"type":"integer","minimum":1},"end_line":{"type":"integer","minimum":1}}), &["paths"]), true, false),
        tool("read_media", "Inspect one bounded workspace media file. Metadata is always safe to return. Set include_content=true only when the MCP client explicitly advertises the run.francis.wcode/media-content extension for the media kind; otherwise wcode fails closed without emitting image/audio payloads. PNG/JPEG/GIF/WebP image content and MP3/WAV/Ogg/FLAC audio content are supported; MP4/WebM are metadata-only.", schema(json!({"path":{"type":"string"},"include_content":{"type":"boolean","default":false}}), &["path"]), true, false),
        tool("path_info", "Inspect one workspace path without loading the whole file into model context. Returns type, size, SHA-256 for files, readonly state, modification time, and hard-link count when available.", schema(json!({"path":{"type":"string"}}), &["path"]), true, false),
        tool("replace_text", "Atomically replace one exact text occurrence with a SHA-256 precondition and optional 1-based original line bounds. When start_line/end_line are supplied together, old_text must match exactly once inside that original range. Protected/symlink/hard-link targets remain blocked.", schema(json!({"path":{"type":"string"},"old_text":{"type":"string"},"new_text":{"type":"string"},"expected_sha256":{"type":"string"},"start_line":{"type":"integer","minimum":1},"end_line":{"type":"integer","minimum":1}}), &["path","old_text","new_text","expected_sha256"]), false, true),
        tool("apply_edits", "Atomically apply up to 128 non-overlapping edits against one original SHA revision. Each edit may add 1-based start_line/end_line bounds; all edits resolve against the same original bytes before one atomic commit, so line shifts from sibling edits cannot affect targeting.", schema(json!({"path":{"type":"string"},"expected_sha256":{"type":"string"},"edits":{"type":"array","minItems":1,"maxItems":128,"items":{"type":"object","properties":{"old_text":{"type":"string","minLength":1},"new_text":{"type":"string"},"start_line":{"type":"integer","minimum":1},"end_line":{"type":"integer","minimum":1}},"required":["old_text","new_text"],"additionalProperties":false}}}), &["path","expected_sha256","edits"]), false, true),
        tool("write_file", "Atomically write a complete UTF-8 file. Creating a new file requires no hash; overwriting an existing file requires expected_sha256 and preserves protected-path, symlink, hard-link, and destructive-replacement safeguards.", schema(json!({"path":{"type":"string"},"content":{"type":"string"},"expected_sha256":{"type":"string"}}), &["path","content"]), false, true),
        tool("create_directory", "Recursively create a workspace-relative directory path while rejecting protected paths, symlink components, and workspace escape.", schema(json!({"path":{"type":"string"}}), &["path"]), false, false),
        tool("create_file", "Atomically create one bounded UTF-8 file without overwrite. Protected paths, symlink components, broad path escapes, and races with an existing target are rejected.", schema(json!({"path":{"type":"string"},"content":{"type":"string"}}), &["path","content"]), false, true),
        tool("create_files", "Create up to 64 independent files concurrently. Each file is atomically created without overwrite and reports its own success or failure.", schema(json!({"files":{"type":"array","minItems":1,"maxItems":64,"items":{"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"],"additionalProperties":false}}}), &["files"]), false, true),
        tool("apply_file_edits", "Apply independent multi-edit transactions to up to 64 files concurrently. Every file is pinned to one SHA-256; each edit may also pin a 1-based original start_line/end_line range, and each file commits once atomically after overlap checks.", schema(json!({"files":{"type":"array","minItems":1,"maxItems":64,"items":{"type":"object","properties":{"path":{"type":"string"},"expected_sha256":{"type":"string"},"edits":{"type":"array","minItems":1,"maxItems":128,"items":{"type":"object","properties":{"old_text":{"type":"string","minLength":1},"new_text":{"type":"string"},"start_line":{"type":"integer","minimum":1},"end_line":{"type":"integer","minimum":1}},"required":["old_text","new_text"],"additionalProperties":false}}},"required":["path","expected_sha256","edits"],"additionalProperties":false}}}), &["files"]), false, true),
        tool("move_path", "Move or rename one file or directory inside the workspace without overwriting the destination. File moves may include expected_source_sha256 to pin the exact source revision; directories reject that file-only precondition. Source trees containing symlinks, hard-linked files, protected paths, or workspace escapes are rejected.", schema(json!({"source":{"type":"string"},"destination":{"type":"string"},"expected_source_sha256":{"type":"string"}}), &["source","destination"]), false, true),
        tool("move_paths", "Move up to 64 independent, non-overlapping files/directories concurrently without destination overwrite. Each file move may pin expected_source_sha256; overlapping or dependent paths are rejected before execution.", schema(json!({"moves":{"type":"array","minItems":1,"maxItems":64,"items":{"type":"object","properties":{"source":{"type":"string"},"destination":{"type":"string"},"expected_source_sha256":{"type":"string"}},"required":["source","destination"],"additionalProperties":false}}}), &["moves"]), false, true),
        tool("delete_path", "Delete one regular file or empty directory only after an exact one-shot human authorization in the TUI or protected local Web UI. File deletion requires expected_sha256. Recursive deletion, workspace-root deletion, protected paths, symlinks, and hard-linked files are permanently blocked.", schema(json!({"path":{"type":"string"},"expected_sha256":{"type":"string"}}), &["path"]), false, true),
        tool("run_command", "Run a policy-checked program without a shell, with scrubbed credentials, bounded streaming output, and timeout termination. A small safe command set is pre-authorized. Other bare executable names become explicit human authorization requests and can be approved per workspace in the TUI or protected local Web UI; shell interpreters, path-bearing program names, workspace escape and protected-resource arguments remain blocked.", schema(json!({"program":{"type":"string","minLength":1,"maxLength":256,"description":"Bare executable name. Non-default programs require explicit per-workspace human authorization before execution."},"args":{"type":"array","items":{"type":"string"}},"cwd":{"type":"string"},"timeout_seconds":{"type":"integer","minimum":1,"maximum":300}}), &["program"]), false, true),
    ]
}

fn schema(mut properties: Value, required: &[&str]) -> Value {
    if let Some(properties) = properties.as_object_mut() {
        properties.insert(
            "workspace".to_owned(),
            json!({
                "type": "string",
                "description": "Workspace ID from workspace_info. Omit to use the default workspace."
            }),
        );
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

pub(crate) fn selected_workspace(
    state: &AppState,
    args: &Value,
) -> Result<(String, Workspace), String> {
    state
        .workspaces
        .select(string_arg(args, "workspace"))
        .map_err(|error| error.to_string())
}

pub(super) async fn run_blocking<F>(work: F) -> AnyResult<Value>
where
    F: FnOnce() -> AnyResult<Value> + Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| anyhow!("blocking task failed: {error}"))?
}

fn tool(
    name: &str,
    description: &str,
    input_schema: Value,
    read_only: bool,
    destructive: bool,
) -> Value {
    let product_scopes = scopes::tool_scopes(name)
        .into_iter()
        .map(|scope| scope.as_str())
        .collect::<Vec<_>>();
    json!({
        "name": name,
        "title": name.replace('_', " "),
        "description": description,
        "inputSchema": input_schema,
        "annotations": {
            "readOnlyHint": read_only,
            "destructiveHint": destructive,
            "idempotentHint": read_only,
            "openWorldHint": false,
        },
        "_meta": {
            "dev.wcode/productScopes": product_scopes,
        }
    })
}

pub(super) fn tool_result(value: Value, is_error: bool) -> Value {
    json!({
        "content": [{"type": "text", "text": serde_json::to_string(&value).unwrap_or_else(|_| "{}".into())}],
        "structuredContent": value,
        "isError": is_error,
    })
}

pub(super) fn batch_validation_error(item_count: usize) -> Option<Value> {
    if item_count == 0 {
        Some(jsonrpc_error(Value::Null, -32600, "empty batch is invalid"))
    } else if item_count > MAX_BATCH_ITEMS {
        Some(jsonrpc_error(
            Value::Null,
            -32600,
            format!("batch exceeds the {MAX_BATCH_ITEMS}-item limit"),
        ))
    } else {
        None
    }
}

pub(crate) fn jsonrpc_error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message.into()}})
}

pub(super) fn task_detail(name: &str, args: &Value) -> String {
    let path = || string_arg(args, "path").unwrap_or(".");
    match name {
        "workspace_info" => "inspect configured roots and capabilities".to_owned(),
        "scope_status" => "audit Product Scope source coverage and unmapped files".to_owned(),
        "design_status" => "validate structured desired software state".to_owned(),
        "design_init" => "initialize minimal structured desired software state".to_owned(),
        "software_graph" => format!(
            "{} · software graph · {} files · {} symbols",
            path(),
            usize_arg(args, "max_files").unwrap_or(500),
            usize_arg(args, "max_symbols").unwrap_or(1_000)
        ),
        "graph_provider_import" => {
            "persist external semantic/runtime graph provider revision".to_owned()
        }
        "graph_provider_status" => "inspect active external graph provider revisions".to_owned(),
        "semantic_provider_status" => "inspect first-party semantic provider coverage".to_owned(),
        "language_quality_status" => {
            "inspect per-language quality capability coverage and gaps".to_owned()
        }
        "language_quality_run" => format!(
            "run check-only language quality provider · {} · {}",
            string_arg(args, "language").unwrap_or("unknown"),
            string_arg(args, "provider_id").unwrap_or("unknown")
        ),
        "semantic_provider_refresh" => format!(
            "refresh first-party semantic providers · {} · {} files / {} symbols",
            path(),
            usize_arg(args, "max_files").unwrap_or(128),
            usize_arg(args, "max_symbols").unwrap_or(1_000)
        ),
        "graph_history" => format!(
            "list persisted graph revisions · limit {}",
            usize_arg(args, "limit").unwrap_or(20)
        ),
        "graph_query" => "query one persisted software graph revision".to_owned(),
        "graph_diff" => "compare persisted software graph revisions".to_owned(),
        "traceability_status" => {
            "resolve requirement, implementation, and verification chains".to_owned()
        }
        "software_context" => format!(
            "task context · query {} chars · budget {}",
            string_arg(args, "query").map(str::len).unwrap_or(0),
            usize_arg(args, "budget").unwrap_or(12_000)
        ),
        "semantic_status" => format!(
            "semantic registry · limit {}",
            usize_arg(args, "limit").unwrap_or(50)
        ),
        "semantic_query" => format!(
            "semantic query · {} chars · limit {}",
            string_arg(args, "query").map(str::len).unwrap_or(0),
            usize_arg(args, "limit").unwrap_or(20)
        ),
        "semantic_record" => "record non-authoritative semantic candidate".to_owned(),
        "semantic_confirm" => "confirm semantic fact after explicit human attestation".to_owned(),
        "semantic_retire" => "retire semantic fact after explicit human attestation".to_owned(),
        "evidence_status" => format!(
            "evidence{} · limit {}",
            string_arg(args, "subject")
                .map(|_| " for one subject")
                .unwrap_or(""),
            usize_arg(args, "limit").unwrap_or(50)
        ),
        "verification_claim" => format!(
            "claim blind review · {} capabilities",
            array_len(args, "capabilities")
        ),
        "verification_submit" => "submit structured blind review evidence".to_owned(),
        "verification_executor_status" => {
            "inspect cross-language verification executors".to_owned()
        }
        "verification_execute_stages" => {
            "execute required property/mutation/fuzz/runtime stages".to_owned()
        }
        "verification_stage_submit" => {
            "submit property/mutation/fuzz/runtime stage evidence".to_owned()
        }
        "verification_approve" => "record explicit human approval evidence".to_owned(),
        "verification_status" => "inspect verification readiness and reviewer states".to_owned(),
        "verification_history" => format!(
            "list persisted verification plans · limit {}",
            usize_arg(args, "limit").unwrap_or(20)
        ),
        "reconciliation_status" => "load one persisted reconciliation plan".to_owned(),
        "reconciliation_history" => format!(
            "list persisted reconciliation plans · limit {}",
            usize_arg(args, "limit").unwrap_or(10)
        ),
        "reconciliation_execution_status" => {
            "inspect durable reconciliation execution state".to_owned()
        }
        "reconciliation_claim" => "claim one dependency-ready reconciliation task".to_owned(),
        "reconciliation_submit" => "submit claimed reconciliation task result".to_owned(),
        "reconciliation_retry" => "requeue one failed reconciliation task".to_owned(),
        "project_context" => "collect repository guidance and inferred quality checks".to_owned(),
        "review_changes" => "review the current Git working tree for bounded risks".to_owned(),
        "drift_status" => "compare Design State with the current Git working tree".to_owned(),
        "risk_status" => "assess change, traceability, and drift risks".to_owned(),
        "impact_analysis" => "map current changes to impacted software context".to_owned(),
        "verification_plan" => "create a risk-adaptive verification plan".to_owned(),
        "reconciliation_plan" => {
            "create a bounded Design/Implementation reconciliation plan".to_owned()
        }
        "verify_project" => format!(
            "{} quality gate · timeout {}s",
            string_arg(args, "level").unwrap_or("quick"),
            args.get("timeout_seconds")
                .and_then(Value::as_u64)
                .unwrap_or(120)
        ),
        "parallel_tools" => format!("{} independent tool requests", array_len(args, "tasks")),
        "list_files" => format!(
            "{} · limit {}",
            path(),
            usize_arg(args, "max_entries").unwrap_or(2_000)
        ),
        "search_code" => format!(
            "{} · query {} chars · limit {}",
            path(),
            string_arg(args, "query").map(str::len).unwrap_or(0),
            usize_arg(args, "max_results").unwrap_or(100)
        ),
        "search_many" => format!(
            "{} · {} queries · limit {}",
            path(),
            array_len(args, "queries"),
            usize_arg(args, "max_results").unwrap_or(200)
        ),
        "file_outline" => format!(
            "{} · syntax outline · limit {}",
            path(),
            usize_arg(args, "max_symbols").unwrap_or(500)
        ),
        "find_symbol" => format!(
            "{} · symbol query {} chars · limit {}",
            path(),
            string_arg(args, "query").map(str::len).unwrap_or(0),
            usize_arg(args, "max_results").unwrap_or(50)
        ),
        "symbol_context" => format!(
            "symbol id {} chars · body limit {} lines",
            string_arg(args, "symbol_id").map(str::len).unwrap_or(0),
            usize_arg(args, "max_body_lines").unwrap_or(200)
        ),
        "read_file" => format!(
            "{} · lines {}-{}",
            path(),
            usize_arg(args, "start_line").unwrap_or(1),
            usize_arg(args, "end_line")
                .map(|line| line.to_string())
                .unwrap_or_else(|| "auto".to_owned())
        ),
        "read_files" => format!(
            "{} files · lines {}-{}{}",
            array_len(args, "paths"),
            usize_arg(args, "start_line").unwrap_or(1),
            usize_arg(args, "end_line")
                .map(|line| line.to_string())
                .unwrap_or_else(|| "auto".to_owned()),
            first_array_item(args, "paths")
                .map(|path| format!(" · first {path}"))
                .unwrap_or_default()
        ),
        "read_media" => format!(
            "{} · media metadata{}",
            path(),
            if args
                .get("include_content")
                .and_then(Value::as_bool)
                .is_some_and(|enabled| enabled)
            {
                " + opt-in content"
            } else {
                ""
            }
        ),
        "path_info" => format!("{} · metadata + digest", path()),
        "replace_text" => format!(
            "{} · replace {}B with {}B",
            path(),
            string_arg(args, "old_text").map(str::len).unwrap_or(0),
            string_arg(args, "new_text").map(str::len).unwrap_or(0)
        ),
        "apply_edits" => format!("{} · {} edits", path(), array_len(args, "edits")),
        "write_file" => format!(
            "{} · write {}B{}",
            path(),
            string_arg(args, "content").map(str::len).unwrap_or(0),
            string_arg(args, "expected_sha256")
                .map(|_| " · guarded overwrite")
                .unwrap_or(" · create")
        ),
        "create_directory" => format!("{} · recursive mkdir", path()),
        "create_file" => format!(
            "{} · create {}B",
            path(),
            string_arg(args, "content").map(str::len).unwrap_or(0)
        ),
        "create_files" => format!("{} files · parallel create", array_len(args, "files")),
        "apply_file_edits" => {
            format!(
                "{} files · parallel guarded edits",
                array_len(args, "files")
            )
        }
        "move_path" => format!(
            "{} → {}",
            string_arg(args, "source").unwrap_or("?"),
            string_arg(args, "destination").unwrap_or("?")
        ),
        "move_paths" => format!("{} independent moves", array_len(args, "moves")),
        "delete_path" => format!("{} · one-shot authorized delete", path()),
        "run_command" => command_preview(args),
        _ => "unknown tool request".to_owned(),
    }
}

fn array_len(args: &Value, key: &str) -> usize {
    args.get(key)
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0)
}

fn first_array_item<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_array)
        .and_then(|values| values.first())
        .and_then(Value::as_str)
}

fn command_preview(args: &Value) -> String {
    let program = string_arg(args, "program").unwrap_or("command");
    let command_args = args
        .get("args")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut parts = vec![program.to_owned()];
    let mut redact_next = false;

    for value in command_args.iter().take(6) {
        let Some(argument) = value.as_str() else {
            continue;
        };
        if redact_next {
            parts.push("[REDACTED]".to_owned());
            redact_next = false;
            continue;
        }

        let lower = argument.to_ascii_lowercase();
        let sensitive = ["token", "secret", "password", "passwd", "api-key", "apikey"]
            .iter()
            .any(|needle| lower.contains(needle));
        if sensitive {
            if let Some((key, _)) = argument.split_once('=') {
                parts.push(format!("{key}=[REDACTED]"));
            } else {
                parts.push(argument.to_owned());
                redact_next = argument.starts_with('-');
            }
        } else {
            parts.push(argument.to_owned());
        }
    }
    if command_args.len() > 6 {
        parts.push(format!("…+{}", command_args.len() - 6));
    }
    format!(
        "{} · cwd {}",
        parts.join(" "),
        string_arg(args, "cwd").unwrap_or(".")
    )
}

pub(super) fn string_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

pub(super) fn required_string<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    string_arg(args, key).ok_or_else(|| format!("missing string argument: {key}"))
}

pub(super) fn usize_arg(args: &Value, key: &str) -> Option<usize> {
    args.get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}

pub(super) fn optional_string_array_arg(
    args: &Value,
    key: &str,
    max_items: usize,
) -> Result<Vec<String>, String> {
    let Some(values) = args.get(key) else {
        return Ok(Vec::new());
    };
    let values = values
        .as_array()
        .ok_or_else(|| format!("{key} must be an array"))?;
    if values.len() > max_items {
        return Err(format!("{key} must contain at most {max_items} items"));
    }
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|value| !value.trim().is_empty())
                .map(str::to_owned)
                .ok_or_else(|| format!("{key} must contain non-empty strings"))
        })
        .collect()
}

pub(super) fn string_array_arg(
    args: &Value,
    key: &str,
    max_items: usize,
) -> Result<Vec<String>, String> {
    let values = args
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("missing array argument: {key}"))?;
    if values.is_empty() || values.len() > max_items {
        return Err(format!(
            "{key} must contain between 1 and {max_items} items"
        ));
    }
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| format!("{key} must contain non-empty strings"))
        })
        .collect()
}

pub(super) fn reviewer_role_arg(args: &Value) -> Result<Option<ReviewerRole>, String> {
    args.get("role")
        .cloned()
        .map(|value| {
            serde_json::from_value(value).map_err(|error| format!("invalid reviewer role: {error}"))
        })
        .transpose()
}

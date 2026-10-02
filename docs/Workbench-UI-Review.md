# Workbench UI review

The workbench keeps the current workspace, source revision and snapshot state visible across all seven pages. Navigation remembers the last supported page; unavailable local storage and invalid preferences fall back safely. The current page heading and browser title follow language and workspace changes. Keyboard users can skip directly to the active page or return to acceptance from the brand link.

Observation coverage remains available in a native disclosure. This gives the acceptance decision, task list, source inspection or requirement detail space before supporting coverage on a phone. Existing source precision, stale snapshots, failed evidence and operator permissions keep their original meanings.

| Surface | Main states retained | Verification |
| --- | --- | --- |
| Acceptance | blocked, missing checks, stale evidence, exact evidence navigation | production DOM fixtures and native tests |
| Architecture | system map, components, dependencies, code graph, source inspector, full screen | production DOM geometry fixtures |
| Activity | running, queued, unavailable, worker ownership, task results | production renderer behavior and geometry fixtures |
| Check results | current failure, history, unavailable verification, evidence selection | production renderer behavior and geometry fixtures |
| Current changes | unavailable review, working/index/HEAD comparisons, escaped source | production renderer behavior and geometry fixtures |
| Requirements | filtered selection, traceability and impact | production renderer behavior and geometry fixtures |
| Project files | partial source coverage, searchable file tree, large files, source navigation | production renderer behavior and geometry fixtures |
| Public setup | local connection, verified/unavailable endpoint, optional launch settings, copy success/manual selection | setup behavior harness and browser DOM checks |
| TUI | wide/compact dashboard, tiny-window recovery, help, authorization, command/workspace overlays | existing monitor tests plus new tiny-window matrix |

The tiny-window TUI reserves a final row for help and stop before laying out explanatory text. It shows the current dimensions and the actual minimum height computed by the dashboard, including setup and tunnel rows. The renderer lives in a separate module so the shell stays below its source-size boundary. Runtime dispatch and authorization actions are unchanged.

Local review used the production HTML/CSS/JavaScript with explicit fixture data. Chrome 154 covered 140 page/language/theme/viewport combinations and keyboard tab, skip and home interactions. Native WebKit covered 256 cases, including architecture subviews and source inspectors. Public setup covered 20 browser combinations; clipboard delivery was mocked while feedback and manual selection used the real DOM. A separately compiled production compact renderer covered 32 terminal-size/language combinations; the old renderer fails the same recovery-key negative control.

These fixtures verify presentation and browser behavior. They do not establish live provider, OAuth, tunnel or backend acceptance. The complete native integration suite and cross-platform builds must pass the pull request CI; the isolated compact renderer is supplementary evidence.

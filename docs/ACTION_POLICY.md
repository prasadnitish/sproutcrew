# Action policy and approval migration

The local Rust `Studio` is the authority for role × tool Allow / Ask / Deny. Model text has no permission, tier, or approval parser. Checking roles can read context and return an artifact report; file edits and external effects remain denied. Shell stays denied even when an imported matrix requests more access. Deletes, external sends and dependency installs cannot default to Allow. Path traversal upgrades writes to destructive; app-owned artifact paths also reject symlinks.

Approvals now bind canonical plan data, the effective policy and policy revision, source snapshot hashes, source-role selection, selected tools, exercise requirements, project budget revision, and API routing/pricing configuration. Changing those inputs blocks subsequent dispatch; a prepared job rechecks immediately before calling its provider, and a response is not promoted after approval changes. An already-sent API request cannot be recalled.

Old plan-only approvals require approval again. Project notes remain explicit owner instructions and do not widen tools, destinations or budgets. Their existing journal and trace behavior remains unchanged. If a role list changes after an explicit policy was saved, reset the matrix to valid defaults before approving.

An Ask binds the exact scope, stage, tool, target and payload hash. Its typed risk tier is derived in Rust. Reversible approvals may be batched; consequential/destructive approvals are individual. Each approval is consumed once. External writes reuse the separate existing exact-content human approval; a policy Deny still blocks that approved write. A duplicate/interrupted send remains subject to the existing write state machine.

The active API engines accept text responses only and expose no model file/shell tools. The checked operations are context preparation, artifact output authorization, result promotion, code harvest authorization and external writes through `App`. This does not intercept arbitrary third-party CLI execution or provide an OS sandbox. Subscription CLIs remain disabled. Provenance types and prompt fences reduce risk; full transitive artifact provenance is not proven end to end.

The browser preview intentionally refuses policy mutations and decisions: it is not the Rust security boundary. Native Tauri commands are declared in build permissions and the capability allowlist. Open-source tests use synthetic fixtures and no provider spend.

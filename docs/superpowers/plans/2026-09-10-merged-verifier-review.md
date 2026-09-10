# 2026-09-10 merged-verifier review repair

Branch `fix/merged-verifier-review-20260910` (base 329566d3). Scope: #2273
merged-review audit gaps. Review artifacts live in the outer repair
directory (verifier-red2.log / verifier-green.log and the two peer reports).

## Fixes

1. **Interactive sentinel failure warning carries the WIRE session id.**
   The shared constructor `goal_verifier_failure_warning` normalizes via
   `wire_key_from_goal_key` internally — one production boundary shared by
   the interactive call site and the tests. Goal lookups keep the scoped
   key. Plain sessions pass through unchanged. The AUTONOMOUS station's
   `goal_verifier_warning_event` intentionally does NOT strip: its argument
   is the turn's plain wire `session_id`; the scoped goal key
   (`goal_ctx.goal_session_key`) is a separate, independent value in that
   context.
2. **A replayed verifier failure no longer re-appends the durable /
   in-memory note.** `session_actor` appends only when `!outcome.replayed`;
   the `tracing::warn!` still fires on every refusal; goal status,
   charging, and TTL are untouched. Changed evidence produces a fresh
   verdict and a fresh note.
3. **Spec/plan sync.** Allowed Changes gained the two test files and this
   plan; Decision 2(f) documents the NOT_DONE delimiter (fused tokens fall
   through to InvalidResponse); Decision 5 documents the optional `reason`
   column (serde default; reader precedence reason → missing_evidence →
   error → bare outcome; old v3 rows keep the legacy fallback) and the
   no-GC retention policy — semantic verdicts are permanent replay
   evidence and any lifecycle bound is an operator decision (wontdo this
   round, rationale in the spec). Status moved from draft to implemented;
   personal/machine paths removed.

## Verification

- Behavioral RED → GREEN with real exit codes: a replay duplicated the
  durable note (2 != 1) before the fix; after the fix the targeted four
  tests pass. A first-pass compile error (unicode escape) and an early
  wrong-path note counter (legacy flat layout) are recorded as test bugs,
  not behavioral RED.
- The warning test exercises the shared constructor plus a real
  WsConnection / `send_notification_ephemeral` send — an integration of the
  boundary, not a full interactive turn dispatch (stated honestly).
- Cargo window (2026-09-10, real exit codes, logs in the recovery
  directory): fmt clean; the targeted set INCLUDING the newly added
  in-memory duplicate assertion passes 4/4 (verifier-final-targeted.log,
  EXIT0); clippy `-p octos-cli --features api --all-targets -- -D
  warnings` EXIT0 (verifier-final-clippy.log). All-targets belongs to the
  outer integration run.

## Handoff

Product and tests are frozen on this branch. The outer combined
integration run (workspace-wide clippy --all-targets and test
--all-targets) is owned by ROOT on the review-combined tree; this worktree
runs no further cargo. Final delivery report with per-item dispositions,
RED/GREEN exits, frozen SHAs, and the dual-model review trail lives in the
recovery directory (`verifier-final.md`).

## Dispositions

- Ledger GC: wontdo — permanent semantic replay evidence; see Decision 5.
- The `diagnostic` field already covers composite outcomes; no new kind,
  five-value outcome compatibility unchanged.

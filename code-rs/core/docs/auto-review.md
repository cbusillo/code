# Auto Review Lifecycle

This note describes the durable Auto Review flow shared by exec, TUI, and
auto-drive. The important rule is that Auto Review is a harness-owned quality
pipeline: the assistant may read compact review state and ask for bounded
details, but run identity, freshness, duplicate avoidance, cancellation, and
restart recovery belong to Every Code runtime code.

## Surfaces

- **Exec (`/review` and auto-resolve):** captures a ghost snapshot, acquires the
  shared review lock, runs review, and may loop fixes into follow-up reviews
  until clean or limits hit.
- **TUI background review:** triggers and renders background review progress,
  while writing the same durable run records as exec.
- **Auto-drive:** orchestrates turns and uses the same lock, snapshot epoch, and
  durable run store when it launches or follows up on reviews.
- **Assistant context:** receives only a compact Auto Review ledger when there is
  active, recent, actionable, or diagnostic review state. Full review output is
  lazy detail, not normal turn context.

## Durable Store

Auto Review state is repo scoped under the shared review state directory:

```text
CODE_HOME/state/review/repo-<repo-key>/auto-review/
  runs.json
  outputs/<run_id>.json
```

`runs.json` stores bounded `AutoReviewRun` records. Output sidecars preserve full
review results for lazy detail lookup. The store keeps enough proof to explain a
run without injecting bulky review bodies into every prompt:

- identity and ownership: `run_id`, `source`, `owner_session_id`, `agent_id`,
  `batch_id`, `worktree_path`
- target: `base_commit`, `snapshot_commit`, `snapshot_epoch`, `head_at_launch`,
  `scope_hash`, `diff_fingerprint`, `prompt_policy_version`, changed paths
- lifecycle: `status`, `freshness`, `created_at`, `started_at`, `updated_at`,
  `completed_at`, `last_activity_at`
- execution shape: `model`, `reasoning_effort`, prompt token estimate, actual
  token count when available, saved token estimate when a duplicate is skipped
- result summary: finding digests, summary digest, supersession, cancel reason,
  error class, error summary, and output sidecar path

## Status And Freshness

`status` is the run lifecycle. `freshness` is the usefulness or liveness of the
review target. They are related but not interchangeable.

Run statuses:

- `Pending`: recorded but not yet doing review work.
- `Snapshotting`: capturing or preparing the reviewed snapshot.
- `Reviewing`: the review agent is running against a snapshot.
- `Resolving`: auto-resolve is applying or preparing follow-up work.
- `Completed`: terminal review output was produced.
- `Failed`: the run ended with an execution error.
- `Cancelled`: the harness or user stopped the run for an explicit reason.
- `Superseded`: a newer run replaces this run's useful scope.
- `Skipped`: the harness intentionally did not launch work, commonly because an
  equivalent active or recent review already exists.
- `Lost`: durable state survived, but the owning process or agent could not be
  reconciled after restart.

Freshness values:

- `Current`: the run still matches the active review target.
- `LongRunning`: the run has taken a while but still has evidence of being live
  and relevant.
- `Inactive`: the run has stopped showing activity without enough evidence to
  call it terminal.
- `Superseded`: a newer matching scope has replaced the run.
- `Obsolete`: the reviewed snapshot is no longer useful for the active head or
  epoch.
- `Lost`: restart reconciliation could not find the owning agent/process.
- `Unknown`: the store cannot yet prove a more specific freshness value.

Elapsed runtime alone must not make a run stale. A 30 minute review can be
healthy if its snapshot is still relevant and the owner is active. Staleness
comes from snapshot/head/epoch mismatch, supersession, obsolete scope, lost
ownership, or inactivity evidence.

## Lock, Epoch, And Snapshot Rules

- Acquire the shared review lock before sending a review or follow-up. If the
  lock is busy, adopt, skip, or defer instead of launching overlapping work.
- Ghost commits are taken with a snapshot epoch bump, so every controlled
  snapshot invalidates older review targets.
- Follow-up reviews compare the recorded snapshot epoch and base commit before
  continuing. If the epoch advanced, the base is no longer an ancestor, or the
  snapshot is identical to the last reviewed commit, the loop stops and
  recaptures only through harness policy.
- Git mutations must use shared helpers that bump the snapshot epoch. New
  mutation touchpoints must either call those helpers or bump the epoch
  immediately after success.
- Session worktree cleanup also coordinates through the shared lock. Successful
  cleanup bumps the epoch so stale review targets are invalidated.

## Duplicate, Supersede, And Cancellation Policy

The harness owns duplicate and cancellation decisions. The assistant may reason
from the ledger, but it should not directly cancel or relaunch reviews as a way
to enforce policy.

- Prefer adopting or reconnecting to an equivalent active run before launching a
  new one.
- Skip duplicate work when the diff fingerprint, scope hash, prompt policy, and
  target are equivalent enough that another run already covers the useful scope.
  Duplicate skips use the `duplicate_auto_review_scope` reason and may record a
  saved token estimate.
- Mark older useful scopes as `Superseded` when a newer run replaces them.
- Use `Obsolete` or stale target applicability for terminal findings that no
  longer match the active checkout; preserve the evidence, but do not surface it
  as current work.
- Cancel only for explicit user stop, superseded or obsolete scope, dead/lost
  process evidence, hard budget exhaustion, or proven duplicate policy. Do not
  cancel solely because elapsed runtime is high.
- Preserve terminal findings as evidence. They should be current, superseded,
  obsolete, dismissed, or archived; they should not vanish silently.

## Compact Ledger

The compact ledger is the assistant-facing summary. It is emitted only when
there is useful active, recent, actionable, or diagnostic state, and it is capped
by byte budget. Idle clean runs produce no ledger.

Ledger entries include active or applicable runs by default, using stable run ids
and compact fields such as status, freshness, target applicability, source,
branch, snapshot, age, last activity, model, reasoning effort, prompt estimate,
actual tokens when available, saved token estimate, elapsed bucket, finding
count, summary, and short finding digests.

The ledger intentionally separates:

- `freshness`: whether the run is current, long-running, inactive, superseded,
  obsolete, lost, or unknown.
- `target`: whether the run matches the active head, is an older snapshot, comes
  from a detached review worktree, or is unknown.

Known stale or detached terminal findings are normally suppressed from actionable
run listings. The ledger may still include diagnostics such as `suppressed_stale`
so the assistant knows evidence exists without treating it as work to apply.

Example:

```xml
<auto_review_ledger schema_version="1" max_bytes="2400">
Auto Review state for this repo. Listed runs are active or target-applicable by default; diagnostics may include stale/detached history that is not an instruction to re-review or fix. Treat run ids as stable references for future detail lookup with the auto_review_detail tool. Freshness describes run recency; target describes whether findings match the active checkout.
diagnostics recent_runs=2 in_flight=1 terminal=1 tokens=25915t token_runs=1 prompt_estimate=54000t prompt_runs=2 high_burn=2 longest_elapsed=lt1m
run id=... status=Reviewing freshness=Current target=matching_head source=Tui branch=feature snapshot=abcdef1 age=lt1m last_activity=lt1m model=gpt-5.4-mini reasoning=medium prompt_estimate=42000t elapsed=lt1m
</auto_review_ledger>
```

## Lazy Detail Lookup

Full review bodies live in output sidecars and are retrieved through bounded
detail lookup by `run_id` and optional `finding_id`. Detail lookup is read-only.
It returns either run detail or one finding's detail, includes metadata about
truncation and omitted findings, and enforces a hard byte cap.

Use the compact ledger first. Fetch details only when a listed finding or run is
relevant to the current task. Do not paste raw sidecar JSON or full review output
into ordinary context.

## Proof Metrics And Dogfood Diagnostics

Diagnostics are designed to prove whether Auto Review is saving work and
surfacing useful findings. The compact ledger can report recent counters without
listing bulky run details:

- lifecycle totals: `recent_runs`, `in_flight`, `terminal`, `failed`,
  `cancelled`, `lost`
- duplicate and supersession proof: `skipped`, `duplicate_skipped`,
  `superseded`, `saved_estimate`, `saved_runs`
- token and prompt cost: `tokens`, `token_runs`, `prompt_estimate`,
  `prompt_runs`, `high_burn`
- usefulness and freshness proof: `suppressed_stale`, plus per-run finding
  counts, elapsed buckets, summary digests, and stable finding ids

Dogfood sessions should use these signals to answer concrete questions:

- Did duplicate review launches decrease?
- How many tokens were actually spent or plausibly avoided?
- Did terminal findings surface while still current?
- Are stale, detached, superseded, cancelled, failed, and lost runs classified
  without polluting normal assistant context?
- Was latency caused by the first review pass, follow-up loops, lock/worktree
  contention, retries, prompt size, model choice, or reasoning effort?

## Extension Checklist

- Add new review entrypoints through the durable run store.
- Record lifecycle transitions with enough target, timing, model, token, and
  terminal-reason data to explain the run later.
- Use lock and epoch helpers for every review or git mutation path.
- Keep runtime separate from staleness; long-running can still be healthy.
- Preserve terminal evidence, but surface only bounded current/actionable state
  by default.
- Keep assistant-visible context compact. Add diagnostics or lazy detail instead
  of injecting raw review bodies.

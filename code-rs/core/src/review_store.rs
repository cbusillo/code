use crate::protocol::{ReviewFinding, ReviewOutputEvent};
use crate::review_coord::scoped_review_state_dir;
use crate::review_coord::scoped_review_state_dir_path;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;
use uuid::Uuid;

const AUTO_REVIEW_DIR: &str = "auto-review";
const RUNS_FILENAME: &str = "runs.json";
const OUTPUTS_DIR: &str = "outputs";
const SCHEMA_VERSION: u32 = 1;
const DEFAULT_MAX_RUNS: usize = 500;
const MAX_FINDING_DIGESTS: usize = 25;
const MAX_FINDING_DIGEST_TITLE_CHARS: usize = 160;
const AUTO_REVIEW_DETAIL_DEFAULT_MAX_BYTES: usize = 12_000;
const AUTO_REVIEW_DETAIL_HARD_MAX_BYTES: usize = 64_000;
const AUTO_REVIEW_DETAIL_MAX_FINDINGS: usize = 10;
const DEFAULT_LEDGER_MAX_BYTES: usize = 2_400;
const DEFAULT_LEDGER_MAX_RUNS: usize = 5;
const LEDGER_RECENT_ACTIONABLE_SECS: u64 = 24 * 60 * 60;
const LEDGER_IN_FLIGHT_ACTIVITY_SECS: u64 = 60 * 60;
const LEDGER_MAX_FINDINGS_PER_RUN: usize = 3;
const LEDGER_DIAGNOSTIC_RECENT_SECS: u64 = 24 * 60 * 60;
const LEDGER_HIGH_TOKEN_COUNT: u64 = 20_000;
const LEDGER_HIGH_PROMPT_ESTIMATE: u64 = 20_000;
const LEDGER_LONG_ELAPSED_SECS: u64 = 15 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoReviewRunSource {
    Tui,
    Exec,
    AutoDrive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoReviewRunStatus {
    Pending,
    Snapshotting,
    Reviewing,
    Resolving,
    Completed,
    Failed,
    Cancelled,
    Superseded,
    Skipped,
    Lost,
}

impl AutoReviewRunStatus {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            AutoReviewRunStatus::Completed
                | AutoReviewRunStatus::Failed
                | AutoReviewRunStatus::Cancelled
                | AutoReviewRunStatus::Superseded
                | AutoReviewRunStatus::Skipped
                | AutoReviewRunStatus::Lost
        )
    }

    pub fn is_in_flight(self) -> bool {
        !self.is_terminal()
    }

    pub fn is_adoptable_duplicate(self) -> bool {
        matches!(
            self,
            AutoReviewRunStatus::Reviewing | AutoReviewRunStatus::Resolving
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoReviewFreshness {
    Current,
    LongRunning,
    Inactive,
    Superseded,
    Obsolete,
    Lost,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoReviewTargetApplicability {
    MatchingHead,
    OlderSnapshot,
    DetachedReviewWorktree,
    Unknown,
}

impl AutoReviewTargetApplicability {
    fn as_ledger_value(self) -> &'static str {
        match self {
            AutoReviewTargetApplicability::MatchingHead => "matching_head",
            AutoReviewTargetApplicability::OlderSnapshot => "older_snapshot",
            AutoReviewTargetApplicability::DetachedReviewWorktree => "detached_review_worktree",
            AutoReviewTargetApplicability::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoReviewFindingDigest {
    pub finding_id: String,
    pub priority: i32,
    pub title: String,
    pub path: Option<PathBuf>,
    pub line_start: Option<u32>,
    pub line_end: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoReviewRun {
    pub schema_version: u32,
    pub run_id: Uuid,
    pub source: AutoReviewRunSource,
    pub status: AutoReviewRunStatus,
    pub freshness: AutoReviewFreshness,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_activity_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_session_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batch_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_epoch: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_at_launch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff_fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_policy_version: Option<String>,
    #[serde(default)]
    pub changed_path_count: usize,
    #[serde(default)]
    pub listed_paths: Vec<PathBuf>,
    #[serde(default)]
    pub omitted_path_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_token_estimate: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_token_estimate: Option<u64>,
    #[serde(default)]
    pub finding_count: usize,
    #[serde(default)]
    pub finding_digests: Vec<AutoReviewFindingDigest>,
    #[serde(default)]
    pub omitted_finding_digest_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancel_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_path: Option<PathBuf>,
}

impl AutoReviewRun {
    pub fn new(run_id: Uuid, source: AutoReviewRunSource, now: u64) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            run_id,
            source,
            status: AutoReviewRunStatus::Pending,
            freshness: AutoReviewFreshness::Unknown,
            created_at: now,
            updated_at: now,
            started_at: None,
            completed_at: None,
            last_activity_at: None,
            owner_session_id: None,
            agent_id: None,
            batch_id: None,
            branch: None,
            worktree_path: None,
            base_commit: None,
            snapshot_commit: None,
            snapshot_epoch: None,
            head_at_launch: None,
            scope_hash: None,
            diff_fingerprint: None,
            prompt_policy_version: None,
            changed_path_count: 0,
            listed_paths: Vec::new(),
            omitted_path_count: 0,
            model: None,
            reasoning_effort: None,
            prompt_token_estimate: None,
            token_count: None,
            saved_token_estimate: None,
            finding_count: 0,
            finding_digests: Vec::new(),
            omitted_finding_digest_count: 0,
            summary_digest: None,
            superseded_by: None,
            cancel_reason: None,
            error_class: None,
            error_summary: None,
            output_path: None,
        }
    }

    pub fn mark_status(&mut self, status: AutoReviewRunStatus, now: u64) {
        self.status = status;
        self.updated_at = now;
        if status.is_terminal() && self.completed_at.is_none() {
            self.completed_at = Some(now);
        }
    }

    pub fn mark_activity(&mut self, now: u64) {
        self.last_activity_at = Some(now);
        self.updated_at = now;
        if self.started_at.is_none() {
            self.started_at = Some(now);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoReviewDuplicateDisposition {
    Adopt,
    ReuseTerminal,
    SupersedeTerminal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoReviewDuplicateMatch {
    pub run_id: Uuid,
    pub status: AutoReviewRunStatus,
    pub disposition: AutoReviewDuplicateDisposition,
    pub agent_id: Option<String>,
    pub snapshot_commit: Option<String>,
    pub owner_session_id: Option<Uuid>,
    pub worktree_path: Option<PathBuf>,
    pub branch: Option<String>,
    pub finding_count: usize,
    pub summary_digest: Option<String>,
    pub token_count: Option<u64>,
    pub prompt_token_estimate: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AutoReviewDetailResponse {
    pub run_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finding_id: Option<String>,
    pub status: AutoReviewRunStatus,
    pub source: AutoReviewRunSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_commit: Option<String>,
    pub detail_kind: AutoReviewDetailKind,
    pub finding_count: usize,
    pub omitted_findings: usize,
    pub content: String,
    pub bytes: usize,
    pub original_bytes: usize,
    pub max_bytes: usize,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sidecar_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutoReviewDetailKind {
    Run,
    Finding,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutoReviewDetailErrorCode {
    InvalidFindingId,
    MissingFinding,
    MissingRun,
    MissingSidecar,
    InvalidSidecar,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AutoReviewDetailError {
    pub code: AutoReviewDetailErrorCode,
    pub run_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finding_id: Option<String>,
    pub message: String,
}

impl std::fmt::Display for AutoReviewDetailError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for AutoReviewDetailError {}

#[derive(Debug, Clone)]
pub struct AutoReviewLedgerOptions {
    pub max_bytes: usize,
    pub max_runs: usize,
    pub now: u64,
    pub active_branch: Option<String>,
    pub active_head: Option<String>,
}

impl AutoReviewLedgerOptions {
    pub fn new(now: u64) -> Self {
        Self {
            max_bytes: DEFAULT_LEDGER_MAX_BYTES,
            max_runs: DEFAULT_LEDGER_MAX_RUNS,
            now,
            active_branch: None,
            active_head: None,
        }
    }

    pub fn with_active_target(
        mut self,
        active_branch: Option<String>,
        active_head: Option<String>,
    ) -> Self {
        self.active_branch = active_branch.and_then(non_empty_string);
        self.active_head = active_head.and_then(non_empty_string);
        self
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AutoReviewRunsFile {
    schema_version: u32,
    runs: Vec<AutoReviewRun>,
}

pub struct AutoReviewRunStore {
    root: PathBuf,
    runs_path: PathBuf,
    outputs_dir: PathBuf,
    runs: BTreeMap<Uuid, AutoReviewRun>,
}

impl AutoReviewRunStore {
    pub fn open(scope: &Path) -> io::Result<Self> {
        Self::open_in_dir(auto_review_dir(scope)?)
    }

    pub fn open_existing(scope: &Path) -> io::Result<Option<Self>> {
        let root = auto_review_dir_path(scope)?;
        if !root.exists() {
            return Ok(None);
        }
        Self::open_in_dir(root).map(Some)
    }

    pub fn open_existing_read_only(scope: &Path) -> io::Result<Option<Self>> {
        let root = auto_review_dir_path(scope)?;
        if !root.exists() {
            return Ok(None);
        }
        let runs_path = root.join(RUNS_FILENAME);
        let runs = read_runs_file(&runs_path)?;
        Ok(Some(Self {
            outputs_dir: root.join(OUTPUTS_DIR),
            root,
            runs_path,
            runs,
        }))
    }

    pub fn open_in_dir(root: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(&root)?;
        let outputs_dir = root.join(OUTPUTS_DIR);
        fs::create_dir_all(&outputs_dir)?;
        let runs_path = root.join(RUNS_FILENAME);
        let runs = read_runs_file(&runs_path)?;
        let mut store = Self {
            root,
            runs_path,
            outputs_dir,
            runs,
        };
        if store.runs.len() > DEFAULT_MAX_RUNS {
            store.compact_in_place(DEFAULT_MAX_RUNS)?;
        }
        Ok(store)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn output_path_for(&self, run_id: Uuid) -> PathBuf {
        self.outputs_dir.join(format!("{run_id}.json"))
    }

    pub fn runs(&self) -> impl Iterator<Item = &AutoReviewRun> {
        self.runs.values()
    }

    pub fn get(&self, run_id: Uuid) -> Option<&AutoReviewRun> {
        self.runs.get(&run_id)
    }

    pub fn get_mut(&mut self, run_id: Uuid) -> Option<&mut AutoReviewRun> {
        self.runs.get_mut(&run_id)
    }

    pub fn upsert(&mut self, run: AutoReviewRun) -> io::Result<()> {
        self.runs.insert(run.run_id, run);
        self.save()
    }

    pub fn save(&mut self) -> io::Result<()> {
        self.merge_latest_from_disk()?;
        write_runs_file(&self.runs_path, self.runs.values())
    }

    pub fn compact(&mut self, max_runs: usize) -> io::Result<()> {
        self.compact_in_place(max_runs)
    }

    fn compact_in_place(&mut self, max_runs: usize) -> io::Result<()> {
        self.merge_latest_from_disk()?;
        let keep = most_recent_run_ids(self.runs.values(), max_runs);
        self.runs.retain(|run_id, _| keep.contains(run_id));
        write_runs_file(&self.runs_path, self.runs.values())
    }

    fn merge_latest_from_disk(&mut self) -> io::Result<()> {
        let latest = read_runs_file(&self.runs_path)?;
        for (run_id, run) in latest {
            self.runs
                .entry(run_id)
                .and_modify(|existing| {
                    if run_is_newer(&run, existing) {
                        *existing = run.clone();
                    }
                })
                .or_insert(run);
        }
        Ok(())
    }

    pub fn reconcile_orphaned_in_flight<I>(&mut self, live_agent_ids: I, now: u64) -> io::Result<usize>
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        let live_agent_ids = live_agent_ids
            .into_iter()
            .map(|agent_id| agent_id.as_ref().to_string())
            .collect::<std::collections::BTreeSet<_>>();
        let mut changed = 0usize;
        for run in self.runs.values_mut() {
            if !run.status.is_in_flight() {
                continue;
            }
            if run
                .agent_id
                .as_deref()
                .is_some_and(|agent_id| live_agent_ids.contains(agent_id))
            {
                continue;
            }
            run.mark_status(AutoReviewRunStatus::Lost, now);
            run.freshness = AutoReviewFreshness::Lost;
            run.cancel_reason = Some("agent_missing_after_restart".to_string());
            changed = changed.saturating_add(1);
        }
        if changed > 0 {
            self.save()?;
        }
        Ok(changed)
    }

    pub fn find_duplicate_by_fingerprint(
        &self,
        diff_fingerprint: &str,
    ) -> Option<AutoReviewDuplicateMatch> {
        self.find_duplicate_by_fingerprint_excluding(diff_fingerprint, None)
    }

    pub fn find_duplicate_by_fingerprint_excluding(
        &self,
        diff_fingerprint: &str,
        excluded_run_id: Option<Uuid>,
    ) -> Option<AutoReviewDuplicateMatch> {
        let fingerprint = diff_fingerprint.trim();
        if fingerprint.is_empty() {
            return None;
        }

        self.runs
            .values()
            .filter(|run| Some(run.run_id) != excluded_run_id)
            .filter(|run| run.diff_fingerprint.as_deref() == Some(fingerprint))
            .filter(|run| {
                !matches!(
                    run.status,
                    AutoReviewRunStatus::Lost
                        | AutoReviewRunStatus::Skipped
                        | AutoReviewRunStatus::Superseded
                )
            })
            .max_by(|left, right| {
                duplicate_priority(left)
                    .cmp(&duplicate_priority(right))
                    .then_with(|| left.updated_at.cmp(&right.updated_at))
                    .then_with(|| left.created_at.cmp(&right.created_at))
                    .then_with(|| left.run_id.cmp(&right.run_id))
            })
            .map(|run| AutoReviewDuplicateMatch {
                run_id: run.run_id,
                status: run.status,
                disposition: duplicate_disposition(run),
                agent_id: run.agent_id.clone(),
                snapshot_commit: run.snapshot_commit.clone(),
                owner_session_id: run.owner_session_id,
                worktree_path: run.worktree_path.clone(),
                branch: run.branch.clone().or_else(|| run.batch_id.clone()),
                finding_count: run.finding_count,
                summary_digest: run.summary_digest.clone(),
                token_count: run.token_count,
                prompt_token_estimate: run.prompt_token_estimate,
            })
    }

    pub fn mark_superseded_by_fingerprint(
        &mut self,
        diff_fingerprint: &str,
        superseded_by: Uuid,
        now: u64,
    ) -> io::Result<usize> {
        let fingerprint = diff_fingerprint.trim();
        if fingerprint.is_empty() {
            return Ok(0);
        }

        let mut changed = 0usize;
        for run in self.runs.values_mut() {
            if run.run_id == superseded_by
                || run.diff_fingerprint.as_deref() != Some(fingerprint)
                || run.status.is_in_flight()
                || run.status == AutoReviewRunStatus::Superseded
                || run.finding_count > 0
                || !run.finding_digests.is_empty()
                || run.error_class.is_some()
                || run.error_summary.is_some()
                || run.cancel_reason.is_some()
            {
                continue;
            }
            run.freshness = AutoReviewFreshness::Superseded;
            run.superseded_by = Some(superseded_by);
            run.mark_status(AutoReviewRunStatus::Superseded, now);
            changed = changed.saturating_add(1);
        }
        if changed > 0 {
            self.save()?;
        }
        Ok(changed)
    }

    pub fn mark_skipped_duplicate(
        &mut self,
        run_id: Uuid,
        duplicate_run_id: Uuid,
        diff_fingerprint: Option<&str>,
        saved_token_estimate: Option<u64>,
        now: u64,
    ) -> io::Result<()> {
        if let Some(run) = self.get_mut(run_id) {
            if let Some(diff_fingerprint) = diff_fingerprint.and_then(non_empty_str) {
                run.diff_fingerprint = Some(diff_fingerprint.to_string());
            }
            run.saved_token_estimate = saved_token_estimate;
            run.freshness = AutoReviewFreshness::Superseded;
            run.superseded_by = Some(duplicate_run_id);
            run.cancel_reason = Some("duplicate_auto_review_scope".to_string());
            run.mark_status(AutoReviewRunStatus::Skipped, now);
        }
        self.save()
    }

    pub fn write_output(&mut self, run_id: Uuid, output: &ReviewOutputEvent) -> io::Result<PathBuf> {
        if !self.runs.contains_key(&run_id) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("auto review run {run_id} is not recorded"),
            ));
        }
        let output_path = self.output_path_for(run_id);
        write_json_file(&output_path, output)?;
        let run = self
            .runs
            .get_mut(&run_id)
            .expect("run existence checked before sidecar write");
        run.output_path = Some(output_path.clone());
        run.finding_count = output.findings.len();
        run.finding_digests = finding_digests(output);
        run.omitted_finding_digest_count = output
            .findings
            .len()
            .saturating_sub(run.finding_digests.len());
        run.summary_digest = summarize(&output.overall_explanation, 240);
        self.save()?;
        Ok(output_path)
    }

    pub fn read_output(&self, run_id: Uuid) -> io::Result<ReviewOutputEvent> {
        let path = self
            .runs
            .get(&run_id)
            .and_then(|run| run.output_path.clone())
            .unwrap_or_else(|| self.output_path_for(run_id));
        let text = fs::read_to_string(path)?;
        serde_json::from_str(&text).map_err(io::Error::other)
    }

    pub fn read_detail(
        &self,
        run_id: Uuid,
        finding_id: Option<&str>,
        max_bytes: Option<usize>,
    ) -> Result<AutoReviewDetailResponse, AutoReviewDetailError> {
        let run = self.runs.get(&run_id).ok_or_else(|| AutoReviewDetailError {
            code: AutoReviewDetailErrorCode::MissingRun,
            run_id,
            finding_id: finding_id.map(str::to_string),
            message: format!("auto review run {run_id} was not found in the durable run ledger"),
        })?;
        let sidecar_path = run
            .output_path
            .clone()
            .unwrap_or_else(|| self.output_path_for(run_id));
        let text = fs::read_to_string(&sidecar_path).map_err(|err| AutoReviewDetailError {
            code: if err.kind() == io::ErrorKind::NotFound {
                AutoReviewDetailErrorCode::MissingSidecar
            } else {
                AutoReviewDetailErrorCode::InvalidSidecar
            },
            run_id,
            finding_id: finding_id.map(str::to_string),
            message: if err.kind() == io::ErrorKind::NotFound {
                format!(
                    "auto review detail sidecar for run {run_id} is not available at {}",
                    sidecar_path.display()
                )
            } else {
                format!(
                    "failed to read auto review detail sidecar for run {run_id} at {}: {err}",
                    sidecar_path.display()
                )
            },
        })?;
        let output = serde_json::from_str::<ReviewOutputEvent>(&text).map_err(|err| {
            AutoReviewDetailError {
                code: AutoReviewDetailErrorCode::InvalidSidecar,
                run_id,
                finding_id: finding_id.map(str::to_string),
                message: format!(
                    "failed to parse auto review detail sidecar for run {run_id} at {}: {err}",
                    sidecar_path.display()
                ),
            }
        })?;
        let max_bytes = normalize_detail_max_bytes(max_bytes);
        let detail_kind = if finding_id.is_some() {
            AutoReviewDetailKind::Finding
        } else {
            AutoReviewDetailKind::Run
        };
        let content = match finding_id {
            Some(finding_id) => {
                let index = parse_finding_id(finding_id).ok_or_else(|| AutoReviewDetailError {
                    code: AutoReviewDetailErrorCode::InvalidFindingId,
                    run_id,
                    finding_id: Some(finding_id.to_string()),
                    message: format!(
                        "invalid auto review finding id {finding_id:?}; expected a stable id like f1"
                    ),
                })?;
                let finding = output.findings.get(index).ok_or_else(|| AutoReviewDetailError {
                    code: AutoReviewDetailErrorCode::MissingFinding,
                    run_id,
                    finding_id: Some(finding_id.to_string()),
                    message: format!(
                        "auto review finding {finding_id} was not found for run {run_id}; this run has {} finding(s)",
                        output.findings.len()
                    ),
                })?;
                format_finding_detail(finding_id, finding)
            }
            None => format_run_detail(&output),
        };
        let original_bytes = content.len();
        let (content, truncated) = truncate_detail_content(content, max_bytes);
        let bytes = content.len();
        let omitted_findings = match finding_id {
            Some(_) => output.findings.len().saturating_sub(1),
            None => output.findings.len().saturating_sub(AUTO_REVIEW_DETAIL_MAX_FINDINGS),
        };
        let findings_capped = finding_id.is_none() && omitted_findings > 0;
        Ok(AutoReviewDetailResponse {
            run_id,
            finding_id: finding_id.map(str::to_string),
            status: run.status,
            source: run.source,
            branch: run.branch.clone(),
            snapshot_commit: run.snapshot_commit.clone(),
            detail_kind,
            finding_count: output.findings.len(),
            omitted_findings,
            content,
            bytes,
            original_bytes,
            max_bytes,
            truncated: truncated || findings_capped,
            sidecar_path: Some(sidecar_path),
        })
    }

    pub fn compact_ledger(&self, options: AutoReviewLedgerOptions) -> Option<String> {
        compact_ledger_from_runs(self.runs.values(), options)
    }
}

pub fn default_auto_review_detail_max_bytes() -> usize {
    AUTO_REVIEW_DETAIL_DEFAULT_MAX_BYTES
}

pub fn hard_auto_review_detail_max_bytes() -> usize {
    AUTO_REVIEW_DETAIL_HARD_MAX_BYTES
}

fn normalize_detail_max_bytes(max_bytes: Option<usize>) -> usize {
    max_bytes
        .unwrap_or(AUTO_REVIEW_DETAIL_DEFAULT_MAX_BYTES)
        .clamp(1, AUTO_REVIEW_DETAIL_HARD_MAX_BYTES)
}

fn parse_finding_id(finding_id: &str) -> Option<usize> {
    let number = finding_id.strip_prefix('f')?.parse::<usize>().ok()?;
    number.checked_sub(1)
}

fn format_run_detail(output: &ReviewOutputEvent) -> String {
    let mut sections = Vec::new();
    sections.push(format!(
        "overall_correctness={} confidence={}",
        output.overall_correctness, output.overall_confidence_score
    ));
    if !output.overall_explanation.trim().is_empty() {
        sections.push(format!(
            "overall_explanation:\n{}",
            output.overall_explanation.trim()
        ));
    }
    if output.findings.is_empty() {
        sections.push("findings: none".to_string());
    } else {
        let mut findings = String::from("findings:");
        for (idx, finding) in output
            .findings
            .iter()
            .take(AUTO_REVIEW_DETAIL_MAX_FINDINGS)
            .enumerate()
        {
            let finding_id = format!("f{}", idx + 1);
            findings.push('\n');
            findings.push_str(&format_finding_detail(&finding_id, finding));
        }
        if output.findings.len() > AUTO_REVIEW_DETAIL_MAX_FINDINGS {
            findings.push_str(&format!(
                "\n... omitted {} additional finding(s); request a specific finding_id for full detail",
                output.findings.len() - AUTO_REVIEW_DETAIL_MAX_FINDINGS
            ));
        }
        sections.push(findings);
    }
    sections.join("\n\n")
}

fn format_finding_detail(finding_id: &str, finding: &ReviewFinding) -> String {
    format!(
        "finding_id={finding_id} priority={} confidence={} location={}:{}-{}\ntitle: {}\nbody:\n{}",
        finding.priority,
        finding.confidence_score,
        finding.code_location.absolute_file_path.display(),
        finding.code_location.line_range.start,
        finding.code_location.line_range.end,
        finding.title.trim(),
        finding.body.trim()
    )
}

fn truncate_detail_content(content: String, max_bytes: usize) -> (String, bool) {
    if content.len() <= max_bytes {
        return (content, false);
    }
    let marker = "\n... truncated auto review detail ...\n";
    if max_bytes <= marker.len() {
        return (marker.chars().take(max_bytes).collect(), true);
    }
    let keep = max_bytes - marker.len();
    let head_budget = keep / 2;
    let tail_budget = keep - head_budget;
    let head_end = floor_char_boundary(&content, head_budget);
    let tail_start = ceil_char_boundary(&content, content.len().saturating_sub(tail_budget));
    let mut truncated = String::new();
    truncated.push_str(&content[..head_end]);
    truncated.push_str(marker);
    truncated.push_str(&content[tail_start..]);
    (truncated, true)
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn ceil_char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

fn duplicate_priority(run: &AutoReviewRun) -> u8 {
    if run.status.is_adoptable_duplicate() {
        return 4;
    }
    if run.finding_count > 0 || !run.finding_digests.is_empty() {
        return 3;
    }
    if run.status == AutoReviewRunStatus::Completed {
        return 2;
    }
    1
}

fn duplicate_disposition(run: &AutoReviewRun) -> AutoReviewDuplicateDisposition {
    if run.status.is_adoptable_duplicate() {
        AutoReviewDuplicateDisposition::Adopt
    } else if run.status == AutoReviewRunStatus::Completed {
        AutoReviewDuplicateDisposition::ReuseTerminal
    } else {
        AutoReviewDuplicateDisposition::SupersedeTerminal
    }
}

pub fn auto_review_dir(scope: &Path) -> io::Result<PathBuf> {
    Ok(scoped_review_state_dir(scope)?.join(AUTO_REVIEW_DIR))
}

pub fn auto_review_dir_path(scope: &Path) -> io::Result<PathBuf> {
    Ok(scoped_review_state_dir_path(scope)?.join(AUTO_REVIEW_DIR))
}

fn read_runs_file(path: &Path) -> io::Result<BTreeMap<Uuid, AutoReviewRun>> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let text = fs::read_to_string(path)?;
    if text.trim().is_empty() {
        return Ok(BTreeMap::new());
    }
    let file: AutoReviewRunsFile = serde_json::from_str(&text).map_err(io::Error::other)?;
    Ok(file.runs.into_iter().map(|run| (run.run_id, run)).collect())
}

fn write_runs_file<'a, I>(path: &Path, runs: I) -> io::Result<()>
where
    I: IntoIterator<Item = &'a AutoReviewRun>,
{
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("runs path has no parent"))?;
    fs::create_dir_all(parent)?;
    let mut runs = runs.into_iter().cloned().collect::<Vec<_>>();
    runs.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.run_id.cmp(&right.run_id))
    });
    let file = AutoReviewRunsFile {
        schema_version: SCHEMA_VERSION,
        runs,
    };
    write_json_file(path, &file)
}

fn write_json_file<T>(path: &Path, value: &T) -> io::Result<()>
where
    T: Serialize + ?Sized,
{
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("json path has no parent"))?;
    fs::create_dir_all(parent)?;
    let mut temp = NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(&mut temp, value).map_err(io::Error::other)?;
    temp.write_all(b"\n")?;
    temp.persist(path)
        .map_err(|err| io::Error::new(err.error.kind(), err.error))?;
    Ok(())
}

fn run_is_newer(candidate: &AutoReviewRun, existing: &AutoReviewRun) -> bool {
    candidate
        .updated_at
        .cmp(&existing.updated_at)
        .then_with(|| candidate.created_at.cmp(&existing.created_at))
        .then_with(|| candidate.run_id.cmp(&existing.run_id))
        .is_gt()
}

fn most_recent_run_ids<'a, I>(runs: I, max_runs: usize) -> std::collections::BTreeSet<Uuid>
where
    I: IntoIterator<Item = &'a AutoReviewRun>,
{
    let mut runs = runs.into_iter().collect::<Vec<_>>();
    runs.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| right.created_at.cmp(&left.created_at))
            .then_with(|| left.run_id.cmp(&right.run_id))
    });
    runs.truncate(max_runs);
    runs.into_iter().map(|run| run.run_id).collect()
}

fn finding_digests(output: &ReviewOutputEvent) -> Vec<AutoReviewFindingDigest> {
    output
        .findings
        .iter()
        .take(MAX_FINDING_DIGESTS)
        .enumerate()
        .map(|(idx, finding)| AutoReviewFindingDigest {
            finding_id: format!("f{}", idx + 1),
            priority: finding.priority,
            title: summarize(&finding.title, MAX_FINDING_DIGEST_TITLE_CHARS).unwrap_or_default(),
            path: Some(finding.code_location.absolute_file_path.clone()),
            line_start: Some(finding.code_location.line_range.start),
            line_end: Some(finding.code_location.line_range.end),
        })
        .collect()
}

fn compact_ledger_from_runs<'a, I>(
    runs: I,
    options: AutoReviewLedgerOptions,
) -> Option<String>
where
    I: IntoIterator<Item = &'a AutoReviewRun>,
{
    let max_bytes = options.max_bytes.max(64);
    let all_runs = runs.into_iter().collect::<Vec<_>>();
    let mut selected = all_runs
        .iter()
        .copied()
        .filter(|run| run_is_ledger_actionable(run, &options))
        .collect::<Vec<_>>();
    let diagnostics = build_ledger_diagnostics(all_runs.iter().copied(), &options);
    if selected.is_empty() && diagnostics.is_none() {
        return None;
    }

    selected.sort_by(|left, right| {
        ledger_priority(right)
            .cmp(&ledger_priority(left))
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| right.created_at.cmp(&left.created_at))
            .then_with(|| right.run_id.cmp(&left.run_id))
    });
    selected.truncate(options.max_runs.max(1));

    let mut lines = Vec::new();
    lines.push(format!(
        "<auto_review_ledger schema_version=\"1\" max_bytes=\"{max_bytes}\">"
    ));
    lines.push("Auto Review state for this repo. Listed runs are active or target-applicable by default; diagnostics may include stale/detached history that is not an instruction to re-review or fix. Treat run ids as stable references for future detail lookup with the auto_review_detail tool. Freshness describes run recency; target describes whether findings match the active checkout.".to_string());
    if let Some(diagnostics) = diagnostics {
        lines.push(diagnostics);
    }
    for run in selected {
        append_ledger_run(&mut lines, run, &options);
    }
    lines.push("</auto_review_ledger>".to_string());

    let mut ledger = lines.join("\n");
    if ledger.len() > max_bytes {
        truncate_ledger_to_bytes(&mut ledger, max_bytes);
    }
    Some(ledger)
}

pub fn auto_review_target_applicability(
    run: &AutoReviewRun,
    active_branch: Option<&str>,
    active_head: Option<&str>,
) -> AutoReviewTargetApplicability {
    let active_head = active_head.and_then(non_empty_str);
    let active_branch = active_branch.and_then(non_empty_str);
    let snapshot = run.snapshot_commit.as_deref().and_then(non_empty_str);
    let head_at_launch = run.head_at_launch.as_deref().and_then(non_empty_str);

    if let (Some(snapshot), Some(active_head)) = (snapshot, active_head) {
        if same_commit_prefix(snapshot, active_head) {
            return AutoReviewTargetApplicability::MatchingHead;
        }
    }
    if let (Some(head_at_launch), Some(active_head)) = (head_at_launch, active_head) {
        if same_commit_prefix(head_at_launch, active_head) {
            return AutoReviewTargetApplicability::MatchingHead;
        }
    }

    let review_branch = run.branch.as_deref().and_then(non_empty_str);
    let detached_review_branch = review_branch.is_some_and(is_auto_review_branch);
    if active_head.is_some() && detached_review_branch && review_branch != active_branch {
        return AutoReviewTargetApplicability::DetachedReviewWorktree;
    }

    if snapshot.is_some() && active_head.is_some() {
        return AutoReviewTargetApplicability::OlderSnapshot;
    }
    if head_at_launch.is_some() && active_head.is_some() {
        return AutoReviewTargetApplicability::OlderSnapshot;
    }

    AutoReviewTargetApplicability::Unknown
}

fn run_is_ledger_actionable(run: &AutoReviewRun, options: &AutoReviewLedgerOptions) -> bool {
    let now = options.now;
    if run.status.is_in_flight() {
        let reference_time = run.last_activity_at.unwrap_or(run.updated_at);
        return now.saturating_sub(reference_time) <= LEDGER_IN_FLIGHT_ACTIVITY_SECS
            || matches!(run.freshness, AutoReviewFreshness::Current | AutoReviewFreshness::LongRunning);
    }
    if matches!(
        run.status,
        AutoReviewRunStatus::Lost | AutoReviewRunStatus::Superseded | AutoReviewRunStatus::Skipped
    ) {
        return false;
    }
    let has_error_detail = run.error_summary.is_some() || run.error_class.is_some();
    let terminal_actionable = run.finding_count > 0
        || !run.finding_digests.is_empty()
        || has_error_detail;
    if !terminal_actionable {
        return false;
    }
    if run_has_known_stale_target(run, options) {
        return false;
    }
    let reference_time = run.completed_at.or(run.last_activity_at).unwrap_or(run.updated_at);
    now.saturating_sub(reference_time) <= LEDGER_RECENT_ACTIONABLE_SECS
}

fn run_target_applicability(
    run: &AutoReviewRun,
    options: &AutoReviewLedgerOptions,
) -> AutoReviewTargetApplicability {
    auto_review_target_applicability(
        run,
        options.active_branch.as_deref(),
        options.active_head.as_deref(),
    )
}

fn target_is_known_stale(target: AutoReviewTargetApplicability) -> bool {
    matches!(
        target,
        AutoReviewTargetApplicability::OlderSnapshot
            | AutoReviewTargetApplicability::DetachedReviewWorktree
    )
}

fn run_has_known_stale_target(run: &AutoReviewRun, options: &AutoReviewLedgerOptions) -> bool {
    !run.status.is_in_flight() && target_is_known_stale(run_target_applicability(run, options))
}

fn run_has_findings(run: &AutoReviewRun) -> bool {
    run.finding_count > 0 || !run.finding_digests.is_empty()
}

#[derive(Default)]
struct LedgerDiagnostics {
    recent_runs: usize,
    in_flight_runs: usize,
    terminal_runs: usize,
    token_count: u64,
    token_runs: usize,
    prompt_token_estimate: u64,
    prompt_runs: usize,
    saved_token_estimate: u64,
    saved_runs: usize,
    high_burn_runs: usize,
    suppressed_stale_runs: usize,
    longest_elapsed_bucket: Option<&'static str>,
}

fn build_ledger_diagnostics<'a, I>(runs: I, options: &AutoReviewLedgerOptions) -> Option<String>
where
    I: IntoIterator<Item = &'a AutoReviewRun>,
{
    let mut diagnostics = LedgerDiagnostics::default();
    let now = options.now;

    for run in runs {
        if !run_is_recent_for_diagnostics(run, now) {
            continue;
        }

        // Only summarize fields that the durable run store already has. Actual
        // provider tokens appear once the agent status path persists them;
        // prompt estimates and elapsed time are available earlier.
        let target = run_target_applicability(run, options);
        let suppresses_stale_findings =
            !run.status.is_in_flight() && target_is_known_stale(target) && run_has_findings(run);
        if suppresses_stale_findings {
            diagnostics.suppressed_stale_runs += 1;
        }
        let has_saved_signal = run.saved_token_estimate.is_some();
        let has_cost_signal = run.token_count.is_some()
            || run.prompt_token_estimate.is_some()
            || has_saved_signal;
        let elapsed_secs = run_elapsed_secs(run, now);
        let has_timing_signal = run_has_long_elapsed_signal(run, elapsed_secs);
        let high_burn = run_has_high_burn_signal(run, elapsed_secs);
        if !has_cost_signal
            && !has_timing_signal
            && !high_burn
            && !suppresses_stale_findings
        {
            continue;
        }

        diagnostics.recent_runs += 1;
        if run.status.is_in_flight() {
            diagnostics.in_flight_runs += 1;
        } else {
            diagnostics.terminal_runs += 1;
        }
        if let Some(token_count) = run.token_count {
            diagnostics.token_count = diagnostics.token_count.saturating_add(token_count);
            diagnostics.token_runs += 1;
        }
        if let Some(prompt_token_estimate) = run.prompt_token_estimate {
            diagnostics.prompt_token_estimate = diagnostics
                .prompt_token_estimate
                .saturating_add(prompt_token_estimate);
            diagnostics.prompt_runs += 1;
        }
        if let Some(saved_token_estimate) = run.saved_token_estimate {
            diagnostics.saved_token_estimate = diagnostics
                .saved_token_estimate
                .saturating_add(saved_token_estimate);
            diagnostics.saved_runs += 1;
        }
        if high_burn {
            diagnostics.high_burn_runs += 1;
        }
        if let Some(elapsed_secs) = elapsed_secs {
            let bucket = duration_bucket(elapsed_secs);
            diagnostics.longest_elapsed_bucket = Some(match diagnostics.longest_elapsed_bucket {
                Some(existing) if duration_bucket_rank(existing) >= duration_bucket_rank(bucket) => {
                    existing
                }
                _ => bucket,
            });
        }
    }

    if diagnostics.recent_runs == 0 {
        return None;
    }

    let mut line = format!(
        "diagnostics recent_runs={} in_flight={} terminal={}",
        diagnostics.recent_runs, diagnostics.in_flight_runs, diagnostics.terminal_runs
    );
    if diagnostics.token_runs > 0 {
        line.push_str(&format!(
            " tokens={}t token_runs={}",
            diagnostics.token_count, diagnostics.token_runs
        ));
    }
    if diagnostics.prompt_runs > 0 {
        line.push_str(&format!(
            " prompt_estimate={}t prompt_runs={}",
            diagnostics.prompt_token_estimate, diagnostics.prompt_runs
        ));
    }
    if diagnostics.high_burn_runs > 0 {
        line.push_str(&format!(" high_burn={}", diagnostics.high_burn_runs));
    }
    if diagnostics.suppressed_stale_runs > 0 {
        line.push_str(&format!(
            " suppressed_stale={}",
            diagnostics.suppressed_stale_runs
        ));
    }
    if diagnostics.saved_runs > 0 {
        line.push_str(&format!(
            " saved_estimate={}t saved_runs={}",
            diagnostics.saved_token_estimate, diagnostics.saved_runs
        ));
    }
    if let Some(longest_elapsed_bucket) = diagnostics.longest_elapsed_bucket {
        line.push_str(&format!(" longest_elapsed={longest_elapsed_bucket}"));
    }
    Some(line)
}

fn duration_bucket(seconds: u64) -> &'static str {
    match seconds {
        0..=59 => "lt1m",
        60..=299 => "lt5m",
        300..=899 => "lt15m",
        900..=1799 => "lt30m",
        1800..=3599 => "lt1h",
        3600..=7199 => "lt2h",
        _ => "gte2h",
    }
}

fn duration_bucket_rank(bucket: &str) -> u8 {
    match bucket {
        "lt1m" => 0,
        "lt5m" => 1,
        "lt15m" => 2,
        "lt30m" => 3,
        "lt1h" => 4,
        "lt2h" => 5,
        "gte2h" => 6,
        _ => 0,
    }
}

fn run_is_recent_for_diagnostics(run: &AutoReviewRun, now: u64) -> bool {
    let reference_time = run
        .completed_at
        .or(run.last_activity_at)
        .unwrap_or(run.updated_at);
    now.saturating_sub(reference_time) <= LEDGER_DIAGNOSTIC_RECENT_SECS
}

fn run_elapsed_secs(run: &AutoReviewRun, now: u64) -> Option<u64> {
    let start = run.started_at.or(run.last_activity_at).or(Some(run.created_at))?;
    let end = run
        .completed_at
        .or_else(|| run.status.is_in_flight().then_some(now))
        .unwrap_or(run.updated_at);
    Some(end.saturating_sub(start))
}

fn run_has_high_burn_signal(run: &AutoReviewRun, elapsed_secs: Option<u64>) -> bool {
    run.token_count
        .is_some_and(|token_count| token_count >= LEDGER_HIGH_TOKEN_COUNT)
        || run
            .prompt_token_estimate
            .is_some_and(|estimate| estimate >= LEDGER_HIGH_PROMPT_ESTIMATE)
        || run_has_long_elapsed_signal(run, elapsed_secs)
}

fn run_has_long_elapsed_signal(run: &AutoReviewRun, elapsed_secs: Option<u64>) -> bool {
    elapsed_secs.is_some_and(|elapsed| elapsed >= LEDGER_LONG_ELAPSED_SECS)
        && (!run.status.is_in_flight()
            || matches!(
                run.freshness,
                AutoReviewFreshness::Current | AutoReviewFreshness::LongRunning
            ))
}

fn ledger_priority(run: &AutoReviewRun) -> u8 {
    if run.status.is_in_flight() {
        return 5;
    }
    if run.finding_count > 0 || !run.finding_digests.is_empty() {
        return 4;
    }
    if matches!(run.status, AutoReviewRunStatus::Failed | AutoReviewRunStatus::Cancelled) {
        return 3;
    }
    1
}

fn append_ledger_run(lines: &mut Vec<String>, run: &AutoReviewRun, options: &AutoReviewLedgerOptions) {
    let now = options.now;
    let age_secs = now.saturating_sub(run.updated_at);
    let activity_age_secs = run
        .last_activity_at
        .map(|last_activity| now.saturating_sub(last_activity));
    let snapshot = run
        .snapshot_commit
        .as_deref()
        .and_then(short_sha)
        .unwrap_or("unknown");
    let branch = run.branch.as_deref().or(run.batch_id.as_deref()).unwrap_or("unknown");
    let target = run_target_applicability(run, options);
    let mut line = format!(
        "run id={} status={:?} freshness={:?} target={} source={:?} branch={} snapshot={} age={}",
        run.run_id,
        run.status,
        run.freshness,
        target.as_ledger_value(),
        run.source,
        branch,
        snapshot,
        duration_bucket(age_secs)
    );
    if let Some(activity_age_secs) = activity_age_secs {
        line.push_str(&format!(
            " last_activity={}",
            duration_bucket(activity_age_secs)
        ));
    }
    if let Some(agent_id) = run.agent_id.as_deref().and_then(short_agent_id) {
        line.push_str(&format!(" agent={agent_id}"));
    }
    if let Some(model) = run.model.as_deref() {
        line.push_str(" model=");
        line.push_str(&single_line(model, 40));
    }
    if let Some(reasoning_effort) = run.reasoning_effort.as_deref() {
        line.push_str(" reasoning=");
        line.push_str(&single_line(reasoning_effort, 24));
    }
    if let Some(prompt_token_estimate) = run.prompt_token_estimate {
        line.push_str(&format!(" prompt_estimate={}t", prompt_token_estimate));
    }
    if let Some(token_count) = run.token_count {
        line.push_str(&format!(" tokens={}t", token_count));
    }
    if let Some(saved_token_estimate) = run.saved_token_estimate {
        line.push_str(&format!(" saved_estimate={}t", saved_token_estimate));
    }
    if let Some(elapsed_secs) = run_elapsed_secs(run, now) {
        line.push_str(&format!(" elapsed={}", duration_bucket(elapsed_secs)));
    }
    if run.finding_count > 0 {
        line.push_str(&format!(" findings={}", run.finding_count));
    }
    if run.omitted_finding_digest_count > 0 {
        line.push_str(&format!(" omitted_findings={}", run.omitted_finding_digest_count));
    }
    if let Some(summary) = run.summary_digest.as_deref() {
        line.push_str(" summary=");
        line.push_str(&single_line(summary, 180));
    }
    if let Some(error) = run.error_summary.as_deref() {
        line.push_str(" error=");
        line.push_str(&single_line(error, 180));
    }
    lines.push(line);

    for finding in run.finding_digests.iter().take(LEDGER_MAX_FINDINGS_PER_RUN) {
        let location = finding
            .path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let line_start = finding
            .line_start
            .map(|line| line.to_string())
            .unwrap_or_else(|| "?".to_string());
        lines.push(format!(
            "  finding id={} priority={} location={}:{} title={}",
            finding.finding_id,
            finding.priority,
            location,
            line_start,
            single_line(&finding.title, 160)
        ));
    }
    if run.finding_digests.len() > LEDGER_MAX_FINDINGS_PER_RUN {
        lines.push(format!(
            "  more_findings={} full_output=lazy",
            run.finding_digests.len() - LEDGER_MAX_FINDINGS_PER_RUN
        ));
    }
}

fn short_sha(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(&value[..value.len().min(12)])
    }
}

fn short_agent_id(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(&value[..value.len().min(8)])
    }
}

fn non_empty_string(value: String) -> Option<String> {
    let value = value.trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn non_empty_str(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn is_auto_review_branch(value: &str) -> bool {
    value == "auto-review" || value.starts_with("auto-review-")
}

fn same_commit_prefix(left: &str, right: &str) -> bool {
    let left = left.trim();
    let right = right.trim();
    if !left.is_ascii() || !right.is_ascii() {
        return false;
    }
    let min_len = left.len().min(right.len());
    min_len >= 7 && left[..min_len].eq_ignore_ascii_case(&right[..min_len])
}

fn single_line(value: &str, max_chars: usize) -> String {
    let flattened = value.split_whitespace().collect::<Vec<_>>().join(" ");
    summarize(&flattened, max_chars).unwrap_or_default()
}

fn truncate_ledger_to_bytes(ledger: &mut String, max_bytes: usize) {
    let marker = "\n<truncated />\n</auto_review_ledger>";
    if max_bytes <= marker.len() {
        ledger.clear();
        ledger.push_str(&marker[..max_bytes.min(marker.len())]);
        return;
    }
    let target = max_bytes - marker.len();
    let mut cutoff = 0usize;
    for (idx, _) in ledger.char_indices() {
        if idx > target {
            break;
        }
        cutoff = idx;
    }
    ledger.truncate(cutoff);
    ledger.push_str(marker);
}

fn summarize(text: &str, max_chars: usize) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let mut chars = text.chars();
    let mut out = String::new();
    for _ in 0..max_chars {
        let Some(ch) = chars.next() else {
            return Some(out);
        };
        out.push(ch);
    }
    if chars.next().is_some() {
        out.push_str("...");
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ReviewCodeLocation, ReviewFinding, ReviewLineRange};
    use serial_test::serial;
    use tempfile::TempDir;

    fn set_code_home(path: &Path) {
        // SAFETY: tests run serially and isolate CODE_HOME within a temp dir per test.
        unsafe { std::env::set_var("CODE_HOME", path); }
    }

    fn review_output() -> ReviewOutputEvent {
        ReviewOutputEvent {
            findings: vec![ReviewFinding {
                title: "finding title".to_string(),
                body: "finding body".to_string(),
                confidence_score: 0.9,
                priority: 2,
                code_location: ReviewCodeLocation {
                    absolute_file_path: PathBuf::from("/repo/src/lib.rs"),
                    line_range: ReviewLineRange { start: 7, end: 9 },
                },
            }],
            overall_correctness: "incorrect".to_string(),
            overall_explanation: "summary".to_string(),
            overall_confidence_score: 0.8,
        }
    }

    fn review_finding(idx: usize, body: &str) -> ReviewFinding {
        ReviewFinding {
            title: format!("finding title {idx}"),
            body: body.to_string(),
            confidence_score: 0.9,
            priority: idx as i32,
            code_location: ReviewCodeLocation {
                absolute_file_path: PathBuf::from(format!("/repo/src/file{idx}.rs")),
                line_range: ReviewLineRange {
                    start: idx as u32,
                    end: idx as u32 + 1,
                },
            },
        }
    }

    fn finding_digest(idx: usize) -> AutoReviewFindingDigest {
        AutoReviewFindingDigest {
            finding_id: format!("f{idx}"),
            priority: 2,
            title: format!("finding title {idx}"),
            path: Some(PathBuf::from(format!("/repo/src/file{idx}.rs"))),
            line_start: Some(idx as u32),
            line_end: Some(idx as u32 + 1),
        }
    }

    fn ledger_options(now: u64) -> AutoReviewLedgerOptions {
        AutoReviewLedgerOptions {
            now,
            max_bytes: 2_400,
            max_runs: 5,
            active_branch: None,
            active_head: None,
        }
    }

    #[test]
    #[serial]
    fn run_store_persists_and_loads_runs() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let run_id = Uuid::new_v4();
        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();
        let mut run = AutoReviewRun::new(run_id, AutoReviewRunSource::Tui, 10);
        run.agent_id = Some("agent-1".to_string());
        run.mark_activity(12);
        run.mark_status(AutoReviewRunStatus::Reviewing, 13);
        store.upsert(run).unwrap();

        let loaded = AutoReviewRunStore::open(repo.path()).unwrap();
        let loaded_run = loaded.get(run_id).expect("run loaded");
        assert_eq!(loaded_run.agent_id.as_deref(), Some("agent-1"));
        assert_eq!(loaded_run.last_activity_at, Some(12));
        assert_eq!(loaded_run.status, AutoReviewRunStatus::Reviewing);
    }

    #[test]
    #[serial]
    fn run_store_writes_and_reads_review_output_sidecar() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let run_id = Uuid::new_v4();
        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();
        store
            .upsert(AutoReviewRun::new(run_id, AutoReviewRunSource::Exec, 1))
            .unwrap();

        let output = review_output();
        let output_path = store.write_output(run_id, &output).unwrap();
        assert!(output_path.exists());
        let loaded_output = store.read_output(run_id).unwrap();
        assert_eq!(loaded_output, output);

        let loaded_store = AutoReviewRunStore::open(repo.path()).unwrap();
        let run = loaded_store.get(run_id).expect("run loaded");
        assert_eq!(run.finding_count, 1);
        assert_eq!(run.finding_digests[0].finding_id, "f1");
        assert_eq!(run.summary_digest.as_deref(), Some("summary"));
    }

    #[test]
    #[serial]
    fn detail_lookup_returns_bounded_run_detail() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let run_id = Uuid::new_v4();
        let mut run = AutoReviewRun::new(run_id, AutoReviewRunSource::Tui, 1);
        run.branch = Some("feature".to_string());
        run.snapshot_commit = Some("abcdef1234567890".to_string());
        run.mark_status(AutoReviewRunStatus::Completed, 2);
        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();
        store.upsert(run).unwrap();
        store.write_output(run_id, &review_output()).unwrap();

        let detail = store.read_detail(run_id, None, Some(2_000)).unwrap();

        assert_eq!(detail.run_id, run_id);
        assert_eq!(detail.detail_kind, AutoReviewDetailKind::Run);
        assert_eq!(detail.status, AutoReviewRunStatus::Completed);
        assert_eq!(detail.branch.as_deref(), Some("feature"));
        assert!(detail.content.contains("overall_correctness=incorrect"));
        assert!(detail.content.contains("finding_id=f1"));
        assert!(!detail.truncated);
        assert!(detail.bytes <= detail.max_bytes);
    }

    #[test]
    #[serial]
    fn detail_lookup_returns_specific_finding_by_stable_id() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let run_id = Uuid::new_v4();
        let mut output = review_output();
        output.findings = vec![review_finding(1, "first body"), review_finding(2, "second body")];
        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();
        store
            .upsert(AutoReviewRun::new(run_id, AutoReviewRunSource::Tui, 1))
            .unwrap();
        store.write_output(run_id, &output).unwrap();

        let detail = store.read_detail(run_id, Some("f2"), Some(2_000)).unwrap();

        assert_eq!(detail.finding_id.as_deref(), Some("f2"));
        assert_eq!(detail.detail_kind, AutoReviewDetailKind::Finding);
        assert_eq!(detail.finding_count, 2);
        assert_eq!(detail.omitted_findings, 1);
        assert!(detail.content.contains("title: finding title 2"));
        assert!(detail.content.contains("second body"));
        assert!(!detail.content.contains("first body"));
    }

    #[test]
    #[serial]
    fn detail_lookup_truncates_long_content_with_metadata() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let run_id = Uuid::new_v4();
        let mut output = review_output();
        output.findings = vec![review_finding(1, &"x".repeat(4_000))];
        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();
        store
            .upsert(AutoReviewRun::new(run_id, AutoReviewRunSource::Tui, 1))
            .unwrap();
        store.write_output(run_id, &output).unwrap();

        let detail = store.read_detail(run_id, Some("f1"), Some(300)).unwrap();

        assert!(detail.truncated);
        assert!(detail.content.contains("truncated auto review detail"));
        assert!(detail.original_bytes > detail.bytes);
        assert!(detail.bytes <= 300);
    }

    #[test]
    #[serial]
    fn detail_lookup_reports_missing_sidecar() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let run_id = Uuid::new_v4();
        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();
        store
            .upsert(AutoReviewRun::new(run_id, AutoReviewRunSource::Tui, 1))
            .unwrap();

        let err = store.read_detail(run_id, None, None).unwrap_err();

        assert_eq!(err.code, AutoReviewDetailErrorCode::MissingSidecar);
        assert!(err.message.contains("sidecar"));
    }

    #[test]
    #[serial]
    fn detail_lookup_reports_missing_and_invalid_finding_ids() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let run_id = Uuid::new_v4();
        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();
        store
            .upsert(AutoReviewRun::new(run_id, AutoReviewRunSource::Tui, 1))
            .unwrap();
        store.write_output(run_id, &review_output()).unwrap();

        let invalid = store.read_detail(run_id, Some("finding-1"), None).unwrap_err();
        assert_eq!(invalid.code, AutoReviewDetailErrorCode::InvalidFindingId);

        let missing = store.read_detail(run_id, Some("f2"), None).unwrap_err();
        assert_eq!(missing.code, AutoReviewDetailErrorCode::MissingFinding);
    }

    #[test]
    #[serial]
    fn compact_ledger_omits_verbose_sidecar_detail() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let run_id = Uuid::new_v4();
        let mut run = AutoReviewRun::new(run_id, AutoReviewRunSource::Tui, 10);
        run.mark_status(AutoReviewRunStatus::Completed, 20);
        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();
        store.upsert(run).unwrap();
        let mut output = review_output();
        output.findings = vec![review_finding(1, "VERY_VERBOSE_FINDING_BODY_SHOULD_STAY_LAZY")];
        store.write_output(run_id, &output).unwrap();

        let ledger = store.compact_ledger(ledger_options(30)).expect("ledger");

        assert!(ledger.contains(&run_id.to_string()));
        assert!(!ledger.contains("VERY_VERBOSE_FINDING_BODY_SHOULD_STAY_LAZY"));
        assert!(!ledger.contains("overall_correctness"));
    }

    #[test]
    #[serial]
    fn run_store_rejects_output_for_unknown_run() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();

        let err = store
            .write_output(Uuid::new_v4(), &review_output())
            .expect_err("unknown run should error");
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    #[serial]
    fn run_store_merges_latest_disk_state_before_save() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let first_id = Uuid::new_v4();
        let second_id = Uuid::new_v4();
        let mut first = AutoReviewRunStore::open(repo.path()).unwrap();
        let mut second = AutoReviewRunStore::open(repo.path()).unwrap();

        first
            .upsert(AutoReviewRun::new(first_id, AutoReviewRunSource::Tui, 1))
            .unwrap();
        second
            .upsert(AutoReviewRun::new(second_id, AutoReviewRunSource::Exec, 2))
            .unwrap();

        let loaded = AutoReviewRunStore::open(repo.path()).unwrap();
        assert!(loaded.get(first_id).is_some());
        assert!(loaded.get(second_id).is_some());
    }

    #[test]
    #[serial]
    fn duplicate_lookup_prefers_in_flight_fingerprint_match() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();

        let completed_id = Uuid::new_v4();
        let mut completed = AutoReviewRun::new(completed_id, AutoReviewRunSource::Tui, 1);
        completed.diff_fingerprint = Some("diff:abc".to_string());
        completed.mark_status(AutoReviewRunStatus::Completed, 2);
        store.upsert(completed).unwrap();

        let live_id = Uuid::new_v4();
        let mut live = AutoReviewRun::new(live_id, AutoReviewRunSource::Tui, 3);
        live.diff_fingerprint = Some("diff:abc".to_string());
        live.agent_id = Some("agent-live".to_string());
        live.snapshot_commit = Some("snap-live".to_string());
        live.mark_status(AutoReviewRunStatus::Reviewing, 4);
        store.upsert(live).unwrap();

        let duplicate = store
            .find_duplicate_by_fingerprint("diff:abc")
            .expect("duplicate");
        assert_eq!(duplicate.run_id, live_id);
        assert_eq!(duplicate.disposition, AutoReviewDuplicateDisposition::Adopt);
        assert_eq!(duplicate.agent_id.as_deref(), Some("agent-live"));
    }

    #[test]
    #[serial]
    fn duplicate_lookup_does_not_adopt_pending_fingerprint_match() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();

        let pending_id = Uuid::new_v4();
        let mut pending = AutoReviewRun::new(pending_id, AutoReviewRunSource::Tui, 1);
        pending.diff_fingerprint = Some("diff:abc".to_string());
        store.upsert(pending).unwrap();

        let duplicate = store
            .find_duplicate_by_fingerprint("diff:abc")
            .expect("duplicate");
        assert_eq!(duplicate.run_id, pending_id);
        assert_eq!(
            duplicate.disposition,
            AutoReviewDuplicateDisposition::SupersedeTerminal
        );
    }

    #[test]
    #[serial]
    fn duplicate_lookup_reuses_terminal_finding_match() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();

        let run_id = Uuid::new_v4();
        let mut run = AutoReviewRun::new(run_id, AutoReviewRunSource::Tui, 1);
        run.diff_fingerprint = Some("diff:abc".to_string());
        run.finding_count = 1;
        run.mark_status(AutoReviewRunStatus::Completed, 2);
        store.upsert(run).unwrap();

        let duplicate = store
            .find_duplicate_by_fingerprint("diff:abc")
            .expect("duplicate");
        assert_eq!(duplicate.run_id, run_id);
        assert_eq!(
            duplicate.disposition,
            AutoReviewDuplicateDisposition::ReuseTerminal
        );
    }

    #[test]
    #[serial]
    fn supersede_by_fingerprint_omits_runs_with_findings_and_errors() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();

        let new_id = Uuid::new_v4();
        let clean_id = Uuid::new_v4();
        let finding_id = Uuid::new_v4();
        let failed_id = Uuid::new_v4();

        let mut clean = AutoReviewRun::new(clean_id, AutoReviewRunSource::Tui, 1);
        clean.diff_fingerprint = Some("diff:abc".to_string());
        clean.mark_status(AutoReviewRunStatus::Completed, 2);
        store.upsert(clean).unwrap();

        let mut finding = AutoReviewRun::new(finding_id, AutoReviewRunSource::Tui, 3);
        finding.diff_fingerprint = Some("diff:abc".to_string());
        finding.finding_count = 1;
        finding.mark_status(AutoReviewRunStatus::Completed, 4);
        store.upsert(finding).unwrap();

        let mut failed = AutoReviewRun::new(failed_id, AutoReviewRunSource::Tui, 5);
        failed.diff_fingerprint = Some("diff:abc".to_string());
        failed.error_class = Some("exec".to_string());
        failed.error_summary = Some("agent failed".to_string());
        failed.mark_status(AutoReviewRunStatus::Failed, 6);
        store.upsert(failed).unwrap();

        let changed = store
            .mark_superseded_by_fingerprint("diff:abc", new_id, 10)
            .unwrap();
        assert_eq!(changed, 1);

        let loaded = AutoReviewRunStore::open(repo.path()).unwrap();
        let clean = loaded.get(clean_id).unwrap();
        assert_eq!(clean.status, AutoReviewRunStatus::Superseded);
        assert_eq!(clean.superseded_by, Some(new_id));

        let finding = loaded.get(finding_id).unwrap();
        assert_eq!(finding.status, AutoReviewRunStatus::Completed);
        assert_eq!(finding.superseded_by, None);

        let failed = loaded.get(failed_id).unwrap();
        assert_eq!(failed.status, AutoReviewRunStatus::Failed);
        assert_eq!(failed.superseded_by, None);
        assert_eq!(failed.error_summary.as_deref(), Some("agent failed"));
    }

    #[test]
    #[serial]
    fn run_store_reconciles_orphaned_in_flight_agents() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let live_id = Uuid::new_v4();
        let orphan_id = Uuid::new_v4();
        let terminal_id = Uuid::new_v4();
        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();

        let mut live = AutoReviewRun::new(live_id, AutoReviewRunSource::Tui, 1);
        live.agent_id = Some("agent-live".to_string());
        live.mark_status(AutoReviewRunStatus::Reviewing, 2);
        store.upsert(live).unwrap();

        let mut orphan = AutoReviewRun::new(orphan_id, AutoReviewRunSource::Tui, 3);
        orphan.agent_id = Some("agent-gone".to_string());
        orphan.mark_status(AutoReviewRunStatus::Reviewing, 4);
        store.upsert(orphan).unwrap();

        let mut terminal = AutoReviewRun::new(terminal_id, AutoReviewRunSource::Tui, 5);
        terminal.agent_id = Some("agent-terminal".to_string());
        terminal.mark_status(AutoReviewRunStatus::Completed, 6);
        store.upsert(terminal).unwrap();

        let agentless_id = Uuid::new_v4();
        let mut agentless = AutoReviewRun::new(agentless_id, AutoReviewRunSource::Tui, 7);
        agentless.mark_status(AutoReviewRunStatus::Snapshotting, 8);
        store.upsert(agentless).unwrap();

        let changed = store
            .reconcile_orphaned_in_flight(["agent-live"], 20)
            .unwrap();
        assert_eq!(changed, 2);
        assert_eq!(
            store.get(live_id).expect("live run").status,
            AutoReviewRunStatus::Reviewing
        );
        let orphan = store.get(orphan_id).expect("orphan run");
        assert_eq!(orphan.status, AutoReviewRunStatus::Lost);
        assert_eq!(orphan.freshness, AutoReviewFreshness::Lost);
        assert_eq!(orphan.cancel_reason.as_deref(), Some("agent_missing_after_restart"));
        assert_eq!(orphan.completed_at, Some(20));
        assert_eq!(
            store.get(terminal_id).expect("terminal run").status,
            AutoReviewRunStatus::Completed
        );
        assert_eq!(
            store.get(agentless_id).expect("agentless run").status,
            AutoReviewRunStatus::Lost
        );
    }

    #[test]
    #[serial]
    fn run_store_compacts_to_most_recent_runs() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();
        let oldest = Uuid::new_v4();
        let middle = Uuid::new_v4();
        let newest = Uuid::new_v4();

        store
            .upsert(AutoReviewRun::new(oldest, AutoReviewRunSource::Tui, 1))
            .unwrap();
        store
            .upsert(AutoReviewRun::new(middle, AutoReviewRunSource::Tui, 2))
            .unwrap();
        store
            .upsert(AutoReviewRun::new(newest, AutoReviewRunSource::Tui, 3))
            .unwrap();

        store.compact(2).unwrap();
        let loaded = AutoReviewRunStore::open(repo.path()).unwrap();
        assert!(loaded.get(oldest).is_none());
        assert!(loaded.get(middle).is_some());
        assert!(loaded.get(newest).is_some());
    }

    #[test]
    #[serial]
    fn compact_ledger_omits_idle_clean_runs() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let run_id = Uuid::new_v4();
        let mut run = AutoReviewRun::new(run_id, AutoReviewRunSource::Tui, 10);
        run.mark_status(AutoReviewRunStatus::Completed, 20);
        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();
        store.upsert(run).unwrap();

        assert!(store.compact_ledger(ledger_options(30)).is_none());
    }

    #[test]
    #[serial]
    fn compact_ledger_includes_active_run_activity_and_snapshot() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let run_id = Uuid::new_v4();
        let mut run = AutoReviewRun::new(run_id, AutoReviewRunSource::Tui, 100);
        run.agent_id = Some("agent-1234567890".to_string());
        run.branch = Some("auto-review".to_string());
        run.snapshot_commit = Some("abcdef1234567890".to_string());
        run.model = Some("gpt-5.4-mini".to_string());
        run.reasoning_effort = Some("medium".to_string());
        run.prompt_token_estimate = Some(42_000);
        run.token_count = Some(84_000);
        run.freshness = AutoReviewFreshness::Current;
        run.mark_activity(120);
        run.mark_status(AutoReviewRunStatus::Reviewing, 130);
        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();
        store.upsert(run).unwrap();

        let ledger = store.compact_ledger(ledger_options(160)).expect("ledger");
        assert!(ledger.contains("<auto_review_ledger"));
        assert!(ledger.contains(&run_id.to_string()));
        assert!(ledger.contains("status=Reviewing"));
        assert!(ledger.contains("last_activity=lt1m"));
        assert!(ledger.contains("snapshot=abcdef123456"));
        assert!(ledger.contains("target=unknown"));
        assert!(ledger.contains("model=gpt-5.4-mini"));
        assert!(ledger.contains("reasoning=medium"));
        assert!(ledger.contains("prompt_estimate=42000t"));
        assert!(ledger.contains("tokens=84000t"));
        assert!(ledger.contains("elapsed=lt1m"));
    }

    #[test]
    #[serial]
    fn compact_ledger_marks_matching_head_target_separately_from_freshness() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let mut run = AutoReviewRun::new(Uuid::new_v4(), AutoReviewRunSource::Tui, 10);
        run.branch = Some("feature".to_string());
        run.snapshot_commit = Some("abcdef1234567890".to_string());
        run.finding_count = 1;
        run.finding_digests = vec![finding_digest(1)];
        run.mark_status(AutoReviewRunStatus::Completed, 20);
        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();
        store.upsert(run).unwrap();

        let ledger = store
            .compact_ledger(
                ledger_options(30).with_active_target(
                    Some("feature".to_string()),
                    Some("abcdef1234567890".to_string()),
                ),
            )
            .expect("ledger");

        assert!(ledger.contains("freshness=Unknown"));
        assert!(ledger.contains("target=matching_head"));
    }

    #[test]
    #[serial]
    fn compact_ledger_marks_launch_head_mismatch_as_older_snapshot() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let mut run = AutoReviewRun::new(Uuid::new_v4(), AutoReviewRunSource::Tui, 10);
        run.branch = Some("feature".to_string());
        run.head_at_launch = Some("aaaaaaaaaaaaaaaa".to_string());
        run.finding_count = 1;
        run.finding_digests = vec![finding_digest(1)];
        run.mark_status(AutoReviewRunStatus::Completed, 20);
        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();
        store.upsert(run).unwrap();

        let ledger = store
            .compact_ledger(
                ledger_options(30).with_active_target(
                    Some("feature".to_string()),
                    Some("bbbbbbbbbbbbbbbb".to_string()),
                ),
            )
            .expect("ledger");

        assert!(ledger.contains("suppressed_stale=1"));
        assert!(!ledger.contains("target=older_snapshot"));
        assert!(!ledger.contains("target=matching_head"));
    }

    #[test]
    #[serial]
    fn compact_ledger_marks_detached_and_older_review_targets() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let mut detached = AutoReviewRun::new(Uuid::new_v4(), AutoReviewRunSource::Tui, 10);
        detached.branch = Some("auto-review".to_string());
        detached.snapshot_commit = Some("1111111111111111".to_string());
        detached.finding_count = 1;
        detached.finding_digests = vec![finding_digest(1)];
        detached.mark_status(AutoReviewRunStatus::Completed, 20);

        let mut older = AutoReviewRun::new(Uuid::new_v4(), AutoReviewRunSource::Tui, 30);
        older.branch = Some("feature".to_string());
        older.snapshot_commit = Some("2222222222222222".to_string());
        older.finding_count = 1;
        older.finding_digests = vec![finding_digest(2)];
        older.mark_status(AutoReviewRunStatus::Completed, 40);

        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();
        store.upsert(detached).unwrap();
        store.upsert(older).unwrap();

        let ledger = store
            .compact_ledger(
                ledger_options(50).with_active_target(
                    Some("feature".to_string()),
                    Some("3333333333333333".to_string()),
                ),
            )
            .expect("ledger");

        assert!(ledger.contains("suppressed_stale=2"));
        assert!(!ledger.contains("target=detached_review_worktree"));
        assert!(!ledger.contains("target=older_snapshot"));
        assert!(ledger.contains("Freshness describes run recency"));
    }

    #[test]
    #[serial]
    fn compact_ledger_suppresses_known_stale_terminal_findings() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let detached_id = Uuid::new_v4();
        let mut detached = AutoReviewRun::new(detached_id, AutoReviewRunSource::Tui, 10);
        detached.branch = Some("auto-review".to_string());
        detached.snapshot_commit = Some("1111111111111111".to_string());
        detached.token_count = Some(42_000);
        detached.finding_count = 1;
        detached.finding_digests = vec![finding_digest(1)];
        detached.mark_status(AutoReviewRunStatus::Completed, 20);

        let older_id = Uuid::new_v4();
        let mut older = AutoReviewRun::new(older_id, AutoReviewRunSource::Tui, 30);
        older.branch = Some("feature".to_string());
        older.snapshot_commit = Some("2222222222222222".to_string());
        older.finding_count = 1;
        older.finding_digests = vec![finding_digest(2)];
        older.mark_status(AutoReviewRunStatus::Completed, 40);

        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();
        store.upsert(detached).unwrap();
        store.upsert(older).unwrap();

        let ledger = store
            .compact_ledger(
                ledger_options(50).with_active_target(
                    Some("feature".to_string()),
                    Some("3333333333333333".to_string()),
                ),
            )
            .expect("ledger");

        assert!(ledger.contains("suppressed_stale=2"));
        assert!(ledger.contains("tokens=42000t"));
        assert!(!ledger.contains(&format!("run id={detached_id}")));
        assert!(!ledger.contains(&format!("run id={older_id}")));
        assert!(!ledger.contains("finding id=f1"));
        assert!(!ledger.contains("finding id=f2"));
    }

    #[test]
    #[serial]
    fn compact_ledger_keeps_matching_head_findings_actionable() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let run_id = Uuid::new_v4();
        let mut run = AutoReviewRun::new(run_id, AutoReviewRunSource::Tui, 10);
        run.branch = Some("feature".to_string());
        run.snapshot_commit = Some("abcdef1234567890".to_string());
        run.finding_count = 1;
        run.finding_digests = vec![finding_digest(1)];
        run.mark_status(AutoReviewRunStatus::Completed, 20);
        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();
        store.upsert(run).unwrap();

        let ledger = store
            .compact_ledger(
                ledger_options(30).with_active_target(
                    Some("feature".to_string()),
                    Some("abcdef1234567890".to_string()),
                ),
            )
            .expect("ledger");

        assert!(ledger.contains(&format!("run id={run_id}")));
        assert!(ledger.contains("target=matching_head"));
        assert!(ledger.contains("finding id=f1"));
        assert!(!ledger.contains("suppressed_stale"));
    }

    #[test]
    #[serial]
    fn compact_ledger_reports_saved_token_estimates_for_skipped_duplicates() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let run_id = Uuid::new_v4();
        let duplicate_id = Uuid::new_v4();
        let mut run = AutoReviewRun::new(run_id, AutoReviewRunSource::Tui, 10);
        run.prompt_token_estimate = Some(8_000);
        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();
        store.upsert(run).unwrap();
        store
            .mark_skipped_duplicate(run_id, duplicate_id, Some("diff:abc"), Some(84_000), 20)
            .unwrap();

        let ledger = store.compact_ledger(ledger_options(30)).expect("ledger");

        assert!(ledger.contains("saved_estimate=84000t"));
        assert!(ledger.contains("saved_runs=1"));
        assert!(!ledger.contains(&format!("run id={run_id}")));
    }

    #[test]
    #[serial]
    fn superseded_clean_duplicate_does_not_report_saved_tokens() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let clean_id = Uuid::new_v4();
        let fresh_id = Uuid::new_v4();
        let mut clean = AutoReviewRun::new(clean_id, AutoReviewRunSource::Tui, 10);
        clean.diff_fingerprint = Some("diff:abc".to_string());
        clean.prompt_token_estimate = Some(42_000);
        clean.mark_status(AutoReviewRunStatus::Completed, 20);
        let mut fresh = AutoReviewRun::new(fresh_id, AutoReviewRunSource::Tui, 30);
        fresh.diff_fingerprint = Some("diff:abc".to_string());
        fresh.mark_status(AutoReviewRunStatus::Reviewing, 40);
        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();
        store.upsert(clean).unwrap();
        store.upsert(fresh).unwrap();

        store.mark_superseded_by_fingerprint("diff:abc", fresh_id, 50).unwrap();
        let ledger = store.compact_ledger(ledger_options(60)).expect("ledger");

        assert!(ledger.contains("prompt_estimate=42000t"));
        assert!(!ledger.contains("saved_estimate="));
        assert!(!ledger.contains("saved_runs="));
        assert_eq!(store.get(clean_id).unwrap().saved_token_estimate, None);
    }

    #[test]
    #[serial]
    fn compact_ledger_includes_recent_burn_diagnostics() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let mut high_token_run = AutoReviewRun::new(Uuid::new_v4(), AutoReviewRunSource::Tui, 10);
        high_token_run.mark_activity(20);
        high_token_run.prompt_token_estimate = Some(12_000);
        high_token_run.token_count = Some(25_915);
        high_token_run.mark_status(AutoReviewRunStatus::Completed, 40);

        let mut high_prompt_run = AutoReviewRun::new(Uuid::new_v4(), AutoReviewRunSource::Tui, 50);
        high_prompt_run.mark_activity(60);
        high_prompt_run.prompt_token_estimate = Some(42_000);
        high_prompt_run.mark_status(AutoReviewRunStatus::Reviewing, 70);
        high_prompt_run.freshness = AutoReviewFreshness::Current;

        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();
        store.upsert(high_token_run).unwrap();
        store.upsert(high_prompt_run).unwrap();

        let ledger = store.compact_ledger(ledger_options(100)).expect("ledger");
        assert!(ledger.contains("diagnostics recent_runs=2"));
        assert!(ledger.contains("in_flight=1"));
        assert!(ledger.contains("terminal=1"));
        assert!(ledger.contains("tokens=25915t"));
        assert!(ledger.contains("token_runs=1"));
        assert!(ledger.contains("prompt_estimate=54000t"));
        assert!(ledger.contains("prompt_runs=2"));
        assert!(ledger.contains("high_burn=2"));
        assert!(ledger.contains("longest_elapsed=lt1m"));
    }

    #[test]
    #[serial]
    fn compact_ledger_can_include_diagnostics_without_listing_clean_run() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let run_id = Uuid::new_v4();
        let mut run = AutoReviewRun::new(run_id, AutoReviewRunSource::Tui, 10);
        run.prompt_token_estimate = Some(35_000);
        run.mark_status(AutoReviewRunStatus::Completed, 20);
        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();
        store.upsert(run).unwrap();

        let ledger = store.compact_ledger(ledger_options(30)).expect("ledger");
        assert!(ledger.contains("diagnostics recent_runs=1"));
        assert!(ledger.contains("prompt_estimate=35000t"));
        assert!(ledger.contains("high_burn=1"));
        assert!(!ledger.contains(&format!("run id={run_id}")));
    }

    #[test]
    #[serial]
    fn compact_ledger_omits_diagnostics_for_old_or_signal_free_runs() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let mut old = AutoReviewRun::new(Uuid::new_v4(), AutoReviewRunSource::Tui, 1);
        old.prompt_token_estimate = Some(50_000);
        old.token_count = Some(75_000);
        old.mark_status(AutoReviewRunStatus::Completed, 2);

        let mut signal_free = AutoReviewRun::new(Uuid::new_v4(), AutoReviewRunSource::Tui, 100);
        signal_free.mark_status(AutoReviewRunStatus::Completed, 101);

        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();
        store.upsert(old).unwrap();
        store.upsert(signal_free).unwrap();

        assert!(
            store
                .compact_ledger(ledger_options(LEDGER_DIAGNOSTIC_RECENT_SECS + 20))
                .is_none()
        );
    }

    #[test]
    #[serial]
    fn compact_ledger_omits_inactive_in_flight_run_without_current_freshness() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let mut run = AutoReviewRun::new(Uuid::new_v4(), AutoReviewRunSource::Tui, 1);
        run.mark_activity(2);
        run.mark_status(AutoReviewRunStatus::Reviewing, 3);
        run.freshness = AutoReviewFreshness::Unknown;
        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();
        store.upsert(run).unwrap();

        assert!(
            store
                .compact_ledger(ledger_options(LEDGER_IN_FLIGHT_ACTIVITY_SECS + 20))
                .is_none()
        );
    }

    #[test]
    #[serial]
    fn compact_ledger_includes_current_long_running_run() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let mut run = AutoReviewRun::new(Uuid::new_v4(), AutoReviewRunSource::Tui, 1);
        run.mark_activity(2);
        run.mark_status(AutoReviewRunStatus::Reviewing, 3);
        run.freshness = AutoReviewFreshness::LongRunning;
        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();
        store.upsert(run).unwrap();

        let ledger = store
            .compact_ledger(ledger_options(LEDGER_IN_FLIGHT_ACTIVITY_SECS + 20))
            .expect("ledger");
        assert!(ledger.contains("freshness=LongRunning"));
    }

    #[test]
    #[serial]
    fn compact_ledger_includes_recent_findings_with_digest_cap() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let mut run = AutoReviewRun::new(Uuid::new_v4(), AutoReviewRunSource::Exec, 100);
        run.mark_status(AutoReviewRunStatus::Completed, 140);
        run.finding_count = 6;
        run.finding_digests = (1..=5).map(finding_digest).collect();
        run.omitted_finding_digest_count = 1;
        run.summary_digest = Some("review found important issues".to_string());
        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();
        store.upsert(run).unwrap();

        let ledger = store.compact_ledger(ledger_options(160)).expect("ledger");
        assert!(ledger.contains("findings=6"));
        assert!(ledger.contains("omitted_findings=1"));
        assert!(ledger.contains("finding id=f1"));
        assert!(ledger.contains("finding id=f3"));
        assert!(!ledger.contains("finding id=f4"));
        assert!(ledger.contains("more_findings=2"));
    }

    #[test]
    #[serial]
    fn compact_ledger_omits_old_terminal_findings() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let mut run = AutoReviewRun::new(Uuid::new_v4(), AutoReviewRunSource::Exec, 1);
        run.mark_status(AutoReviewRunStatus::Completed, 2);
        run.finding_count = 1;
        run.finding_digests = vec![finding_digest(1)];
        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();
        store.upsert(run).unwrap();

        assert!(
            store
                .compact_ledger(ledger_options(LEDGER_RECENT_ACTIONABLE_SECS + 10))
                .is_none()
        );
    }

    #[test]
    #[serial]
    fn compact_ledger_omits_clean_cancelled_run() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let mut run = AutoReviewRun::new(Uuid::new_v4(), AutoReviewRunSource::Exec, 1);
        run.mark_status(AutoReviewRunStatus::Cancelled, 2);
        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();
        store.upsert(run).unwrap();

        assert!(store.compact_ledger(ledger_options(10)).is_none());
    }

    #[test]
    #[serial]
    fn open_existing_does_not_create_auto_review_dir() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());

        let store = AutoReviewRunStore::open_existing(repo.path()).unwrap();
        assert!(store.is_none());
        assert!(!auto_review_dir_path(repo.path()).unwrap().exists());
        assert!(!code_home.path().join("state").join("review").exists());
    }

    #[test]
    #[serial]
    fn open_existing_read_only_does_not_create_outputs_dir() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let root = auto_review_dir_path(repo.path()).unwrap();
        fs::create_dir_all(&root).unwrap();
        write_runs_file(&root.join(RUNS_FILENAME), std::iter::empty()).unwrap();

        let store = AutoReviewRunStore::open_existing_read_only(repo.path())
            .unwrap()
            .expect("store");

        assert_eq!(store.root(), root.as_path());
        assert!(!root.join(OUTPUTS_DIR).exists());
    }

    #[test]
    #[serial]
    fn compact_ledger_honors_byte_cap() {
        let code_home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        set_code_home(code_home.path());
        let mut store = AutoReviewRunStore::open(repo.path()).unwrap();
        for idx in 0..10 {
            let mut run = AutoReviewRun::new(Uuid::new_v4(), AutoReviewRunSource::Tui, idx);
            run.mark_status(AutoReviewRunStatus::Reviewing, idx + 1);
            run.summary_digest = Some("a very long summary ".repeat(40));
            store.upsert(run).unwrap();
        }

        let ledger = store
            .compact_ledger(AutoReviewLedgerOptions {
                now: 20,
                max_bytes: 180,
                max_runs: 10,
                active_branch: None,
                active_head: None,
            })
            .expect("ledger");
        assert!(ledger.len() <= 180, "ledger len was {}", ledger.len());
        assert!(ledger.contains("truncated"));
    }
}

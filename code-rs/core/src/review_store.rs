use crate::protocol::ReviewOutputEvent;
use crate::review_coord::scoped_review_state_dir;
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
}

pub fn auto_review_dir(scope: &Path) -> io::Result<PathBuf> {
    Ok(scoped_review_state_dir(scope)?.join(AUTO_REVIEW_DIR))
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
}

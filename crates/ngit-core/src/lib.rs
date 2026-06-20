use rand::distr::{Alphanumeric, SampleString};
use schemars::{schema_for, JsonSchema};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use tempfile::NamedTempFile;
use thiserror::Error;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

pub type CoreResult<T> = Result<T, CoreError>;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("not a Git repo: {0}")]
    NotGitRepo(PathBuf),
    #[error(".ngit is not initialized at {0}")]
    NotInitialized(PathBuf),
    #[error("unsupported schema version {version} in {path}")]
    UnsupportedSchema { path: PathBuf, version: u32 },
    #[error("malformed JSON at {path}: {source}")]
    MalformedJson {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("ambiguous id prefix: {0}")]
    AmbiguousId(String),
    #[error("record not found: {0}")]
    MissingRecord(String),
    #[error("draft not found: {0}")]
    MissingDraft(String),
    #[error("evidence owner missing: {0}")]
    EvidenceOwnerMissing(String),
    #[error("annotation owner missing: {0}")]
    AnnotationOwnerMissing(String),
    #[error("lock held by active process: {0}")]
    LockHeld(PathBuf),
    #[error("Git command failed: {0}")]
    GitCommandFailed(String),
    #[error("dirty state changed since draft capture: {0}")]
    DirtyStateChanged(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct Manifest {
    pub schema_version: u32,
    pub store_format: String,
    pub created_at: String,
    pub updated_at: String,
    pub repo: ManifestRepo,
    pub core: ManifestCore,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ManifestRepo {
    pub vcs: String,
    pub root: PathBuf,
    pub init_head: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ManifestCore {
    pub implementation: String,
    pub implementation_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct CapturePolicy {
    pub schema_version: u32,
    pub mode: String,
    pub triggers: Vec<String>,
    pub allow_empty_capture: bool,
    pub dedupe: DedupePolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DedupePolicy {
    pub enabled: bool,
    pub fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ReadinessPolicy {
    pub schema_version: u32,
    pub required_status_buckets: Vec<String>,
    pub missing_evidence_status: String,
    pub include_counts: bool,
    pub include_producer_kinds: bool,
    pub include_artifact_availability: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct EvidencePolicy {
    pub schema_version: u32,
    pub durable_artifacts_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct RepoSnapshot {
    pub schema_version: u32,
    pub captured_at: String,
    pub repo_root: PathBuf,
    pub branch: String,
    pub head: Option<String>,
    pub head_parent_count: u32,
    pub changed_files: Vec<ChangedFile>,
    pub recent_commits: Vec<CommitSummary>,
    pub staged_digest: String,
    pub worktree_digest: String,
    pub dirty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
pub struct ChangedFile {
    pub path: String,
    pub index_status: String,
    pub worktree_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct CommitSummary {
    pub sha: String,
    pub subject: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DraftChange {
    pub schema_version: u32,
    pub draft_id: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub capture: CaptureInfo,
    pub repo_snapshot: RepoSnapshot,
    pub changed_paths: Vec<String>,
    pub summary: String,
    pub annotation_refs: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub readiness: DecisionContext,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct CaptureInfo {
    pub source: String,
    pub trigger: String,
    pub capture_key: String,
    pub deduped_from: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct AnnotationRecord {
    pub schema_version: u32,
    pub annotation_id: String,
    pub owner: RecordOwner,
    #[serde(rename = "type")]
    pub annotation_type: String,
    pub status: String,
    pub summary: Option<String>,
    pub body: String,
    pub producer: Producer,
    pub created_at: String,
    pub updated_at: String,
    pub refs: Vec<AnnotationRef>,
    pub payload_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct AnnotationRef {
    pub kind: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct RecordOwner {
    #[serde(rename = "type")]
    pub owner_type: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct Producer {
    pub kind: String,
    pub name: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct EvidenceRecord {
    pub schema_version: u32,
    pub evidence_id: String,
    pub owner: RecordOwner,
    #[serde(rename = "type")]
    pub evidence_type: String,
    pub status: String,
    pub summary: String,
    pub created_at: String,
    pub producer: Producer,
    pub command: Option<CommandEvidence>,
    pub findings: Vec<String>,
    pub artifacts: Vec<ArtifactRef>,
    pub payload_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct CommandEvidence {
    pub argv: Vec<String>,
    pub exit_code: i32,
    pub duration_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ArtifactRef {
    pub kind: String,
    pub path: PathBuf,
    pub digest: String,
    #[serde(default)]
    pub size_bytes: u64,
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub original_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DecisionContext {
    pub schema_version: u32,
    pub computed_at: String,
    pub policy_hash: String,
    pub evidence_summary: BTreeMap<String, Vec<String>>,
    pub unresolved_evidence: Vec<String>,
    pub deterministic_action: String,
    pub review_action: Option<String>,
    pub final_action: String,
    pub override_required: bool,
    pub override_targets: Vec<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct AcceptedChange {
    pub schema_version: u32,
    pub change_id: String,
    pub draft_id: String,
    pub status: String,
    pub accepted_at: String,
    pub accepted_by: Actor,
    pub repo_snapshot_at_capture: RepoSnapshot,
    pub repo_snapshot_at_acceptance: RepoSnapshot,
    pub changed_paths: Vec<String>,
    pub annotation_refs: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub decision_context: DecisionContext,
    pub override_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct RejectedChange {
    pub schema_version: u32,
    pub draft_id: String,
    pub status: String,
    pub rejected_at: String,
    pub rejected_by: Actor,
    pub reason: String,
    pub repo_snapshot_at_capture: RepoSnapshot,
    pub annotation_refs: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub decision_context: DecisionContext,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct Actor {
    pub kind: String,
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct LineageRecord {
    pub schema_version: u32,
    pub lineage_id: String,
    pub event_type: String,
    pub change_id: Option<String>,
    pub draft_id: String,
    pub created_at: String,
    pub repo: BTreeMap<String, RepoSnapshot>,
    pub changed_paths: Vec<String>,
    pub annotation_refs: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub decision_context: DecisionContext,
    pub decision: Option<TerminalDecision>,
    pub override_reason: Option<String>,
    pub links: LineageLinks,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct TerminalDecision {
    pub action: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct LineageLinks {
    pub event_ids: Vec<String>,
    pub supersedes: Vec<String>,
    pub related_changes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct EventRecord {
    pub schema_version: u32,
    pub event_id: String,
    pub event_type: String,
    pub created_at: String,
    pub signals: Vec<String>,
    pub repo: EventRepo,
    pub changed_files: Vec<ChangedFile>,
    pub related: BTreeMap<String, String>,
    pub detail: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct EventRepo {
    pub branch: String,
    pub head: Option<String>,
    pub staged_digest: String,
    pub worktree_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct RepoStatus {
    pub repo_root: PathBuf,
    pub initialized: bool,
    pub snapshot: RepoSnapshot,
    pub open_drafts: usize,
    pub latest_evidence_state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DoctorReport {
    pub schema_version: u32,
    pub checked_at: String,
    pub issues: Vec<DoctorIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DoctorIssue {
    pub code: String,
    pub message: String,
    pub path: Option<PathBuf>,
    pub record_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", content = "record")]
pub enum Record {
    Draft(DraftChange),
    Annotation(AnnotationRecord),
    Evidence(EvidenceRecord),
    Accepted(AcceptedChange),
    Rejected(RejectedChange),
    Lineage(LineageRecord),
    Event(EventRecord),
}

#[derive(Debug, Clone)]
pub struct CaptureOptions {
    pub trigger: String,
    pub intent: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EvidenceRunOptions {
    pub timeout: Duration,
    pub max_output_bytes: usize,
}

impl Default for EvidenceRunOptions {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(120),
            max_output_bytes: 5 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AnnotationInput {
    pub owner_id: String,
    pub annotation_type: String,
    pub status: String,
    pub summary: Option<String>,
    pub body: String,
    pub producer_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct MigrationReport {
    pub schema_version: u32,
    pub created_at: String,
    pub imported: BTreeMap<String, usize>,
    pub skipped: Vec<MigrationIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct MigrationIssue {
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct SchemaExportReport {
    pub schema_version: u32,
    pub exported_at: String,
    pub files: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
struct TransactionManifest {
    schema_version: u32,
    transaction_id: String,
    operation: String,
    draft_id: String,
    state: String,
    created_at: String,
    updated_at: String,
    terminal_path: PathBuf,
    lineage_path: PathBuf,
    draft_path: PathBuf,
}

pub fn init(repo_path: &Path) -> CoreResult<Manifest> {
    let root = git_root(repo_path)?;
    let ngit = ngit_dir(&root);
    for dir in durable_dirs(&ngit).into_iter().chain(ephemeral_dirs(&ngit)) {
        fs::create_dir_all(&dir).map_err(|source| CoreError::Io {
            path: dir.clone(),
            source,
        })?;
    }
    let now = now_string();
    let manifest_path = ngit.join("manifest.json");
    let manifest = if manifest_path.exists() {
        read_json::<Manifest>(&manifest_path)?
    } else {
        let manifest = Manifest {
            schema_version: 1,
            store_format: "ngit-core-files-v1".to_string(),
            created_at: now.clone(),
            updated_at: now.clone(),
            repo: ManifestRepo {
                vcs: "git".to_string(),
                root: root.clone(),
                init_head: git_optional(&root, ["rev-parse", "HEAD"]),
            },
            core: ManifestCore {
                implementation: "ngitd-core".to_string(),
                implementation_version: env!("CARGO_PKG_VERSION").to_string(),
            },
        };
        write_json_atomic(&manifest_path, &manifest)?;
        manifest
    };
    write_default_policies(&ngit)?;
    Ok(manifest)
}

pub fn export_json_schemas(out_dir: &Path) -> CoreResult<SchemaExportReport> {
    fs::create_dir_all(out_dir).map_err(|source| CoreError::Io {
        path: out_dir.to_path_buf(),
        source,
    })?;
    let mut files = Vec::new();
    macro_rules! export_schema {
        ($ty:ty, $name:literal) => {{
            let path = out_dir.join(concat!($name, ".schema.json"));
            let schema = schema_for!($ty);
            write_json_atomic(&path, &schema)?;
            files.push(path);
        }};
    }
    export_schema!(Manifest, "manifest");
    export_schema!(CapturePolicy, "capture-policy");
    export_schema!(ReadinessPolicy, "evidence-rollup-policy");
    export_schema!(RepoSnapshot, "repo-snapshot");
    export_schema!(EventRecord, "event");
    export_schema!(DraftChange, "draft");
    export_schema!(AnnotationRecord, "annotation");
    export_schema!(EvidenceRecord, "evidence");
    export_schema!(DecisionContext, "decision-context");
    export_schema!(AcceptedChange, "accepted-change");
    export_schema!(RejectedChange, "rejected-change");
    export_schema!(LineageRecord, "lineage");
    export_schema!(DoctorReport, "doctor-report");
    export_schema!(TransactionManifest, "transaction");
    Ok(SchemaExportReport {
        schema_version: 1,
        exported_at: now_string(),
        files,
    })
}

pub fn migrate_legacy(repo_path: &Path) -> CoreResult<MigrationReport> {
    let root = initialized_root(repo_path)?;
    let _lock = StoreLock::acquire(&root, "migration")?;
    let ngit = ngit_dir(&root);
    let mut report = MigrationReport {
        schema_version: 1,
        created_at: now_string(),
        imported: BTreeMap::new(),
        skipped: vec![],
    };
    migrate_record_dir::<DraftChange>(
        &ngit.join("mutations/drafts"),
        &ngit.join("changes/drafts"),
        "drafts",
        &mut report,
    )?;
    migrate_record_dir::<AcceptedChange>(
        &ngit.join("mutations/accepted"),
        &ngit.join("changes/accepted"),
        "accepted",
        &mut report,
    )?;
    migrate_record_dir::<RejectedChange>(
        &ngit.join("mutations/rejected"),
        &ngit.join("changes/rejected"),
        "rejected",
        &mut report,
    )?;
    migrate_record_dir::<EvidenceRecord>(
        &ngit.join("checks"),
        &ngit.join("evidence/records"),
        "evidence",
        &mut report,
    )?;
    migrate_record_dir::<LineageRecord>(
        &ngit.join("lineage"),
        &ngit.join("lineage"),
        "lineage",
        &mut report,
    )?;
    let report_path = ngit.join("runtime/logs/migration-report.json");
    write_json_atomic(&report_path, &report)?;
    Ok(report)
}

pub fn status(repo_path: &Path) -> CoreResult<RepoStatus> {
    let root = git_root(repo_path)?;
    let initialized = ngit_dir(&root).join("manifest.json").exists();
    let snapshot = repo_snapshot(&root)?;
    let drafts = if initialized {
        list_drafts(&root)?
    } else {
        vec![]
    };
    let latest_evidence_state = drafts.last().map(|d| d.readiness.final_action.clone());
    Ok(RepoStatus {
        repo_root: root,
        initialized,
        snapshot,
        open_drafts: drafts.len(),
        latest_evidence_state,
    })
}

pub fn repo_snapshot(repo_path: &Path) -> CoreResult<RepoSnapshot> {
    let root = git_root(repo_path)?;
    let branch = git_output(&root, ["rev-parse", "--abbrev-ref", "HEAD"])?;
    let head = git_optional(&root, ["rev-parse", "HEAD"]);
    let head_parent_count = match &head {
        Some(_) => git_output(&root, ["rev-list", "--parents", "-n", "1", "HEAD"])
            .map(|line| line.split_whitespace().count().saturating_sub(1) as u32)
            .unwrap_or(0),
        None => 0,
    };
    let changed_files = parse_status_v2(&git_bytes(&root, ["status", "--porcelain=v2", "-z"])?);
    let staged_diff = git_bytes(
        &root,
        [
            "diff",
            "--no-ext-diff",
            "--no-color",
            "--binary",
            "--cached",
        ],
    )?;
    let worktree_diff = git_bytes(&root, ["diff", "--no-ext-diff", "--no-color", "--binary"])?;
    let recent_commits = recent_commits(&root);
    Ok(RepoSnapshot {
        schema_version: 1,
        captured_at: now_string(),
        repo_root: root,
        branch,
        head,
        head_parent_count,
        dirty: !changed_files.is_empty(),
        changed_files,
        recent_commits,
        staged_digest: sha256_prefixed(&staged_diff),
        worktree_digest: sha256_prefixed(&worktree_diff),
    })
}

pub fn capture(repo_path: &Path, options: CaptureOptions) -> CoreResult<DraftChange> {
    let root = initialized_root(repo_path)?;
    let _lock = StoreLock::acquire(&root, "capture")?;
    let policy = capture_policy(&root)?;
    let snapshot = repo_snapshot(&root)?;
    if snapshot.changed_files.is_empty() && !policy.allow_empty_capture {
        return Err(CoreError::InvalidInput(
            "empty capture is disallowed by policy".to_string(),
        ));
    }
    let capture_key = capture_key(&snapshot);
    if policy.dedupe.enabled {
        for draft in list_drafts(&root)? {
            if draft.capture.capture_key == capture_key {
                return Ok(draft);
            }
        }
    }
    let now = now_string();
    let draft_id = new_id("draft");
    let annotation_refs = Vec::new();
    let changed_paths = snapshot
        .changed_files
        .iter()
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    let mut draft = DraftChange {
        schema_version: 1,
        draft_id: draft_id.clone(),
        status: "draft".to_string(),
        created_at: now.clone(),
        updated_at: now,
        capture: CaptureInfo {
            source: "ngit-core".to_string(),
            trigger: options.trigger.clone(),
            capture_key,
            deduped_from: None,
        },
        repo_snapshot: snapshot,
        changed_paths,
        summary: String::new(),
        annotation_refs,
        evidence_refs: vec![],
        readiness: compute_readiness_for_refs(&root, &[])?,
    };
    draft.summary = format!(
        "Captured {} change affecting {} file{}.",
        options.trigger,
        draft.changed_paths.len(),
        if draft.changed_paths.len() == 1 {
            ""
        } else {
            "s"
        }
    );
    write_json_atomic(&draft_path(&root, &draft_id), &draft)?;
    if let Some(intent) = options.intent {
        let annotation = add_annotation(
            &root,
            AnnotationInput {
                owner_id: draft_id.clone(),
                annotation_type: "intent".to_string(),
                status: "supplied".to_string(),
                summary: Some(first_line(&intent)),
                body: intent,
                producer_kind: "human".to_string(),
            },
        )?;
        draft.annotation_refs.push(annotation.annotation_id);
        draft.updated_at = now_string();
        write_json_atomic(&draft_path(&root, &draft_id), &draft)?;
    }
    Ok(draft)
}

pub fn list_drafts(repo_path: &Path) -> CoreResult<Vec<DraftChange>> {
    let root = initialized_root(repo_path)?;
    read_dir_records(&ngit_dir(&root).join("changes/drafts"))
}

pub fn add_annotation(repo_path: &Path, input: AnnotationInput) -> CoreResult<AnnotationRecord> {
    let root = initialized_root(repo_path)?;
    let _lock = StoreLock::acquire(&root, "annotation")?;
    if resolve_record(&root, &input.owner_id).is_err() {
        return Err(CoreError::AnnotationOwnerMissing(input.owner_id));
    }
    let now = now_string();
    let id = new_id("annotation");
    let mut record = AnnotationRecord {
        schema_version: 1,
        annotation_id: id.clone(),
        owner: owner_for(&root, &input.owner_id)?,
        annotation_type: input.annotation_type,
        status: input.status,
        summary: input.summary,
        body: input.body,
        producer: Producer {
            kind: input.producer_kind,
            name: None,
            version: None,
        },
        created_at: now.clone(),
        updated_at: now,
        refs: vec![],
        payload_hash: String::new(),
    };
    record.payload_hash = payload_hash(&record)?;
    write_json_atomic(&annotation_path(&root, &id), &record)?;
    attach_annotation_ref(&root, &input.owner_id, &id)?;
    Ok(record)
}

pub fn list_annotations(repo_path: &Path, owner_id: &str) -> CoreResult<Vec<AnnotationRecord>> {
    let root = initialized_root(repo_path)?;
    let records: Vec<AnnotationRecord> = read_dir_records(&ngit_dir(&root).join("annotations"))?;
    Ok(records
        .into_iter()
        .filter(|record| record.owner.id == owner_id)
        .collect())
}

pub fn add_evidence_from_file(
    repo_path: &Path,
    draft_id: &str,
    file: &Path,
) -> CoreResult<EvidenceRecord> {
    let root = initialized_root(repo_path)?;
    if load_draft(&root, draft_id).is_err() {
        return Err(CoreError::EvidenceOwnerMissing(draft_id.to_string()));
    }
    let source = file.canonicalize().map_err(|source| CoreError::Io {
        path: file.to_path_buf(),
        source,
    })?;
    let evidence_id = new_id("evidence");
    let artifacts_dir = ngit_dir(&root)
        .join("evidence/artifacts")
        .join(&evidence_id);
    fs::create_dir_all(&artifacts_dir).map_err(|source| CoreError::Io {
        path: artifacts_dir.clone(),
        source,
    })?;
    let file_name = source
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("evidence-file");
    let artifact_path = artifacts_dir.join(file_name);
    fs::copy(&source, &artifact_path).map_err(|source| CoreError::Io {
        path: artifact_path.clone(),
        source,
    })?;
    let text = fs::read_to_string(&artifact_path).unwrap_or_default();
    let summary = if text.is_empty() {
        format!("External evidence file {file_name}.")
    } else {
        first_line(&text)
    };
    let artifacts = vec![artifact_ref(
        &root,
        "file",
        &artifact_path,
        false,
        Some(PathBuf::from(file_name)),
    )?];
    write_evidence_with_input(
        &root,
        EvidenceWriteInput {
            draft_id,
            evidence_id,
            evidence_type: "external",
            status: "needs_review",
            summary,
            command: None,
            artifacts,
        },
    )
}

pub fn run_evidence(
    repo_path: &Path,
    draft_id: &str,
    command: &[String],
) -> CoreResult<EvidenceRecord> {
    run_evidence_with_options(repo_path, draft_id, command, EvidenceRunOptions::default())
}

pub fn run_evidence_with_options(
    repo_path: &Path,
    draft_id: &str,
    command: &[String],
    options: EvidenceRunOptions,
) -> CoreResult<EvidenceRecord> {
    let root = initialized_root(repo_path)?;
    if command.is_empty() {
        return Err(CoreError::InvalidInput(
            "missing evidence command".to_string(),
        ));
    }
    if load_draft(&root, draft_id).is_err() {
        return Err(CoreError::EvidenceOwnerMissing(draft_id.to_string()));
    }
    let started = Instant::now();
    let mut child = Command::new(&command[0])
        .args(&command[1..])
        .current_dir(&root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| CoreError::Io {
            path: PathBuf::from(&command[0]),
            source,
        })?;
    let mut timed_out = false;
    loop {
        match child.try_wait().map_err(|source| CoreError::Io {
            path: PathBuf::from(&command[0]),
            source,
        })? {
            Some(_) => break,
            None if started.elapsed() >= options.timeout => {
                timed_out = true;
                child.kill().map_err(|source| CoreError::Io {
                    path: PathBuf::from(&command[0]),
                    source,
                })?;
                break;
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    let output = child.wait_with_output().map_err(|source| CoreError::Io {
        path: PathBuf::from(&command[0]),
        source,
    })?;
    let duration_ms = started.elapsed().as_millis();
    let status = if timed_out {
        "blocked"
    } else if output.status.success() {
        "passed"
    } else {
        "failed"
    };
    let exit_code = output.status.code().unwrap_or(-1);
    let command_evidence = CommandEvidence {
        argv: command.to_vec(),
        exit_code,
        duration_ms,
    };
    let evidence_id = new_id("evidence");
    let artifacts_dir = ngit_dir(&root)
        .join("evidence/artifacts")
        .join(&evidence_id);
    fs::create_dir_all(&artifacts_dir).map_err(|source| CoreError::Io {
        path: artifacts_dir.clone(),
        source,
    })?;
    let stdout_path = artifacts_dir.join("stdout.log");
    let stderr_path = artifacts_dir.join("stderr.log");
    let (stdout, stdout_truncated) = redact_and_truncate(&output.stdout, options.max_output_bytes);
    let (stderr, stderr_truncated) = redact_and_truncate(&output.stderr, options.max_output_bytes);
    fs::write(&stdout_path, stdout).map_err(|source| CoreError::Io {
        path: stdout_path.clone(),
        source,
    })?;
    fs::write(&stderr_path, stderr).map_err(|source| CoreError::Io {
        path: stderr_path.clone(),
        source,
    })?;
    let artifacts = vec![
        artifact_ref(&root, "stdout", &stdout_path, stdout_truncated, None)?,
        artifact_ref(&root, "stderr", &stderr_path, stderr_truncated, None)?,
    ];
    let summary = if timed_out {
        format!(
            "Command timed out after {} seconds.",
            options.timeout.as_secs()
        )
    } else {
        format!("Command exited with status {exit_code}.")
    };
    write_evidence_with_input(
        &root,
        EvidenceWriteInput {
            draft_id,
            evidence_id,
            evidence_type: "command_check",
            status,
            summary,
            command: Some(command_evidence),
            artifacts,
        },
    )
}

pub fn compute_readiness(repo_path: &Path, draft_id: &str) -> CoreResult<DecisionContext> {
    let root = initialized_root(repo_path)?;
    let draft = load_draft(&root, draft_id)?;
    compute_readiness_for_refs(&root, &draft.evidence_refs)
}

pub fn accept(
    repo_path: &Path,
    draft_id: &str,
    override_reason: Option<String>,
) -> CoreResult<AcceptedChange> {
    let root = initialized_root(repo_path)?;
    let _lock = StoreLock::acquire(&root, "accept")?;
    let draft = load_draft(&root, draft_id)?;
    let decision = compute_readiness_for_refs(&root, &draft.evidence_refs)?;
    let change_id = new_id("change");
    let acceptance_snapshot = repo_snapshot(&root)?;
    let accepted = AcceptedChange {
        schema_version: 1,
        change_id: change_id.clone(),
        draft_id: draft_id.to_string(),
        status: "accepted".to_string(),
        accepted_at: now_string(),
        accepted_by: Actor {
            kind: "human".to_string(),
            id: None,
        },
        repo_snapshot_at_capture: draft.repo_snapshot.clone(),
        repo_snapshot_at_acceptance: acceptance_snapshot.clone(),
        changed_paths: draft.changed_paths.clone(),
        annotation_refs: draft.annotation_refs.clone(),
        evidence_refs: draft.evidence_refs.clone(),
        decision_context: decision.clone(),
        override_reason: override_reason.clone(),
    };
    let lineage = accepted_lineage(&draft, &accepted, acceptance_snapshot, override_reason);
    finalize_terminal_transition(
        &root,
        "accept",
        draft_id,
        &accepted_path(&root, &change_id),
        &accepted,
        &lineage_path(&root, &lineage.lineage_id),
        &lineage,
    )?;
    Ok(accepted)
}

pub fn reject(repo_path: &Path, draft_id: &str, reason: String) -> CoreResult<RejectedChange> {
    if reason.trim().is_empty() {
        return Err(CoreError::InvalidInput(
            "reject requires a non-empty reason".to_string(),
        ));
    }
    let root = initialized_root(repo_path)?;
    let _lock = StoreLock::acquire(&root, "reject")?;
    let draft = load_draft(&root, draft_id)?;
    let decision_context = compute_readiness_for_refs(&root, &draft.evidence_refs)?;
    let rejected = RejectedChange {
        schema_version: 1,
        draft_id: draft_id.to_string(),
        status: "rejected".to_string(),
        rejected_at: now_string(),
        rejected_by: Actor {
            kind: "human".to_string(),
            id: None,
        },
        reason: reason.clone(),
        repo_snapshot_at_capture: draft.repo_snapshot.clone(),
        annotation_refs: draft.annotation_refs.clone(),
        evidence_refs: draft.evidence_refs.clone(),
        decision_context: decision_context.clone(),
    };
    let lineage = rejected_lineage(&draft, &rejected, reason);
    finalize_terminal_transition(
        &root,
        "reject",
        draft_id,
        &rejected_path(&root, draft_id),
        &rejected,
        &lineage_path(&root, &lineage.lineage_id),
        &lineage,
    )?;
    Ok(rejected)
}

pub fn history(repo_path: &Path) -> CoreResult<Vec<Record>> {
    let root = initialized_root(repo_path)?;
    let mut records = Vec::new();
    for accepted in read_dir_records::<AcceptedChange>(&ngit_dir(&root).join("changes/accepted"))? {
        records.push(Record::Accepted(accepted));
    }
    for rejected in read_dir_records::<RejectedChange>(&ngit_dir(&root).join("changes/rejected"))? {
        records.push(Record::Rejected(rejected));
    }
    Ok(records)
}

pub fn lineage(repo_path: &Path, id: &str) -> CoreResult<LineageRecord> {
    let root = initialized_root(repo_path)?;
    if let Ok(Record::Lineage(record)) = resolve_record(&root, id) {
        return Ok(record);
    }
    for record in read_dir_records::<LineageRecord>(&ngit_dir(&root).join("lineage"))? {
        if record.change_id.as_deref() == Some(id) || record.draft_id == id {
            return Ok(record);
        }
    }
    Err(CoreError::MissingRecord(id.to_string()))
}

pub fn show_record(repo_path: &Path, id: &str) -> CoreResult<Record> {
    let root = initialized_root(repo_path)?;
    resolve_record(&root, id)
}

pub fn watch_once(repo_path: &Path) -> CoreResult<EventRecord> {
    let root = initialized_root(repo_path)?;
    let snapshot = repo_snapshot(&root)?;
    Ok(event_from_snapshot(snapshot, "initial_observation"))
}

pub fn watch_capture_once(repo_path: &Path) -> CoreResult<(EventRecord, Option<DraftChange>)> {
    let root = initialized_root(repo_path)?;
    let _lock = StoreLock::acquire(&root, "watch")?;
    let snapshot = repo_snapshot(&root)?;
    let event = event_from_snapshot(snapshot.clone(), "repo_changed");
    write_json_atomic(&event_path(&root, &event.event_id), &event)?;
    let policy = capture_policy(&root)?;
    let should_capture = policy.mode == "auto"
        && event
            .signals
            .iter()
            .any(|signal| policy.triggers.iter().any(|trigger| trigger == signal));
    if should_capture {
        drop(_lock);
        let draft = capture(
            &root,
            CaptureOptions {
                trigger: "watch".to_string(),
                intent: None,
            },
        )?;
        Ok((event, Some(draft)))
    } else {
        Ok((event, None))
    }
}

fn event_from_snapshot(snapshot: RepoSnapshot, event_type: &str) -> EventRecord {
    EventRecord {
        schema_version: 1,
        event_id: new_id("event"),
        event_type: event_type.to_string(),
        created_at: now_string(),
        signals: event_signals(&snapshot),
        repo: EventRepo {
            branch: snapshot.branch,
            head: snapshot.head,
            staged_digest: snapshot.staged_digest,
            worktree_digest: snapshot.worktree_digest,
        },
        changed_files: snapshot.changed_files,
        related: BTreeMap::new(),
        detail: BTreeMap::new(),
    }
}

pub fn doctor(repo_path: &Path) -> CoreResult<DoctorReport> {
    let root = initialized_root(repo_path)?;
    let mut report = DoctorReport {
        schema_version: 1,
        checked_at: now_string(),
        issues: vec![],
    };
    let drafts =
        tolerant_records::<DraftChange>(&ngit_dir(&root).join("changes/drafts"), &mut report);
    let annotations =
        tolerant_records::<AnnotationRecord>(&ngit_dir(&root).join("annotations"), &mut report);
    let evidence =
        tolerant_records::<EvidenceRecord>(&ngit_dir(&root).join("evidence/records"), &mut report);
    let lineage_records =
        tolerant_records::<LineageRecord>(&ngit_dir(&root).join("lineage"), &mut report);
    let accepted_records =
        tolerant_records::<AcceptedChange>(&ngit_dir(&root).join("changes/accepted"), &mut report);
    let rejected_records =
        tolerant_records::<RejectedChange>(&ngit_dir(&root).join("changes/rejected"), &mut report);
    let transactions = tolerant_transaction_records(&root, &mut report);
    let mut ids = HashMap::new();
    for draft in &drafts {
        ids.insert(draft.draft_id.clone(), "draft");
    }
    for annotation in &annotations {
        ids.insert(annotation.annotation_id.clone(), "annotation");
    }
    for ev in &evidence {
        ids.insert(ev.evidence_id.clone(), "evidence");
    }
    for accepted in &accepted_records {
        ids.insert(accepted.change_id.clone(), "accepted_change");
        ids.insert(accepted.draft_id.clone(), "terminal_draft");
    }
    for rejected in &rejected_records {
        ids.insert(rejected.draft_id.clone(), "terminal_draft");
    }
    for annotation in &annotations {
        if !payload_matches_annotation(annotation) {
            report.issues.push(DoctorIssue {
                code: "payload_hash_mismatch".to_string(),
                message: format!(
                    "annotation {} payload hash does not match",
                    annotation.annotation_id
                ),
                path: Some(annotation_path(&root, &annotation.annotation_id)),
                record_id: Some(annotation.annotation_id.clone()),
            });
        }
        if !ids.contains_key(&annotation.owner.id) {
            report.issues.push(DoctorIssue {
                code: "annotation_owner_missing".to_string(),
                message: format!("annotation {} owner is missing", annotation.annotation_id),
                path: Some(annotation_path(&root, &annotation.annotation_id)),
                record_id: Some(annotation.annotation_id.clone()),
            });
        }
    }
    for ev in &evidence {
        if !payload_matches_evidence(ev) {
            report.issues.push(DoctorIssue {
                code: "payload_hash_mismatch".to_string(),
                message: format!("evidence {} payload hash does not match", ev.evidence_id),
                path: Some(evidence_path(&root, &ev.evidence_id)),
                record_id: Some(ev.evidence_id.clone()),
            });
        }
        if !ids.contains_key(&ev.owner.id) {
            report.issues.push(DoctorIssue {
                code: "evidence_owner_missing".to_string(),
                message: format!("evidence {} owner is missing", ev.evidence_id),
                path: Some(evidence_path(&root, &ev.evidence_id)),
                record_id: Some(ev.evidence_id.clone()),
            });
        }
        for artifact in &ev.artifacts {
            let path = root.join(&artifact.path);
            if !path.exists() {
                report.issues.push(DoctorIssue {
                    code: "artifact_missing".to_string(),
                    message: format!("artifact missing for evidence {}", ev.evidence_id),
                    path: Some(path),
                    record_id: Some(ev.evidence_id.clone()),
                });
            } else if let Ok(digest) = sha256_file(&path) {
                if digest != artifact.digest {
                    report.issues.push(DoctorIssue {
                        code: "artifact_digest_mismatch".to_string(),
                        message: format!(
                            "artifact digest mismatch for evidence {}",
                            ev.evidence_id
                        ),
                        path: Some(path),
                        record_id: Some(ev.evidence_id.clone()),
                    });
                }
            }
        }
    }
    for accepted in &accepted_records {
        if !lineage_records
            .iter()
            .any(|lineage| lineage.change_id.as_deref() == Some(&accepted.change_id))
        {
            report.issues.push(DoctorIssue {
                code: "terminal_without_lineage".to_string(),
                message: format!("accepted change {} has no lineage", accepted.change_id),
                path: Some(accepted_path(&root, &accepted.change_id)),
                record_id: Some(accepted.change_id.clone()),
            });
        }
    }
    for rejected in &rejected_records {
        if !lineage_records
            .iter()
            .any(|lineage| lineage.draft_id == rejected.draft_id)
        {
            report.issues.push(DoctorIssue {
                code: "terminal_without_lineage".to_string(),
                message: format!("rejected draft {} has no lineage", rejected.draft_id),
                path: Some(rejected_path(&root, &rejected.draft_id)),
                record_id: Some(rejected.draft_id.clone()),
            });
        }
    }
    for draft in &drafts {
        if ids.get(&draft.draft_id) == Some(&"terminal_draft") {
            report.issues.push(DoctorIssue {
                code: "draft_terminal_conflict".to_string(),
                message: format!("draft {} also has a terminal record", draft.draft_id),
                path: Some(draft_path(&root, &draft.draft_id)),
                record_id: Some(draft.draft_id.clone()),
            });
        }
    }
    for tx in transactions {
        if tx.state != "complete" {
            report.issues.push(DoctorIssue {
                code: "transaction_incomplete".to_string(),
                message: format!("transaction {} is {}", tx.transaction_id, tx.state),
                path: Some(
                    ngit_dir(&root)
                        .join("runtime/transactions")
                        .join(&tx.transaction_id)
                        .join("manifest.json"),
                ),
                record_id: Some(tx.transaction_id),
            });
        }
    }
    Ok(report)
}

fn write_default_policies(ngit: &Path) -> CoreResult<()> {
    let capture = ngit.join("policies/capture.json");
    if !capture.exists() {
        write_json_atomic(&capture, &default_capture_policy())?;
    }
    let evidence_rollup = ngit.join("policies/evidence-rollup.json");
    if !evidence_rollup.exists() {
        write_json_atomic(&evidence_rollup, &default_readiness_policy())?;
    }
    let evidence = ngit.join("policies/evidence.json");
    if !evidence.exists() {
        write_json_atomic(
            &evidence,
            &EvidencePolicy {
                schema_version: 1,
                durable_artifacts_required: true,
            },
        )?;
    }
    Ok(())
}

fn default_capture_policy() -> CapturePolicy {
    CapturePolicy {
        schema_version: 1,
        mode: "manual_only".to_string(),
        triggers: vec![],
        allow_empty_capture: false,
        dedupe: DedupePolicy {
            enabled: true,
            fields: vec![
                "branch".to_string(),
                "head".to_string(),
                "head_parent_count".to_string(),
                "changed_files".to_string(),
                "staged_digest".to_string(),
                "worktree_digest".to_string(),
            ],
        },
    }
}

fn default_readiness_policy() -> ReadinessPolicy {
    ReadinessPolicy {
        schema_version: 1,
        required_status_buckets: vec![
            "passed".to_string(),
            "failed".to_string(),
            "needs_review".to_string(),
            "blocked".to_string(),
        ],
        missing_evidence_status: "unresolved".to_string(),
        include_counts: true,
        include_producer_kinds: true,
        include_artifact_availability: true,
    }
}

fn capture_policy(root: &Path) -> CoreResult<CapturePolicy> {
    read_json(&ngit_dir(root).join("policies/capture.json"))
}

fn readiness_policy(root: &Path) -> CoreResult<ReadinessPolicy> {
    read_json(&ngit_dir(root).join("policies/evidence-rollup.json"))
        .or_else(|_| read_json(&ngit_dir(root).join("policies/readiness.json")))
}

fn compute_readiness_for_refs(root: &Path, refs: &[String]) -> CoreResult<DecisionContext> {
    let policy = readiness_policy(root).unwrap_or_else(|_| default_readiness_policy());
    let mut summary: BTreeMap<String, Vec<String>> = policy
        .required_status_buckets
        .iter()
        .map(|status| (status.clone(), Vec::new()))
        .collect();
    summary
        .entry(policy.missing_evidence_status.clone())
        .or_default();
    let mut unresolved_evidence = Vec::new();
    for id in refs {
        match read_json::<EvidenceRecord>(&evidence_path(root, id)) {
            Ok(evidence) => {
                summary
                    .entry(evidence.status.clone())
                    .or_default()
                    .push(evidence.evidence_id);
            }
            Err(_) => {
                unresolved_evidence.push(id.clone());
                summary
                    .entry(policy.missing_evidence_status.clone())
                    .or_default()
                    .push(id.clone());
            }
        }
    }
    let evidence_state = if refs.is_empty() {
        "no_evidence"
    } else {
        "evidence_present"
    }
    .to_string();
    let rollup_summary = evidence_rollup_summary(&summary, refs.len());
    Ok(DecisionContext {
        schema_version: 1,
        computed_at: now_string(),
        policy_hash: payload_hash(&policy)?,
        evidence_summary: summary,
        unresolved_evidence,
        deterministic_action: evidence_state.clone(),
        review_action: None,
        final_action: evidence_state,
        override_required: false,
        override_targets: vec![],
        summary: rollup_summary,
    })
}

struct EvidenceWriteInput<'a> {
    draft_id: &'a str,
    evidence_id: String,
    evidence_type: &'a str,
    status: &'a str,
    summary: String,
    command: Option<CommandEvidence>,
    artifacts: Vec<ArtifactRef>,
}

fn write_evidence_with_input(
    root: &Path,
    input: EvidenceWriteInput<'_>,
) -> CoreResult<EvidenceRecord> {
    let _lock = StoreLock::acquire(root, "evidence")?;
    let mut evidence = EvidenceRecord {
        schema_version: 1,
        evidence_id: input.evidence_id.clone(),
        owner: RecordOwner {
            owner_type: "draft".to_string(),
            id: input.draft_id.to_string(),
        },
        evidence_type: input.evidence_type.to_string(),
        status: input.status.to_string(),
        summary: input.summary,
        created_at: now_string(),
        producer: Producer {
            kind: if input.evidence_type == "command_check" {
                "command".to_string()
            } else {
                "external".to_string()
            },
            name: None,
            version: None,
        },
        command: input.command,
        findings: vec![],
        artifacts: input.artifacts,
        payload_hash: String::new(),
    };
    evidence.payload_hash = payload_hash(&evidence)?;
    write_json_atomic(&evidence_path(root, &input.evidence_id), &evidence)?;
    let mut draft = load_draft(root, input.draft_id)?;
    draft.evidence_refs.push(input.evidence_id);
    draft.readiness = compute_readiness_for_refs(root, &draft.evidence_refs)?;
    draft.updated_at = now_string();
    write_json_atomic(&draft_path(root, input.draft_id), &draft)?;
    Ok(evidence)
}

fn attach_annotation_ref(root: &Path, owner_id: &str, annotation_id: &str) -> CoreResult<()> {
    if let Record::Draft(mut draft) = resolve_record(root, owner_id)? {
        if !draft.annotation_refs.iter().any(|id| id == annotation_id) {
            draft.annotation_refs.push(annotation_id.to_string());
            draft.updated_at = now_string();
            write_json_atomic(&draft_path(root, &draft.draft_id), &draft)?;
        }
    }
    Ok(())
}

fn owner_for(root: &Path, id: &str) -> CoreResult<RecordOwner> {
    let owner_type = match resolve_record(root, id)? {
        Record::Draft(_) => "draft",
        Record::Accepted(_) => "accepted_change",
        Record::Rejected(_) => "rejected_draft",
        Record::Evidence(_) => "evidence",
        Record::Lineage(_) => "lineage",
        Record::Annotation(_) => "annotation",
        Record::Event(_) => "event",
    };
    Ok(RecordOwner {
        owner_type: owner_type.to_string(),
        id: id.to_string(),
    })
}

fn accepted_lineage(
    draft: &DraftChange,
    accepted: &AcceptedChange,
    acceptance_snapshot: RepoSnapshot,
    override_reason: Option<String>,
) -> LineageRecord {
    LineageRecord {
        schema_version: 1,
        lineage_id: new_id("lineage"),
        event_type: "change_accepted".to_string(),
        change_id: Some(accepted.change_id.clone()),
        draft_id: draft.draft_id.clone(),
        created_at: now_string(),
        repo: BTreeMap::from([
            ("capture".to_string(), draft.repo_snapshot.clone()),
            ("acceptance".to_string(), acceptance_snapshot),
        ]),
        changed_paths: draft.changed_paths.clone(),
        annotation_refs: draft.annotation_refs.clone(),
        evidence_refs: draft.evidence_refs.clone(),
        decision_context: accepted.decision_context.clone(),
        decision: Some(TerminalDecision {
            action: "accepted".to_string(),
            reason: override_reason
                .clone()
                .unwrap_or_else(|| accepted.decision_context.summary.clone()),
        }),
        override_reason,
        links: LineageLinks {
            event_ids: vec![],
            supersedes: vec![],
            related_changes: vec![],
        },
    }
}

fn rejected_lineage(
    draft: &DraftChange,
    rejected: &RejectedChange,
    reason: String,
) -> LineageRecord {
    LineageRecord {
        schema_version: 1,
        lineage_id: new_id("lineage"),
        event_type: "change_rejected".to_string(),
        change_id: None,
        draft_id: draft.draft_id.clone(),
        created_at: now_string(),
        repo: BTreeMap::from([("capture".to_string(), draft.repo_snapshot.clone())]),
        changed_paths: draft.changed_paths.clone(),
        annotation_refs: draft.annotation_refs.clone(),
        evidence_refs: draft.evidence_refs.clone(),
        decision_context: rejected.decision_context.clone(),
        decision: Some(TerminalDecision {
            action: "rejected".to_string(),
            reason,
        }),
        override_reason: None,
        links: LineageLinks {
            event_ids: vec![],
            supersedes: vec![],
            related_changes: vec![],
        },
    }
}

fn finalize_terminal_transition<T: Serialize, L: Serialize>(
    root: &Path,
    operation: &str,
    draft_id: &str,
    terminal_path: &Path,
    terminal: &T,
    lineage_path: &Path,
    lineage: &L,
) -> CoreResult<()> {
    let transaction_id = new_id("transaction");
    let transaction_dir = ngit_dir(root)
        .join("runtime/transactions")
        .join(&transaction_id);
    fs::create_dir_all(&transaction_dir).map_err(|source| CoreError::Io {
        path: transaction_dir.clone(),
        source,
    })?;
    let mut manifest = TransactionManifest {
        schema_version: 1,
        transaction_id: transaction_id.clone(),
        operation: operation.to_string(),
        draft_id: draft_id.to_string(),
        state: "pending".to_string(),
        created_at: now_string(),
        updated_at: now_string(),
        terminal_path: relative_to_root(root, terminal_path),
        lineage_path: relative_to_root(root, lineage_path),
        draft_path: relative_to_root(root, &draft_path(root, draft_id)),
    };
    write_json_atomic(&transaction_dir.join("manifest.json"), &manifest)?;
    write_json_atomic(&transaction_dir.join("terminal.json"), terminal)?;
    write_json_atomic(&transaction_dir.join("lineage.json"), lineage)?;
    write_json_atomic(terminal_path, terminal)?;
    write_json_atomic(lineage_path, lineage)?;
    fs::remove_file(draft_path(root, draft_id)).map_err(|source| CoreError::Io {
        path: draft_path(root, draft_id),
        source,
    })?;
    manifest.state = "complete".to_string();
    manifest.updated_at = now_string();
    write_json_atomic(&transaction_dir.join("manifest.json"), &manifest)?;
    Ok(())
}

fn parse_status_v2(output: &[u8]) -> Vec<ChangedFile> {
    let mut files = Vec::new();
    let mut entries = output
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty());
    while let Some(entry) = entries.next() {
        let text = String::from_utf8_lossy(entry);
        if let Some(rest) = text.strip_prefix("1 ") {
            if let Some((status, path)) = status_v2_status_and_path(rest, 7) {
                push_changed_file(&mut files, status, path);
            }
        } else if let Some(rest) = text.strip_prefix("2 ") {
            if let Some((status, path)) = status_v2_status_and_path(rest, 8) {
                push_changed_file(&mut files, status, path);
                let _ = entries.next();
            }
        } else if let Some(rest) = text.strip_prefix("u ") {
            if let Some((status, path)) = status_v2_status_and_path(rest, 9) {
                push_changed_file(&mut files, status, path);
            }
        } else if let Some(path) = text.strip_prefix("? ") {
            push_changed_file(&mut files, "??", path);
        } else if let Some(path) = text.strip_prefix("! ") {
            push_changed_file(&mut files, "!!", path);
        }
    }
    files.sort();
    files.dedup();
    files
}

fn status_v2_status_and_path(entry: &str, field_count: usize) -> Option<(&str, &str)> {
    let mut status = None;
    let mut start = 0usize;
    for field_index in 0..field_count {
        let remaining = &entry[start..];
        let end = remaining.find(' ')?;
        let field = &remaining[..end];
        if field_index == 0 {
            status = Some(field);
        }
        start += end + 1;
    }
    Some((status?, &entry[start..]))
}

fn push_changed_file(files: &mut Vec<ChangedFile>, status: &str, path: &str) {
    if path.starts_with(".ngit/") || path == ".ngit" {
        return;
    }
    let mut chars = status.chars();
    files.push(ChangedFile {
        path: path.to_string(),
        index_status: chars.next().unwrap_or(' ').to_string(),
        worktree_status: chars.next().unwrap_or(' ').to_string(),
    });
}

#[allow(dead_code)]
fn parse_status(output: &str) -> Vec<ChangedFile> {
    let mut files = Vec::new();
    for line in output.lines() {
        if line.len() < 4 {
            continue;
        }
        let index_status = line.chars().next().unwrap_or(' ').to_string();
        let worktree_status = line.chars().nth(1).unwrap_or(' ').to_string();
        let path = line[3..].to_string();
        if path.starts_with(".ngit/") || path == ".ngit" {
            continue;
        }
        files.push(ChangedFile {
            path,
            index_status,
            worktree_status,
        });
    }
    files.sort();
    files
}

fn recent_commits(root: &Path) -> Vec<CommitSummary> {
    git_output(root, ["log", "-5", "--pretty=%H%x00%s"])
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let (sha, subject) = line.split_once('\0')?;
            Some(CommitSummary {
                sha: sha.to_string(),
                subject: subject.to_string(),
            })
        })
        .collect()
}

fn event_signals(snapshot: &RepoSnapshot) -> Vec<String> {
    let mut signals = vec!["initial_observation".to_string()];
    if snapshot
        .changed_files
        .iter()
        .any(|file| !file.index_status.trim().is_empty())
    {
        signals.push("on_stage".to_string());
    }
    signals
}

fn capture_key(snapshot: &RepoSnapshot) -> String {
    sha256_prefixed(
        serde_json::to_vec(&(
            &snapshot.branch,
            &snapshot.head,
            snapshot.head_parent_count,
            &snapshot.changed_files,
            &snapshot.staged_digest,
            &snapshot.worktree_digest,
        ))
        .unwrap()
        .as_slice(),
    )
}

fn resolve_record(root: &Path, id: &str) -> CoreResult<Record> {
    let mut matches = Vec::new();
    for draft in read_dir_records::<DraftChange>(&ngit_dir(root).join("changes/drafts"))? {
        if draft.draft_id.starts_with(id) {
            matches.push(Record::Draft(draft));
        }
    }
    for annotation in read_dir_records::<AnnotationRecord>(&ngit_dir(root).join("annotations"))? {
        if annotation.annotation_id.starts_with(id) {
            matches.push(Record::Annotation(annotation));
        }
    }
    for evidence in read_dir_records::<EvidenceRecord>(&ngit_dir(root).join("evidence/records"))? {
        if evidence.evidence_id.starts_with(id) {
            matches.push(Record::Evidence(evidence));
        }
    }
    for accepted in read_dir_records::<AcceptedChange>(&ngit_dir(root).join("changes/accepted"))? {
        if accepted.change_id.starts_with(id) {
            matches.push(Record::Accepted(accepted));
        }
    }
    for rejected in read_dir_records::<RejectedChange>(&ngit_dir(root).join("changes/rejected"))? {
        if rejected.draft_id.starts_with(id) {
            matches.push(Record::Rejected(rejected));
        }
    }
    for lineage in read_dir_records::<LineageRecord>(&ngit_dir(root).join("lineage"))? {
        if lineage.lineage_id.starts_with(id) {
            matches.push(Record::Lineage(lineage));
        }
    }
    for event in read_dir_records::<EventRecord>(&ngit_dir(root).join("events"))? {
        if event.event_id.starts_with(id) {
            matches.push(Record::Event(event));
        }
    }
    match matches.len() {
        0 => Err(CoreError::MissingRecord(id.to_string())),
        1 => Ok(matches.remove(0)),
        _ => Err(CoreError::AmbiguousId(id.to_string())),
    }
}

fn load_draft(root: &Path, draft_id: &str) -> CoreResult<DraftChange> {
    read_json(&draft_path(root, draft_id))
        .map_err(|_| CoreError::MissingDraft(draft_id.to_string()))
}

fn read_dir_records<T: DeserializeOwned>(dir: &Path) -> CoreResult<Vec<T>> {
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut records = Vec::new();
    let mut entries = fs::read_dir(dir)
        .map_err(|source| CoreError::Io {
            path: dir.to_path_buf(),
            source,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| CoreError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        if path.extension() == Some(OsStr::new("json")) {
            records.push(read_json(&path)?);
        }
    }
    Ok(records)
}

fn tolerant_records<T: DeserializeOwned>(dir: &Path, report: &mut DoctorReport) -> Vec<T> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) if !dir.exists() => return vec![],
        Err(source) => {
            report.issues.push(DoctorIssue {
                code: "record_scan_failed".to_string(),
                message: source.to_string(),
                path: Some(dir.to_path_buf()),
                record_id: None,
            });
            return vec![];
        }
    };
    let mut paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension() == Some(OsStr::new("json")))
        .collect::<Vec<_>>();
    paths.sort();
    let mut records = Vec::new();
    for path in paths {
        match read_json::<T>(&path) {
            Ok(record) => records.push(record),
            Err(err) => report.issues.push(DoctorIssue {
                code: "record_malformed".to_string(),
                message: err.to_string(),
                path: Some(path),
                record_id: None,
            }),
        }
    }
    records
}

fn tolerant_transaction_records(
    root: &Path,
    report: &mut DoctorReport,
) -> Vec<TransactionManifest> {
    let dir = ngit_dir(root).join("runtime/transactions");
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) if !dir.exists() => return vec![],
        Err(source) => {
            report.issues.push(DoctorIssue {
                code: "record_scan_failed".to_string(),
                message: source.to_string(),
                path: Some(dir),
                record_id: None,
            });
            return vec![];
        }
    };
    let mut manifests = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path().join("manifest.json");
        if path.exists() {
            match read_json::<TransactionManifest>(&path) {
                Ok(record) => manifests.push(record),
                Err(err) => report.issues.push(DoctorIssue {
                    code: "record_malformed".to_string(),
                    message: err.to_string(),
                    path: Some(path),
                    record_id: None,
                }),
            }
        }
    }
    manifests
}

fn migrate_record_dir<T>(
    source_dir: &Path,
    target_dir: &Path,
    bucket: &str,
    report: &mut MigrationReport,
) -> CoreResult<()>
where
    T: DeserializeOwned + Serialize,
{
    if !source_dir.exists() {
        return Ok(());
    }
    fs::create_dir_all(target_dir).map_err(|source| CoreError::Io {
        path: target_dir.to_path_buf(),
        source,
    })?;
    let mut imported = 0usize;
    let entries = fs::read_dir(source_dir)
        .map_err(|source| CoreError::Io {
            path: source_dir.to_path_buf(),
            source,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| CoreError::Io {
            path: source_dir.to_path_buf(),
            source,
        })?;
    for entry in entries {
        let source_path = entry.path();
        if source_path.extension() != Some(OsStr::new("json")) {
            continue;
        }
        match read_json::<T>(&source_path) {
            Ok(record) => {
                let target_path = target_dir.join(
                    source_path
                        .file_name()
                        .unwrap_or_else(|| OsStr::new("legacy-record.json")),
                );
                if !target_path.exists() {
                    write_json_atomic(&target_path, &record)?;
                    imported += 1;
                }
            }
            Err(err) => report.skipped.push(MigrationIssue {
                path: source_path,
                reason: err.to_string(),
            }),
        }
    }
    report.imported.insert(bucket.to_string(), imported);
    Ok(())
}

fn read_json<T: DeserializeOwned>(path: &Path) -> CoreResult<T> {
    let file = File::open(path).map_err(|source| CoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let value: serde_json::Value =
        serde_json::from_reader(file).map_err(|source| CoreError::MalformedJson {
            path: path.to_path_buf(),
            source,
        })?;
    if let Some(version) = value.get("schema_version").and_then(|value| value.as_u64()) {
        if version != 1 {
            return Err(CoreError::UnsupportedSchema {
                path: path.to_path_buf(),
                version: version as u32,
            });
        }
    }
    serde_json::from_value(value).map_err(|source| CoreError::MalformedJson {
        path: path.to_path_buf(),
        source,
    })
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> CoreResult<()> {
    let dir = path.parent().ok_or_else(|| {
        CoreError::InvalidInput(format!("path has no parent: {}", path.display()))
    })?;
    fs::create_dir_all(dir).map_err(|source| CoreError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    let mut tmp = NamedTempFile::new_in(dir).map_err(|source| CoreError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    serde_json::to_writer_pretty(&mut tmp, value).map_err(|source| CoreError::MalformedJson {
        path: path.to_path_buf(),
        source,
    })?;
    tmp.write_all(b"\n").map_err(|source| CoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    tmp.as_file().sync_all().map_err(|source| CoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    tmp.persist(path).map_err(|err| CoreError::Io {
        path: path.to_path_buf(),
        source: err.error,
    })?;
    Ok(())
}

struct StoreLock {
    path: PathBuf,
}

impl StoreLock {
    fn acquire(root: &Path, name: &str) -> CoreResult<Self> {
        let path = ngit_dir(root)
            .join("runtime/locks")
            .join(format!("{name}.lock"));
        if path.exists() {
            let metadata = fs::metadata(&path).map_err(|source| CoreError::Io {
                path: path.clone(),
                source,
            })?;
            if metadata
                .modified()
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .unwrap_or_default()
                < Duration::from_secs(300)
            {
                return Err(CoreError::LockHeld(path));
            }
            let _ = fs::remove_file(&path);
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|source| CoreError::Io {
                path: path.clone(),
                source,
            })?;
        writeln!(file, "pid={}", std::process::id()).map_err(|source| CoreError::Io {
            path: path.clone(),
            source,
        })?;
        Ok(Self { path })
    }
}

impl Drop for StoreLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn git_root(path: &Path) -> CoreResult<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(path)
        .output()
        .map_err(|source| CoreError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if !output.status.success() {
        return Err(CoreError::NotGitRepo(path.to_path_buf()));
    }
    Ok(PathBuf::from(
        String::from_utf8_lossy(&output.stdout).trim(),
    ))
}

fn initialized_root(path: &Path) -> CoreResult<PathBuf> {
    let root = git_root(path)?;
    if !ngit_dir(&root).join("manifest.json").exists() {
        return Err(CoreError::NotInitialized(root));
    }
    Ok(root)
}

fn git_output<const N: usize>(root: &Path, args: [&str; N]) -> CoreResult<String> {
    let output = git_bytes(root, args)?;
    Ok(String::from_utf8_lossy(&output).trim_end().to_string())
}

fn git_bytes<const N: usize>(root: &Path, args: [&str; N]) -> CoreResult<Vec<u8>> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|source| CoreError::Io {
            path: root.to_path_buf(),
            source,
        })?;
    if !output.status.success() {
        return Err(CoreError::GitCommandFailed(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    Ok(output.stdout)
}

fn git_optional<const N: usize>(root: &Path, args: [&str; N]) -> Option<String> {
    git_output(root, args).ok().filter(|text| !text.is_empty())
}

fn ngit_dir(root: &Path) -> PathBuf {
    root.join(".ngit")
}

fn durable_dirs(ngit: &Path) -> Vec<PathBuf> {
    [
        "policies",
        "events",
        "changes/drafts",
        "changes/accepted",
        "changes/rejected",
        "annotations",
        "evidence/records",
        "evidence/artifacts",
        "lineage",
    ]
    .iter()
    .map(|dir| ngit.join(dir))
    .collect()
}

fn ephemeral_dirs(ngit: &Path) -> Vec<PathBuf> {
    [
        "runtime/locks",
        "runtime/watch",
        "runtime/logs",
        "runtime/cache",
        "runtime/transactions",
    ]
    .iter()
    .map(|dir| ngit.join(dir))
    .collect()
}

fn draft_path(root: &Path, id: &str) -> PathBuf {
    ngit_dir(root)
        .join("changes/drafts")
        .join(format!("{id}.json"))
}

fn accepted_path(root: &Path, id: &str) -> PathBuf {
    ngit_dir(root)
        .join("changes/accepted")
        .join(format!("{id}.json"))
}

fn rejected_path(root: &Path, id: &str) -> PathBuf {
    ngit_dir(root)
        .join("changes/rejected")
        .join(format!("{id}.json"))
}

fn annotation_path(root: &Path, id: &str) -> PathBuf {
    ngit_dir(root)
        .join("annotations")
        .join(format!("{id}.json"))
}

fn evidence_path(root: &Path, id: &str) -> PathBuf {
    ngit_dir(root)
        .join("evidence/records")
        .join(format!("{id}.json"))
}

fn lineage_path(root: &Path, id: &str) -> PathBuf {
    ngit_dir(root).join("lineage").join(format!("{id}.json"))
}

fn event_path(root: &Path, id: &str) -> PathBuf {
    ngit_dir(root).join("events").join(format!("{id}.json"))
}

fn now_string() -> String {
    OffsetDateTime::now_utc().format(&Rfc3339).unwrap()
}

fn new_id(prefix: &str) -> String {
    let ts = OffsetDateTime::now_utc()
        .format(&time::macros::format_description!(
            "[year][month][day]T[hour][minute][second]Z"
        ))
        .unwrap();
    let mut rng = rand::rng();
    let suffix = Alphanumeric.sample_string(&mut rng, 8).to_ascii_lowercase();
    format!("{prefix}-{ts}-{suffix}")
}

fn payload_hash<T: Serialize>(value: &T) -> CoreResult<String> {
    let bytes = serde_json::to_vec(value).map_err(|source| CoreError::MalformedJson {
        path: PathBuf::from("<payload>"),
        source,
    })?;
    Ok(sha256_prefixed(&bytes))
}

fn sha256_prefixed(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

fn sha256_file(path: &Path) -> CoreResult<String> {
    let bytes = fs::read(path).map_err(|source| CoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(sha256_prefixed(&bytes))
}

fn artifact_ref(
    root: &Path,
    kind: &str,
    path: &Path,
    truncated: bool,
    original_path: Option<PathBuf>,
) -> CoreResult<ArtifactRef> {
    let size_bytes = fs::metadata(path)
        .map_err(|source| CoreError::Io {
            path: path.to_path_buf(),
            source,
        })?
        .len();
    Ok(ArtifactRef {
        kind: kind.to_string(),
        path: relative_to_root(root, path),
        digest: sha256_file(path)?,
        size_bytes,
        truncated,
        original_path,
    })
}

fn first_line(text: &str) -> String {
    text.lines()
        .next()
        .unwrap_or("")
        .trim()
        .chars()
        .take(120)
        .collect()
}

fn evidence_rollup_summary(summary: &BTreeMap<String, Vec<String>>, total_refs: usize) -> String {
    if total_refs == 0 {
        return "No evidence has been attached.".to_string();
    }
    let parts = summary
        .iter()
        .filter(|(_, ids)| !ids.is_empty())
        .map(|(status, ids)| format!("{} {}", ids.len(), status))
        .collect::<Vec<_>>();
    if parts.is_empty() {
        "Evidence refs are attached, but no evidence records could be summarized.".to_string()
    } else {
        format!("Evidence rollup: {}.", parts.join(", "))
    }
}

fn relative_to_root(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

fn redact(bytes: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(bytes);
    let mut out = Vec::new();
    for word in text.split_whitespace() {
        if word.starts_with("sk-") || word.to_ascii_lowercase().contains("token=") {
            out.extend_from_slice(b"[REDACTED] ");
        } else {
            out.extend_from_slice(word.as_bytes());
            out.push(b' ');
        }
    }
    out
}

fn redact_and_truncate(bytes: &[u8], max_bytes: usize) -> (Vec<u8>, bool) {
    let redacted = redact(bytes);
    if redacted.len() <= max_bytes {
        return (redacted, false);
    }
    let mut truncated = redacted[..max_bytes].to_vec();
    truncated.extend_from_slice(b"\n[TRUNCATED]\n");
    (truncated, true)
}

fn payload_matches_annotation(record: &AnnotationRecord) -> bool {
    let mut copy = record.clone();
    let expected = copy.payload_hash.clone();
    copy.payload_hash.clear();
    payload_hash(&copy)
        .map(|actual| actual == expected)
        .unwrap_or(false)
}

fn payload_matches_evidence(record: &EvidenceRecord) -> bool {
    let mut copy = record.clone();
    let expected = copy.payload_hash.clone();
    copy.payload_hash.clear();
    payload_hash(&copy)
        .map(|actual| actual == expected)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn git_repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        Command::new("git")
            .args(["add", "README.md"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        dir
    }

    fn success_command() -> Vec<String> {
        vec!["rustc".to_string(), "--version".to_string()]
    }

    #[test]
    fn init_creates_layout_and_is_idempotent() {
        let repo = git_repo();
        init(repo.path()).unwrap();
        init(repo.path()).unwrap();
        assert!(repo.path().join(".ngit/manifest.json").exists());
        assert!(repo.path().join(".ngit/annotations").exists());
        assert!(repo.path().join(".ngit/evidence/records").exists());
    }

    #[test]
    fn capture_with_intent_adds_annotation_and_dedupes() {
        let repo = git_repo();
        init(repo.path()).unwrap();
        fs::write(repo.path().join("README.md"), "hello again\n").unwrap();
        let draft = capture(
            repo.path(),
            CaptureOptions {
                trigger: "manual".to_string(),
                intent: Some("Improve the README greeting.".to_string()),
            },
        )
        .unwrap();
        assert_eq!(draft.annotation_refs.len(), 1);
        let same = capture(
            repo.path(),
            CaptureOptions {
                trigger: "manual".to_string(),
                intent: None,
            },
        )
        .unwrap();
        assert_eq!(draft.draft_id, same.draft_id);
    }

    #[test]
    fn evidence_rollup_and_acceptance_write_lineage() {
        let repo = git_repo();
        init(repo.path()).unwrap();
        fs::write(repo.path().join("README.md"), "changed\n").unwrap();
        let draft = capture(
            repo.path(),
            CaptureOptions {
                trigger: "manual".to_string(),
                intent: None,
            },
        )
        .unwrap();
        run_evidence(repo.path(), &draft.draft_id, &success_command()).unwrap();
        let context = compute_readiness(repo.path(), &draft.draft_id).unwrap();
        assert_eq!(context.final_action, "evidence_present");
        assert_eq!(context.evidence_summary["passed"].len(), 1);
        assert!(!context.override_required);
        let accepted = accept(repo.path(), &draft.draft_id, None).unwrap();
        assert!(lineage(repo.path(), &accepted.change_id).is_ok());
        assert!(list_drafts(repo.path()).unwrap().is_empty());
    }

    #[test]
    fn reject_requires_reason_and_writes_lineage() {
        let repo = git_repo();
        init(repo.path()).unwrap();
        fs::write(repo.path().join("README.md"), "changed\n").unwrap();
        let draft = capture(
            repo.path(),
            CaptureOptions {
                trigger: "manual".to_string(),
                intent: None,
            },
        )
        .unwrap();
        assert!(reject(repo.path(), &draft.draft_id, "".to_string()).is_err());
        let rejected = reject(repo.path(), &draft.draft_id, "not needed".to_string()).unwrap();
        assert!(lineage(repo.path(), &rejected.draft_id).is_ok());
    }

    #[test]
    fn doctor_detects_missing_artifact() {
        let repo = git_repo();
        init(repo.path()).unwrap();
        fs::write(repo.path().join("README.md"), "changed\n").unwrap();
        let draft = capture(
            repo.path(),
            CaptureOptions {
                trigger: "manual".to_string(),
                intent: None,
            },
        )
        .unwrap();
        let ev = run_evidence(repo.path(), &draft.draft_id, &success_command()).unwrap();
        let path = repo.path().join(&ev.artifacts[0].path);
        fs::remove_file(path).unwrap();
        let report = doctor(repo.path()).unwrap();
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "artifact_missing"));
    }

    #[test]
    fn missing_evidence_ref_is_unresolved_but_does_not_block_acceptance() {
        let repo = git_repo();
        init(repo.path()).unwrap();
        fs::write(repo.path().join("README.md"), "changed\n").unwrap();
        let mut draft = capture(
            repo.path(),
            CaptureOptions {
                trigger: "manual".to_string(),
                intent: None,
            },
        )
        .unwrap();
        draft.evidence_refs.push("evidence-missing".to_string());
        write_json_atomic(&draft_path(repo.path(), &draft.draft_id), &draft).unwrap();

        let context = compute_readiness(repo.path(), &draft.draft_id).unwrap();
        assert_eq!(context.final_action, "evidence_present");
        assert_eq!(context.unresolved_evidence, vec!["evidence-missing"]);
        assert_eq!(
            context.evidence_summary["unresolved"],
            vec!["evidence-missing"]
        );
        assert!(!context.override_required);

        let accepted = accept(repo.path(), &draft.draft_id, None).unwrap();
        let lineage = lineage(repo.path(), &accepted.change_id).unwrap();
        assert_eq!(
            lineage.decision_context.unresolved_evidence,
            vec!["evidence-missing"]
        );
    }

    #[test]
    fn doctor_reports_malformed_json_and_payload_hash_mismatch() {
        let repo = git_repo();
        init(repo.path()).unwrap();
        fs::write(repo.path().join("README.md"), "changed\n").unwrap();
        let draft = capture(
            repo.path(),
            CaptureOptions {
                trigger: "manual".to_string(),
                intent: Some("keep rationale".to_string()),
            },
        )
        .unwrap();
        fs::write(
            repo.path().join(".ngit/evidence/records/bad.json"),
            "{not-json",
        )
        .unwrap();
        let annotation_id = draft.annotation_refs[0].clone();
        let mut annotation: AnnotationRecord =
            read_json(&annotation_path(repo.path(), &annotation_id)).unwrap();
        annotation.body = "tampered".to_string();
        write_json_atomic(&annotation_path(repo.path(), &annotation_id), &annotation).unwrap();

        let report = doctor(repo.path()).unwrap();
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "record_malformed"));
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "payload_hash_mismatch"));
    }

    #[test]
    fn external_evidence_is_copied_to_artifacts() {
        let repo = git_repo();
        init(repo.path()).unwrap();
        fs::write(repo.path().join("README.md"), "changed\n").unwrap();
        let draft = capture(
            repo.path(),
            CaptureOptions {
                trigger: "manual".to_string(),
                intent: None,
            },
        )
        .unwrap();
        let source = repo.path().join("evidence.txt");
        fs::write(&source, "manual evidence\n").unwrap();
        let evidence = add_evidence_from_file(repo.path(), &draft.draft_id, &source).unwrap();
        assert_eq!(evidence.artifacts.len(), 1);
        assert!(repo.path().join(&evidence.artifacts[0].path).exists());
        assert_eq!(
            evidence.artifacts[0].original_path,
            Some(PathBuf::from("evidence.txt"))
        );
    }

    #[test]
    fn command_evidence_records_failure_and_truncation() {
        let repo = git_repo();
        init(repo.path()).unwrap();
        fs::write(repo.path().join("README.md"), "changed\n").unwrap();
        let draft = capture(
            repo.path(),
            CaptureOptions {
                trigger: "manual".to_string(),
                intent: None,
            },
        )
        .unwrap();
        let evidence = run_evidence_with_options(
            repo.path(),
            &draft.draft_id,
            &[
                "rustc".to_string(),
                "--definitely-not-a-real-rustc-flag".to_string(),
            ],
            EvidenceRunOptions {
                timeout: Duration::from_secs(120),
                max_output_bytes: 8,
            },
        )
        .unwrap();
        assert_eq!(evidence.status, "failed");
        assert!(evidence.artifacts.iter().any(|artifact| artifact.truncated));
    }

    #[test]
    fn status_parses_paths_with_spaces() {
        let repo = git_repo();
        init(repo.path()).unwrap();
        fs::write(repo.path().join("file with spaces.txt"), "changed\n").unwrap();
        let snapshot = repo_snapshot(repo.path()).unwrap();
        assert!(snapshot
            .changed_files
            .iter()
            .any(|file| file.path == "file with spaces.txt"));
    }

    #[test]
    fn doctor_reports_incomplete_transaction() {
        let repo = git_repo();
        init(repo.path()).unwrap();
        let dir = repo
            .path()
            .join(".ngit/runtime/transactions/transaction-test");
        fs::create_dir_all(&dir).unwrap();
        write_json_atomic(
            &dir.join("manifest.json"),
            &TransactionManifest {
                schema_version: 1,
                transaction_id: "transaction-test".to_string(),
                operation: "accept".to_string(),
                draft_id: "draft-test".to_string(),
                state: "pending".to_string(),
                created_at: now_string(),
                updated_at: now_string(),
                terminal_path: PathBuf::from(".ngit/changes/accepted/change-test.json"),
                lineage_path: PathBuf::from(".ngit/lineage/lineage-test.json"),
                draft_path: PathBuf::from(".ngit/changes/drafts/draft-test.json"),
            },
        )
        .unwrap();
        let report = doctor(repo.path()).unwrap();
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "transaction_incomplete"));
    }
}

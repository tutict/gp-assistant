//! Research-only delivery journal. Call mutations under the existing exclusive app DB guard.
//! Snapshots are immutable recovery evidence; no cleanup is attempted on the error path.
use super::*;
use crate::durability;

const MANIFEST: &str = "research-delivery.json";
const GENERATION: &str = "delivery_generation";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Preparing,
    Prepared,
    Activated,
    Committed,
    Aborted,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    generation: u64,
    previous_generation: u64,
    phase: Phase,
    old_sha256: Option<String>,
    staged_sha256: Option<String>,
    // This stays at the previously committed rollback target until commit.
    rollback_file: Option<String>,
    rollback_sha256: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FaultPhase {
    Reserved,
    OldSnapshot,
    Staged,
    Prepared,
    Installed,
    Activated,
    Committed,
}

fn artifact(root: &Path, generation: u64, suffix: &str) -> PathBuf {
    root.join(format!("research.sqlite.g{generation}.{suffix}"))
}

pub(super) fn validate_header(path: &Path) -> Result<(), String> {
    let mut file =
        fs::File::open(path).map_err(|e| format!("cannot inspect research database: {e}"))?;
    let mut header = [0; 16];
    if file.metadata().map_err(|e| e.to_string())?.len() < 100
        || file.read_exact(&mut header).is_err()
        || &header != b"SQLite format 3\0"
    {
        return Err(format!(
            "invalid research database {}; original preserved",
            path.display()
        ));
    }
    Ok(())
}

pub(super) fn validate_database(path: &Path) -> Result<(), String> {
    validate_header(path)?; // SQLite considers a zero-byte file a valid empty DB; we must not.
    durability::validate_sqlite(path)?;
    let connection = read_only(path)?;
    let count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN
         ('research_metadata','documents','chunks','research_threads','research_answers','research_messages','research_answer_citations')",
        [], |row| row.get(0)
    ).map_err(|e| e.to_string())?;
    if count != 7 {
        return Err("unrecognized research schema; original preserved".into());
    }
    let foreign_key_error = connection
        .prepare("PRAGMA foreign_key_check")
        .map_err(|e| e.to_string())?
        .exists([])
        .map_err(|e| e.to_string())?;
    if foreign_key_error {
        return Err("research foreign key validation failed; original preserved".into());
    }
    Ok(())
}

fn read_only(path: &Path) -> Result<Connection, String> {
    Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("cannot read research database: {e}"))
}

fn generation(path: &Path) -> Result<u64, String> {
    let value: Option<String> = read_only(path)?
        .query_row(
            "SELECT value FROM research_metadata WHERE key=?1",
            [GENERATION],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    value
        .map(|value| {
            value
                .parse()
                .map_err(|_| "invalid research delivery generation".into())
        })
        .unwrap_or(Ok(0))
}

fn safe_rollback_name(name: &str) -> bool {
    name == "research.sqlite.rollback"
        || name
            .strip_prefix("research.sqlite.g")
            .and_then(|s| s.strip_suffix(".previous"))
            .and_then(|s| s.parse::<u64>().ok())
            .is_some()
}

fn read_manifest(root: &Path) -> Result<Option<Manifest>, String> {
    let path = root.join(MANIFEST);
    if !path.exists() {
        return Ok(None);
    }
    let manifest: Manifest = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| format!("invalid research delivery manifest; originals preserved: {e}"))?;
    if manifest.version != 1
        || manifest.generation <= manifest.previous_generation
        || manifest
            .rollback_file
            .as_deref()
            .is_some_and(|name| !safe_rollback_name(name))
    {
        return Err("invalid research delivery manifest identity; originals preserved".into());
    }
    let valid_hash =
        |hash: &str| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit());
    if [
        &manifest.old_sha256,
        &manifest.staged_sha256,
        &manifest.rollback_sha256,
    ]
    .iter()
    .any(|hash| hash.as_deref().is_some_and(|hash| !valid_hash(hash)))
        || manifest.old_sha256.is_some() != manifest.staged_sha256.is_some()
        || (matches!(
            manifest.phase,
            Phase::Prepared | Phase::Activated | Phase::Committed
        ) && manifest.old_sha256.is_none())
        || !matches!(
            (
                manifest.rollback_file.as_deref(),
                manifest.rollback_sha256.as_deref()
            ),
            (None, None) | (Some("research.sqlite.rollback"), None) | (Some(_), Some(_))
        )
        || (manifest.phase == Phase::Committed
            && (manifest.rollback_file.as_deref()
                != Some(format!("research.sqlite.g{}.previous", manifest.generation).as_str())
                || manifest.rollback_sha256 != manifest.old_sha256))
    {
        return Err(
            "invalid research delivery manifest snapshot identity; originals preserved".into(),
        );
    }
    Ok(Some(manifest))
}

fn write_manifest(root: &Path, manifest: &Manifest) -> Result<(), String> {
    durability::atomic_write(
        &root.join(MANIFEST),
        &serde_json::to_vec_pretty(manifest).map_err(|e| e.to_string())?,
    )
}

fn has_artifacts(root: &Path) -> Result<bool, String> {
    if !root.exists() {
        return Ok(false);
    }
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let name = entry.map_err(|e| e.to_string())?.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("research.sqlite") || name == MANIFEST {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn needs_recovery(root: &Path) -> Result<bool, String> {
    let main = root.join("research.sqlite");
    if !main.exists() {
        return has_artifacts(root);
    }
    if read_manifest(root)?.is_some_and(|m| {
        matches!(
            m.phase,
            Phase::Preparing | Phase::Prepared | Phase::Activated
        )
    }) {
        return Ok(true);
    }
    // Validate once at process startup, not a full integrity scan on each native query.
    Ok(!INITIALIZED_DATABASES
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .map_err(|_| "research initialization lock is poisoned")?
        .contains(&main))
}

fn forget_main(path: &Path) {
    if let Ok(mut initialized) = INITIALIZED_DATABASES
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
    {
        initialized.remove(path);
    }
    invalidate_vector_cache(path);
}

fn digest(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn validate_snapshot(path: &Path, expected: &Option<String>) -> Result<(), String> {
    validate_database(path)?;
    if expected.as_ref() != Some(&digest(path)?) {
        return Err(format!(
            "research recovery snapshot checksum mismatch: {}; originals preserved",
            path.display()
        ));
    }
    Ok(())
}

/// Only remove sidecars after checking all checkpoint result columns AND closing the handle.
fn quiesce(path: &Path) -> Result<(), String> {
    let connection = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|e| e.to_string())?;
    configure_connection(&connection)?;
    durability::checkpoint(&connection)?;
    connection
        .close()
        .map_err(|(_, e)| format!("failed to close checkpointed database: {e}"))?;
    fs::OpenOptions::new()
        .write(true)
        .open(path)
        .and_then(|file| file.sync_all())
        .map_err(|e| format!("failed to sync research database: {e}"))?;
    remove_sqlite_sidecars(path)
}

pub(super) fn recover(root: &Path) -> Result<(), String> {
    let main = root.join("research.sqlite");
    // Never overwrite a corrupt main with any backup, or initialize it as an empty store.
    if main.exists() {
        validate_database(&main)?;
    }
    let Some(mut manifest) = read_manifest(root)? else {
        if !main.exists() && has_artifacts(root)? {
            return Err("research main is missing without a delivery manifest; originals preserved for manual recovery".into());
        }
        return Ok(());
    };
    let current = if main.exists() {
        Some(generation(&main)?)
    } else {
        None
    };
    match manifest.phase {
        Phase::Committed => {
            if current != Some(manifest.generation) {
                return Err("committed research main is missing or has the wrong generation; originals preserved".into());
            }
            // The committed main may contain newer user writes: NEVER replace it with a snapshot.
            return Ok(());
        }
        Phase::Aborted => {
            if current != Some(manifest.previous_generation) {
                return Err(
                    "recovered research main has the wrong generation; originals preserved".into(),
                );
            }
            return Ok(());
        }
        _ => {}
    }
    if current != Some(manifest.previous_generation) {
        if current.is_some() && current != Some(manifest.generation) {
            return Err("unexpected research generation; originals preserved".into());
        }
        let old = artifact(root, manifest.generation, "previous");
        validate_snapshot(&old, &manifest.old_sha256)?;
        if generation(&old)? != manifest.previous_generation {
            return Err("invalid old research generation".into());
        }
        if main.exists() {
            // Preserve even the uncommitted main before reverting it; never delete original evidence.
            let rejected = artifact(root, manifest.generation, "uncommitted");
            if !rejected.exists() {
                durability::snapshot_sqlite(&read_only(&main)?, &rejected)?;
            }
            quiesce(&main)?;
        } else {
            // Orphan live sidecars cannot safely be associated with the old snapshot.
            for suffix in ["-wal", "-shm", "-journal"] {
                if PathBuf::from(format!("{}{suffix}", main.display())).exists() {
                    return Err(
                        "missing research main has orphan sidecars; originals preserved".into(),
                    );
                }
            }
        }
        durability::atomic_copy(&old, &main)?;
        forget_main(&main);
        validate_database(&main)?;
    }
    manifest.phase = Phase::Aborted;
    write_manifest(root, &manifest)
}

pub(super) fn rollback_source(root: &Path) -> Result<PathBuf, String> {
    let manifest = read_manifest(root)?;
    let name = match &manifest {
        Some(manifest) => manifest.rollback_file.clone(),
        None => Some("research.sqlite.rollback".to_string()),
    }
    .ok_or("no research database rollback is available")?;
    let path = root.join(name);
    if !path.exists() {
        return Err("no research database rollback is available".into());
    }
    if let Some(expected) = manifest.and_then(|m| m.rollback_sha256) {
        validate_snapshot(&path, &Some(expected))?;
    }
    Ok(path)
}

pub(super) fn deliver(
    root: &Path,
    build: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(), String> {
    deliver_with_hook(root, build, |_| Ok(()))
}

fn deliver_with_hook(
    root: &Path,
    build: impl FnOnce(&Path) -> Result<(), String>,
    mut hook: impl FnMut(FaultPhase) -> Result<(), String>,
) -> Result<(), String> {
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    recover(root)?;
    let main = root.join("research.sqlite");
    // A truly new installation gets a valid baseline; existing/orphan/corrupt files never do.
    ResearchStore::open(&main)?;
    let previous_generation = generation(&main)?;
    let prior = read_manifest(root)?;
    let mut high_water = prior.as_ref().map_or(previous_generation, |m| {
        m.generation.max(previous_generation)
    });
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let name = entry.map_err(|e| e.to_string())?.file_name();
        if let Some(g) = name
            .to_string_lossy()
            .strip_prefix("research.sqlite.g")
            .and_then(|s| s.split('.').next())
            .and_then(|s| s.parse::<u64>().ok())
        {
            high_water = high_water.max(g);
        }
    }
    let next = high_water
        .checked_add(1)
        .ok_or("research delivery generation exhausted")?;
    let mut manifest = Manifest {
        version: 1,
        generation: next,
        previous_generation,
        phase: Phase::Preparing,
        old_sha256: None,
        staged_sha256: None,
        rollback_sha256: prior.as_ref().and_then(|m| m.rollback_sha256.clone()),
        rollback_file: prior.and_then(|m| m.rollback_file).or_else(|| {
            root.join("research.sqlite.rollback")
                .exists()
                .then(|| "research.sqlite.rollback".into())
        }),
    };
    write_manifest(root, &manifest)?;
    hook(FaultPhase::Reserved)?;
    quiesce(&main)?; // A busy checkpoint returns BEFORE any live sidecar deletion/file switch.
    let old = artifact(root, next, "previous");
    durability::snapshot_sqlite(&read_only(&main)?, &old)?;
    hook(FaultPhase::OldSnapshot)?;
    let staged = artifact(root, next, "staged");
    build(&staged)?;
    let stage = ResearchStore::open(&staged)?;
    stage
        .connection()?
        .execute(
            "INSERT OR REPLACE INTO research_metadata(key,value) VALUES(?1,?2)",
            params![GENERATION, next.to_string()],
        )
        .map_err(|e| e.to_string())?;
    quiesce(&staged)?;
    validate_database(&staged)?;
    hook(FaultPhase::Staged)?;
    manifest.old_sha256 = Some(digest(&old)?);
    manifest.staged_sha256 = Some(digest(&staged)?);
    manifest.phase = Phase::Prepared;
    write_manifest(root, &manifest)?;
    hook(FaultPhase::Prepared)?;
    validate_snapshot(&old, &manifest.old_sha256)?;
    validate_snapshot(&staged, &manifest.staged_sha256)?;
    quiesce(&main)?;
    // Atomic replacement has no missing-main rename gap. Both sealed snapshots remain intact.
    forget_main(&main);
    durability::atomic_copy(&staged, &main)?;
    hook(FaultPhase::Installed)?;
    manifest.phase = Phase::Activated;
    write_manifest(root, &manifest)?;
    hook(FaultPhase::Activated)?;
    validate_database(&main)?;
    if generation(&main)? != next {
        return Err("activated research generation mismatch".into());
    }
    manifest.phase = Phase::Committed;
    manifest.rollback_file = Some(format!("research.sqlite.g{next}.previous"));
    manifest.rollback_sha256 = manifest.old_sha256.clone();
    write_manifest(root, &manifest)?;
    hook(FaultPhase::Committed)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempRoot(PathBuf);
    impl TempRoot {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "gp-research-d3-{name}-{}-{}",
                std::process::id(),
                unique_suffix()
            ));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }
        fn main(&self) -> PathBuf {
            self.0.join("research.sqlite")
        }
    }
    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn document(content: &str) -> Value {
        json!({"document_id":"evidence", "title":"Evidence", "content":content, "source_tier":"news", "stock_codes":["600000.SH"]})
    }
    fn seed(root: &TempRoot) -> ResearchStore {
        let store = ResearchStore::open(root.main()).unwrap();
        store
            .ingest_documents(&[document("original research evidence")])
            .unwrap();
        store
    }
    fn staged_document(path: &Path) -> Result<(), String> {
        ResearchStore::open(path)?
            .ingest_documents(&[document("replacement research evidence")])
            .map(|_| ())
    }
    fn content(root: &TempRoot) -> String {
        read_only(&root.main())
            .unwrap()
            .query_row(
                "SELECT content FROM documents WHERE id='evidence'",
                [],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn fault_phases_old_valid_or_committed_current_wins_and_recovery_is_idempotent() {
        for phase in [
            FaultPhase::Reserved,
            FaultPhase::OldSnapshot,
            FaultPhase::Staged,
            FaultPhase::Prepared,
            FaultPhase::Installed,
            FaultPhase::Activated,
            FaultPhase::Committed,
        ] {
            let root = TempRoot::new("phase");
            seed(&root);
            let result = deliver_with_hook(&root.0, staged_document, |at| {
                if at == phase {
                    Err(format!("injected {phase:?}"))
                } else {
                    Ok(())
                }
            });
            assert_eq!(result, Err(format!("injected {phase:?}")));
            let attempted = read_manifest(&root.0).unwrap().unwrap().generation;
            recover(&root.0).unwrap();
            recover(&root.0).unwrap();
            let expected = if phase == FaultPhase::Committed {
                "replacement research evidence"
            } else {
                "original research evidence"
            };
            assert_eq!(content(&root), expected, "fault at {phase:?}");
            deliver(&root.0, staged_document).unwrap();
            assert!(read_manifest(&root.0).unwrap().unwrap().generation > attempted);
        }
    }

    #[test]
    fn rollback_after_new_thread_deleted_thread_and_new_answer_keeps_current_authority() {
        let root = TempRoot::new("rollback-state");
        let original = seed(&root);
        let deleted = original
            .create_thread(&json!({"title":"Delete after import"}))
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let retained = original
            .create_thread(&json!({"title":"Retained"}))
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        original
            .save_answer(&retained, "before", &json!({"answer":"before answer"}))
            .unwrap();
        import_research_documents(
            &root.0,
            &[
                document("original research evidence"),
                json!({"document_id":"new-doc", "content":"new evidence"}),
            ],
        )
        .unwrap();
        let current = ResearchStore::open(root.main()).unwrap();
        current.delete_thread(&deleted).unwrap();
        let fresh = current
            .create_thread(&json!({"title":"Created after import"}))
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        current
            .save_answer(&fresh, "new", &json!({"answer":"new thread answer"}))
            .unwrap();
        current
            .save_answer(&retained, "after", &json!({"answer":"latest answer"}))
            .unwrap();
        current
            .mark_read(&json!({"stock_code":"600000.SH"}))
            .unwrap();
        rollback_research_documents(&root.0).unwrap();
        let rolled_back = ResearchStore::open(root.main()).unwrap();
        assert!(rolled_back.thread(&deleted).is_err());
        assert_eq!(
            rolled_back.thread(&fresh).unwrap()["answers"][0]["answer"],
            "new thread answer"
        );
        let answers = rolled_back.thread(&retained).unwrap()["answers"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(answers.len(), 2);
        assert!(answers
            .iter()
            .any(|answer| answer["answer"] == "latest answer"));
        assert_eq!(rolled_back.overview(&json!({})).unwrap()["unread_count"], 0);
        assert_eq!(rolled_back.index_status().unwrap()["document_count"], 1);
        assert_eq!(content(&root), "original research evidence");
    }

    #[test]
    fn unavailable_citation_stays_historical_across_import_and_rollback() {
        let root = TempRoot::new("citation");
        let current = seed(&root);
        let thread = current
            .create_thread(&json!({"title":"Citations"}))
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let response = current
            .query(&json!({"query":"original research evidence"}))
            .unwrap();
        current
            .save_answer(&thread, "original?", &response)
            .unwrap();
        import_research_documents(&root.0, &[document("replacement research evidence")]).unwrap();
        let history = ResearchStore::open(root.main())
            .unwrap()
            .thread(&thread)
            .unwrap();
        assert_eq!(history["answers"][0]["citations"][0]["unavailable"], true);
        rollback_research_documents(&root.0).unwrap();
        let history = ResearchStore::open(root.main())
            .unwrap()
            .thread(&thread)
            .unwrap();
        assert_eq!(
            history["answers"][0]["citations"][0]["unavailable"], true,
            "do not silently rebind a previously unavailable citation"
        );
        assert_eq!(
            history["answers"][0]["citations"][0]["excerpt"],
            response["citations"][0]["excerpt"]
        );
    }

    #[test]
    fn committed_main_with_later_user_writes_wins_over_all_snapshots() {
        let root = TempRoot::new("committed");
        seed(&root);
        deliver(&root.0, staged_document).unwrap();
        let current = ResearchStore::open(root.main()).unwrap();
        let id = current
            .create_thread(&json!({"title":"After commit"}))
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        // Even a corrupt retired snapshot must not roll back a committed main.
        fs::write(rollback_source(&root.0).unwrap(), b"corrupt retired backup").unwrap();
        recover(&root.0).unwrap();
        assert!(current.thread(&id).is_ok());
        assert_eq!(content(&root), "replacement research evidence");
    }

    #[test]
    fn failed_checkpoint_keeps_live_wal_and_both_current_writes() {
        let root = TempRoot::new("busy");
        let store = seed(&root);
        let writer = store.connection().unwrap();
        writer
            .execute_batch("PRAGMA wal_autocheckpoint=0;")
            .unwrap();
        let reader = store.connection().unwrap();
        reader
            .execute_batch("BEGIN; SELECT * FROM research_threads;")
            .unwrap();
        writer
            .execute(
                "INSERT INTO research_threads VALUES('wal-thread','WAL committed',NULL,1,1)",
                [],
            )
            .unwrap();
        let wal = PathBuf::from(format!("{}-wal", root.main().display()));
        let before = fs::read(&wal).unwrap();
        let result = deliver(&root.0, staged_document);
        assert!(result.unwrap_err().contains("checkpoint"));
        assert_eq!(
            fs::read(&wal).unwrap(),
            before,
            "failed checkpoint must not unlink or truncate live WAL"
        );
        assert_eq!(content(&root), "original research evidence");
        reader.execute_batch("ROLLBACK;").unwrap();
        drop(reader);
        drop(writer);
        recover(&root.0).unwrap();
        assert!(store.thread("wal-thread").is_ok());
    }

    #[test]
    fn recovery_corrupt_main_and_corrupt_old_snapshot_fail_closed() {
        for corrupt_main in [true, false] {
            let root = TempRoot::new("corrupt");
            seed(&root);
            assert_eq!(
                deliver_with_hook(&root.0, staged_document, |phase| {
                    if phase == FaultPhase::Activated {
                        Err("crash".into())
                    } else {
                        Ok(())
                    }
                }),
                Err("crash".into())
            );
            let manifest = read_manifest(&root.0).unwrap().unwrap();
            let target = if corrupt_main {
                root.main()
            } else {
                artifact(&root.0, manifest.generation, "previous")
            };
            fs::write(&target, b"corrupt database").unwrap();
            let main_before = fs::read(root.main()).unwrap();
            assert!(recover(&root.0).is_err());
            assert_eq!(fs::read(target).unwrap(), b"corrupt database");
            assert_eq!(fs::read(root.main()).unwrap(), main_before);
            assert_eq!(
                read_manifest(&root.0).unwrap().unwrap().phase,
                Phase::Activated
            );
        }
    }

    #[test]
    fn checksum_mismatch_in_valid_old_database_is_not_accepted() {
        let root = TempRoot::new("checksum");
        seed(&root);
        assert_eq!(
            deliver_with_hook(&root.0, staged_document, |phase| {
                if phase == FaultPhase::Activated {
                    Err("crash".into())
                } else {
                    Ok(())
                }
            }),
            Err("crash".into())
        );
        let m = read_manifest(&root.0).unwrap().unwrap();
        let old = artifact(&root.0, m.generation, "previous");
        Connection::open(&old)
            .unwrap()
            .execute("UPDATE documents SET title='tampered'", [])
            .unwrap();
        validate_database(&old).unwrap();
        assert!(recover(&root.0).unwrap_err().contains("checksum mismatch"));
    }

    #[test]
    fn valid_legacy_rollback_is_rebuilt_not_reactivated() {
        let root = TempRoot::new("legacy-rollback");
        let current = seed(&root);
        let old = root.0.join("research.sqlite.rollback");
        durability::snapshot_sqlite(&current.connection().unwrap(), &old).unwrap();
        let id = current.create_thread(&json!({"title":"Recent"})).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        rollback_research_documents(&root.0).unwrap();
        assert!(ResearchStore::open(root.main())
            .unwrap()
            .thread(&id)
            .is_ok());
        assert!(old.exists(), "legacy rollback original is retained");
    }

    #[test]
    fn v1_and_v2_packs_share_the_durable_delivery_path() {
        for version in [1, 2] {
            let root = TempRoot::new("pack");
            seed(&root);
            let package = root.0.join("input.pack");
            if version == 1 {
                fs::write(
                    &package,
                    serde_json::to_vec(
                        &json!({"schema_version":1,"documents":[document("v1 imported evidence")]}),
                    )
                    .unwrap(),
                )
                .unwrap();
            } else {
                let source = ResearchStore::open(root.0.join("export-source.sqlite")).unwrap();
                source
                    .ingest_documents(&[document("v2 imported evidence")])
                    .unwrap();
                source.export_portable_pack(&package).unwrap();
            }
            import_research_documents(&root.0, &read_portable_pack(&package).unwrap()).unwrap();
            let imported: String = read_only(&root.main())
                .unwrap()
                .query_row("SELECT content FROM documents", [], |row| row.get(0))
                .unwrap();
            assert_eq!(imported, format!("v{version} imported evidence"));
            assert_eq!(
                read_manifest(&root.0).unwrap().unwrap().phase,
                Phase::Committed
            );
        }
    }
    #[test]
    fn prepared_missing_main_restores_old_but_committed_missing_main_fails_closed() {
        for committed in [false, true] {
            let root = TempRoot::new("missing-main");
            seed(&root);
            if committed {
                deliver(&root.0, staged_document).unwrap();
            } else {
                assert_eq!(
                    deliver_with_hook(&root.0, staged_document, |phase| {
                        if phase == FaultPhase::Prepared {
                            Err("crash".into())
                        } else {
                            Ok(())
                        }
                    }),
                    Err("crash".into())
                );
            }
            let preserved = root.0.join("main-preserved-for-test.sqlite");
            quiesce(&root.main()).unwrap();
            fs::rename(root.main(), &preserved).unwrap();
            if committed {
                assert!(recover(&root.0).is_err());
                assert!(!root.main().exists());
            } else {
                recover(&root.0).unwrap();
                assert_eq!(content(&root), "original research evidence");
            }
            assert!(preserved.exists());
        }
    }

    #[test]
    fn valid_but_tampered_staging_never_replaces_main() {
        let root = TempRoot::new("staged-checksum");
        seed(&root);
        let result = deliver_with_hook(&root.0, staged_document, |phase| {
            if phase == FaultPhase::Prepared {
                let manifest = read_manifest(&root.0)?.unwrap();
                let path = artifact(&root.0, manifest.generation, "staged");
                let connection = Connection::open(&path).map_err(|e| e.to_string())?;
                connection
                    .execute("UPDATE documents SET title='tampered'", [])
                    .map_err(|e| e.to_string())?;
                durability::checkpoint(&connection)?;
            }
            Ok(())
        });
        assert!(result.unwrap_err().contains("checksum mismatch"));
        recover(&root.0).unwrap();
        assert_eq!(content(&root), "original research evidence");
    }

    #[test]
    fn invalid_manifest_is_preserved_and_does_not_initialize_missing_main() {
        let root = TempRoot::new("manifest");
        let path = root.0.join(MANIFEST);
        fs::write(&path, b"{broken manifest").unwrap();
        assert!(recover(&root.0).is_err());
        assert!(deliver(&root.0, staged_document).is_err());
        assert!(!root.main().exists());
        assert_eq!(fs::read(path).unwrap(), b"{broken manifest");
    }

    #[test]
    fn rollback_refuses_valid_but_modified_document_snapshot() {
        let root = TempRoot::new("rollback-checksum");
        seed(&root);
        deliver(&root.0, staged_document).unwrap();
        let old = rollback_source(&root.0).unwrap();
        Connection::open(&old)
            .unwrap()
            .execute("UPDATE documents SET title='changed after seal'", [])
            .unwrap();
        assert!(rollback_research_documents(&root.0)
            .unwrap_err()
            .contains("checksum mismatch"));
        assert_eq!(content(&root), "replacement research evidence");
    }
    #[test]
    fn committed_manifest_requires_its_snapshot_identity_fields() {
        let root = TempRoot::new("manifest-identity");
        seed(&root);
        deliver(&root.0, staged_document).unwrap();
        let mut manifest = read_manifest(&root.0).unwrap().unwrap();
        manifest.old_sha256 = None;
        manifest.rollback_sha256 = None;
        write_manifest(&root.0, &manifest).unwrap();
        assert!(
            recover(&root.0).is_err(),
            "a partially damaged committed manifest must fail closed"
        );
        assert_eq!(content(&root), "replacement research evidence");
    }
}

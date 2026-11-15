use crate::research::schema::configure_connection;
use crate::research::schema::initialize_schema;
use crate::research::schema::RESEARCH_SCHEMA_VERSION;
use crate::research::validation;
use rusqlite::Connection;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::OnceLock;

pub(super) mod conversations;
pub(super) mod documents;
pub(super) mod evidence;
pub(super) mod packs;
pub(super) mod retrieval;
pub(super) mod vectors;

pub(crate) struct ResearchStore {
    path: PathBuf,
}

impl ResearchStore {
    pub(crate) fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("failed to create research directory: {error}"))?;
        }
        let existed = path.exists();
        if existed {
            validation::validate_header(&path)?;
        }
        let registry = INITIALIZED_DATABASES.get_or_init(|| Mutex::new(HashSet::new()));
        let mut initialized = registry
            .lock()
            .map_err(|_| "research database initialization lock is poisoned".to_string())?;
        if !existed || !initialized.contains(&path) {
            if existed {
                validation::validate_database(&path)?;
            }
            let connection = Connection::open(&path)
                .map_err(|error| format!("failed to open research database: {error}"))?;
            let version: i64 = connection
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .map_err(|e| e.to_string())?;
            crate::durability::backup_before_migration(
                &connection,
                &path,
                version,
                RESEARCH_SCHEMA_VERSION,
            )?;
            initialize_schema(&connection)?;
            initialized.insert(path.clone());
        }
        Ok(Self { path })
    }
}

impl ResearchStore {
    pub(in crate::research) fn connection(&self) -> Result<Connection, String> {
        let connection =
            Connection::open_with_flags(&self.path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)
                .map_err(|error| format!("failed to open research database: {error}"))?;
        configure_connection(&connection)?;
        Ok(connection)
    }
}

pub(in crate::research) static INITIALIZED_DATABASES: OnceLock<Mutex<HashSet<PathBuf>>> =
    OnceLock::new();

impl ResearchStore {
    pub(in crate::research) fn database_path(&self) -> &Path {
        &self.path
    }
}

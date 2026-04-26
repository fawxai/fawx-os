//! Persistent task state for Fawx OS.
//!
//! The store is intentionally small: one JSON file per task. That keeps the
//! first runtime slice inspectable while preserving a structured migration path
//! to SQLite or another durable backend later.
//!
//! Mutation is deliberately routed through [`TaskStore::create`] and
//! [`TaskStore::transition_state`]. Callers may load snapshots for inspection,
//! but stale snapshots are not accepted as whole-record writes:
//!
//! ```compile_fail
//! use fawx_harness::TaskState;
//! use fawx_task_store::TaskStore;
//!
//! let store = TaskStore::new(std::env::temp_dir().join("fawx-task-store-doc-test"));
//! store
//!     .create(TaskState::new_background_task("task-1", "original"))
//!     .unwrap();
//! let stale = store.load("task-1").unwrap();
//! store
//!     .transition_state("task-1", |mut state| {
//!         state.contract.user_intent = "newer transition".to_string();
//!         Ok::<_, fawx_task_store::TaskStoreError>(state)
//!     })
//!     .unwrap();
//!
//! store.update_state(stale.state).unwrap();
//! ```
//!
//! ```compile_fail
//! use fawx_harness::TaskState;
//! use fawx_task_store::TaskStore;
//!
//! let store = TaskStore::new(std::env::temp_dir().join("fawx-task-store-doc-test"));
//! let task = store
//!     .create(TaskState::new_background_task("task-1", "original"))
//!     .unwrap();
//!
//! store.save(&task).unwrap();
//! ```

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fawx_harness::TaskState;
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub enum TaskStoreError {
    Io(io::Error),
    Json(serde_json::Error),
    InvalidTaskId(String),
    TaskIdChanged { expected: String, actual: String },
    ClockBeforeUnixEpoch,
    TaskAlreadyExists(String),
    TaskLocked(String),
}

impl Display for TaskStoreError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::Json(error) => write!(f, "JSON error: {error}"),
            Self::InvalidTaskId(task_id) => write!(f, "invalid task id: {task_id}"),
            Self::TaskIdChanged { expected, actual } => {
                write!(f, "task transition changed id from {expected} to {actual}")
            }
            Self::ClockBeforeUnixEpoch => write!(f, "system clock is before unix epoch"),
            Self::TaskAlreadyExists(task_id) => write!(f, "task already exists: {task_id}"),
            Self::TaskLocked(task_id) => write!(f, "task is locked by another writer: {task_id}"),
        }
    }
}

impl Error for TaskStoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::InvalidTaskId(_)
            | Self::TaskIdChanged { .. }
            | Self::ClockBeforeUnixEpoch
            | Self::TaskAlreadyExists(_)
            | Self::TaskLocked(_) => None,
        }
    }
}

impl From<io::Error> for TaskStoreError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for TaskStoreError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[derive(Debug)]
pub enum TaskStoreTransitionError<E> {
    Store(TaskStoreError),
    Transition(E),
}

impl<E: Display> Display for TaskStoreTransitionError<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(error) => Display::fmt(error, f),
            Self::Transition(error) => Display::fmt(error, f),
        }
    }
}

impl<E: Error + 'static> Error for TaskStoreTransitionError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            Self::Transition(error) => Some(error),
        }
    }
}

impl<E> From<TaskStoreError> for TaskStoreTransitionError<E> {
    fn from(error: TaskStoreError) -> Self {
        Self::Store(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredTask {
    pub state: TaskState,
    pub created_at_ms: u128,
    pub updated_at_ms: u128,
}

#[derive(Debug, Clone)]
pub struct TaskStore {
    root: PathBuf,
}

impl TaskStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn create(&self, state: TaskState) -> Result<StoredTask, TaskStoreError> {
        validate_task_id(&state.task_id)?;
        fs::create_dir_all(&self.root).map_err(TaskStoreError::from)?;

        let _process_lock = process_lock()?;
        let _lock = TaskFileLock::acquire(self.lock_path(&state.task_id)?)?;
        let path = self.task_path(&state.task_id)?;
        if path.exists() {
            return Err(TaskStoreError::TaskAlreadyExists(state.task_id));
        }

        let now = now_ms()?;
        let task = StoredTask {
            state,
            created_at_ms: now,
            updated_at_ms: now,
        };
        self.write_task(&task)?;
        Ok(task)
    }

    pub fn transition_state<E>(
        &self,
        task_id: &str,
        transition: impl FnOnce(TaskState) -> Result<TaskState, E>,
    ) -> Result<StoredTask, TaskStoreTransitionError<E>> {
        validate_task_id(task_id)?;
        fs::create_dir_all(&self.root).map_err(TaskStoreError::from)?;

        let _process_lock = process_lock()?;
        let _lock = TaskFileLock::acquire(self.lock_path(task_id)?)?;
        let path = self.task_path(task_id)?;
        let existing = self.read_task(&path)?;
        let state = transition(existing.state).map_err(TaskStoreTransitionError::Transition)?;
        if state.task_id != task_id {
            return Err(TaskStoreTransitionError::Store(
                TaskStoreError::TaskIdChanged {
                    expected: task_id.to_string(),
                    actual: state.task_id,
                },
            ));
        }

        let task = StoredTask {
            state,
            created_at_ms: existing.created_at_ms,
            updated_at_ms: now_ms()?,
        };
        self.write_task(&task)?;
        Ok(task)
    }

    fn write_task(&self, task: &StoredTask) -> Result<(), TaskStoreError> {
        let path = self.task_path(&task.state.task_id)?;
        let temporary_path = self.temporary_task_path(&task.state.task_id)?;
        let payload = serde_json::to_vec_pretty(task)?;

        write_synced_file(&temporary_path, &payload)?;
        if let Err(error) = fs::rename(&temporary_path, &path) {
            let _ = fs::remove_file(&temporary_path);
            return Err(TaskStoreError::Io(error));
        }
        sync_directory(&self.root)?;
        Ok(())
    }

    pub fn load(&self, task_id: &str) -> Result<StoredTask, TaskStoreError> {
        let path = self.task_path(task_id)?;
        self.read_task(&path)
    }

    pub fn list(&self) -> Result<Vec<StoredTask>, TaskStoreError> {
        if !self.root.exists() {
            return Ok(vec![]);
        }

        let mut tasks: Vec<StoredTask> = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                continue;
            }
            let payload = fs::read(path)?;
            tasks.push(serde_json::from_slice(&payload)?);
        }
        tasks.sort_by(|left, right| left.state.task_id.cmp(&right.state.task_id));
        Ok(tasks)
    }

    fn task_path(&self, task_id: &str) -> Result<PathBuf, TaskStoreError> {
        validate_task_id(task_id)?;
        Ok(self.root.join(format!("{task_id}.json")))
    }

    fn lock_path(&self, task_id: &str) -> Result<PathBuf, TaskStoreError> {
        validate_task_id(task_id)?;
        Ok(self.root.join(format!("{task_id}.lock")))
    }

    fn temporary_task_path(&self, task_id: &str) -> Result<PathBuf, TaskStoreError> {
        validate_task_id(task_id)?;
        Ok(self.root.join(format!(
            "{task_id}.{}.{}.{}.json.tmp",
            std::process::id(),
            now_ms()?,
            TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed)
        )))
    }

    fn read_task(&self, path: &Path) -> Result<StoredTask, TaskStoreError> {
        let payload = fs::read(path)?;
        Ok(serde_json::from_slice(&payload)?)
    }
}

static TEMP_FILE_COUNTER: AtomicUsize = AtomicUsize::new(0);
static TASK_STORE_PROCESS_LOCK: Mutex<()> = Mutex::new(());

fn process_lock() -> Result<std::sync::MutexGuard<'static, ()>, TaskStoreError> {
    TASK_STORE_PROCESS_LOCK
        .lock()
        .map_err(|_| TaskStoreError::Io(io::Error::other("task store process lock poisoned")))
}

#[derive(Debug)]
struct TaskFileLock {
    path: PathBuf,
    owner: String,
}

impl TaskFileLock {
    fn acquire(path: PathBuf) -> Result<Self, TaskStoreError> {
        let deadline = now_ms()?.saturating_add(5_000);
        let task_id = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".lock"))
            .unwrap_or("unknown")
            .to_string();

        loop {
            match fs::create_dir(&path) {
                Ok(()) => {
                    let owner = lock_owner_token();
                    write_synced_file(&path.join("owner.pid"), owner.as_bytes())?;
                    sync_directory(path.parent().unwrap_or_else(|| Path::new(".")))?;
                    return Ok(Self { path, owner });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    if lock_is_stale(&path) {
                        remove_stale_lock(&path)?;
                        continue;
                    }

                    if now_ms()? >= deadline {
                        return Err(TaskStoreError::TaskLocked(task_id));
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(TaskStoreError::Io(error)),
            }
        }
    }
}

impl Drop for TaskFileLock {
    fn drop(&mut self) {
        if lock_owner_matches(&self.path, &self.owner) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn lock_owner_token() -> String {
    format!(
        "{}:{}",
        std::process::id(),
        TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

fn lock_is_stale(path: &Path) -> bool {
    match fs::read_to_string(path.join("owner.pid")) {
        Ok(owner) => owner
            .trim()
            .split_once(':')
            .map(|(pid, _)| pid)
            .unwrap_or(owner.trim())
            .parse::<u32>()
            .map(|pid| !process_is_alive(pid))
            .unwrap_or(true),
        Err(_) => lock_without_owner_is_stale(path),
    }
}

fn lock_owner_matches(path: &Path, owner: &str) -> bool {
    fs::read_to_string(path.join("owner.pid"))
        .map(|stored_owner| stored_owner.trim() == owner)
        .unwrap_or(false)
}

fn process_is_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn lock_without_owner_is_stale(path: &Path) -> bool {
    const OWNER_WRITE_GRACE: Duration = Duration::from_secs(30);

    let Ok(metadata) = fs::metadata(path) else {
        return true;
    };
    if !metadata.is_dir() {
        return true;
    }
    let Ok(modified_at) = metadata.modified() else {
        return false;
    };
    modified_at
        .elapsed()
        .map(|age| age > OWNER_WRITE_GRACE)
        .unwrap_or(false)
}

fn remove_stale_lock(path: &Path) -> Result<(), TaskStoreError> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotADirectory => {
            fs::remove_file(path).map_err(TaskStoreError::Io)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::DirectoryNotEmpty => Ok(()),
        Err(error) => Err(TaskStoreError::Io(error)),
    }
}

fn write_synced_file(path: &Path, payload: &[u8]) -> Result<(), TaskStoreError> {
    let mut file = File::create(path)?;
    file.write_all(payload)?;
    file.sync_all()?;
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), TaskStoreError> {
    File::open(path)?.sync_all()?;
    Ok(())
}

fn validate_task_id(task_id: &str) -> Result<(), TaskStoreError> {
    let valid = !task_id.is_empty()
        && task_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));

    if valid {
        Ok(())
    } else {
        Err(TaskStoreError::InvalidTaskId(task_id.to_string()))
    }
}

fn now_ms() -> Result<u128, TaskStoreError> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| TaskStoreError::ClockBeforeUnixEpoch)?
        .as_millis())
}

pub fn default_task_store_path() -> PathBuf {
    std::env::var_os("FAWX_OS_TASK_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(".fawx-os").join("tasks"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fawx_harness::TaskPhase;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    static TEST_STORE_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn temporary_store() -> TaskStore {
        let counter = TEST_STORE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let unique = format!(
            "fawx-os-task-store-test-{}-{}-{}",
            std::process::id(),
            now_ms().expect("test clock should be after unix epoch"),
            counter
        );
        TaskStore::new(std::env::temp_dir().join(unique))
    }

    #[test]
    fn persists_and_loads_task_state() {
        let store = temporary_store();
        let state = TaskState::new_background_task("task-1", "cancel subscription");

        store.create(state.clone()).expect("create task");
        let loaded = store.load("task-1").expect("load task");

        assert_eq!(loaded.state, state);
    }

    #[test]
    fn rejects_path_like_task_ids() {
        let store = temporary_store();
        let state = TaskState::new_background_task("../task", "escape store");

        let error = store.create(state).expect_err("invalid id should fail");

        assert!(matches!(error, TaskStoreError::InvalidTaskId(_)));
    }

    #[test]
    fn lists_tasks_in_stable_order() {
        let store = temporary_store();
        store
            .create(TaskState::new_background_task("task-b", "second"))
            .expect("create task b");
        store
            .create(TaskState::new_background_task("task-a", "first"))
            .expect("create task a");

        let ids: Vec<_> = store
            .list()
            .expect("list tasks")
            .into_iter()
            .map(|task| task.state.task_id)
            .collect();

        assert_eq!(ids, vec!["task-a", "task-b"]);
    }

    #[test]
    fn stale_lock_file_does_not_block_writes() {
        let store = temporary_store();
        fs::create_dir_all(&store.root).expect("create store root");
        File::create(store.lock_path("task-1").expect("lock path")).expect("create lock");

        store
            .create(TaskState::new_background_task("task-1", "locked write"))
            .expect("stale lock file should not block OS lock acquisition");

        assert!(store.load("task-1").is_ok());
    }

    #[test]
    fn create_fails_when_task_already_exists() {
        let store = temporary_store();
        store
            .create(TaskState::new_background_task("task-1", "first"))
            .expect("create first task");

        let error = store
            .create(TaskState::new_background_task("task-1", "second"))
            .expect_err("duplicate create should fail");
        let loaded = store.load("task-1").expect("load task");

        assert!(matches!(error, TaskStoreError::TaskAlreadyExists(_)));
        assert_eq!(loaded.state.contract.user_intent, "first");
    }

    #[test]
    fn transition_state_serializes_concurrent_updates() {
        let store = Arc::new(temporary_store());
        store
            .create(TaskState::new_background_task("task-1", "0"))
            .expect("create task");

        let handles: Vec<_> = (0..8)
            .map(|_| {
                let store = Arc::clone(&store);
                thread::spawn(move || {
                    for _ in 0..10 {
                        store
                            .transition_state("task-1", |mut state| {
                                let current: usize = state
                                    .contract
                                    .user_intent
                                    .parse()
                                    .expect("counter should be numeric");
                                state.contract.user_intent = (current + 1).to_string();
                                Ok::<_, TaskStoreError>(state)
                            })
                            .expect("transition state");
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().expect("worker should finish");
        }

        let loaded = store.load("task-1").expect("load task");
        assert_eq!(loaded.state.contract.user_intent, "80");
    }

    #[test]
    fn transition_state_reads_latest_state_after_stale_snapshot_load() {
        let store = temporary_store();
        store
            .create(TaskState::new_background_task("task-1", "original"))
            .expect("create task");
        let stale = store.load("task-1").expect("load stale snapshot");

        store
            .transition_state("task-1", |mut state| {
                state.contract.user_intent = "newer transition".to_string();
                Ok::<_, TaskStoreError>(state)
            })
            .expect("write newer transition");
        store
            .transition_state("task-1", |mut state| {
                assert_eq!(stale.state.contract.user_intent, "original");
                assert_eq!(state.contract.user_intent, "newer transition");
                state.phase = TaskPhase::Checkpointed;
                Ok::<_, TaskStoreError>(state)
            })
            .expect("stale snapshot should not be the transition input");

        let loaded = store.load("task-1").expect("load task");
        assert_eq!(loaded.state.contract.user_intent, "newer transition");
        assert_eq!(loaded.state.phase, TaskPhase::Checkpointed);
    }

    #[test]
    fn transition_state_does_not_leave_temporary_files_after_success() {
        let store = temporary_store();

        store
            .create(TaskState::new_background_task("task-1", "clean temp files"))
            .expect("create task");
        store
            .transition_state("task-1", |mut state| {
                state.contract.user_intent = "updated cleanly".to_string();
                Ok::<_, TaskStoreError>(state)
            })
            .expect("transition task");

        let temporary_files: Vec<_> = fs::read_dir(&store.root)
            .expect("read store")
            .map(|entry| entry.expect("entry").path())
            .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("tmp"))
            .collect();

        assert!(temporary_files.is_empty());
    }
}

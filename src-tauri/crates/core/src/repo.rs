//! Repository: layout, cross-process lock, atomic writes, backups, revisions (§4.2.3, §4.3).

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use fs2::FileExt;
use serde_json::{Map, Value};

use crate::error::{CoreError, CoreResult};
use crate::model::{
    DataFile, DomainMeta, IdempotencyRecord, ScheduleFile, SettingsFile, DATA_SCHEMA_VERSION,
    SCHEDULE_SCHEMA_VERSION, SETTINGS_SCHEMA_VERSION,
};
use crate::time::now_iso;

pub const LOCK_FILE: &str = ".kxtodo.lock";
pub const DATA_FILE: &str = "data.json";
pub const SETTINGS_FILE: &str = "settings.json";
pub const SCHEDULE_FILE: &str = "tasks.json";
pub const HISTORY_DIR: &str = "history";
pub const SCHEDULE_HISTORY: &str = "schedule.ndjson";
pub const AUDIT_HISTORY: &str = "audit.ndjson";
pub const BACKUP_DIR: &str = "backups";
pub const RUNTIME_DIR: &str = "runtime";
pub const HOST_DESCRIPTOR: &str = "host.json";
pub const HOST_LAUNCH_LOCK: &str = "host.launch.lock";
pub const HOST_OWNER_LOCK: &str = "host.owner.lock";
pub const RECOVERY_FILE: &str = "recovery.json";
pub const IMG_DIR: &str = "img";
/// 数据目录名（桌面端默认布局：平台数据根/kxtodo/todo-note-data）。
pub const DATA_DIR_NAME: &str = "todo-note-data";

/// 平台标准数据根：Windows → %LOCALAPPDATA%（回退 %USERPROFILE%\AppData\Local）；
/// Unix → $XDG_DATA_HOME（须为绝对路径）或 ~/.local/share。
fn platform_data_root() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if let Some(dir) = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .filter(|dir| !dir.as_os_str().is_empty())
        {
            return Some(dir);
        }
        std::env::var_os("USERPROFILE")
            .map(|home| PathBuf::from(home).join("AppData").join("Local"))
    }
    #[cfg(not(windows))]
    {
        if let Some(dir) = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute())
        {
            return Some(dir);
        }
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("share"))
    }
}

/// 桌面端默认数据目录（GUI 与 CLI 共用）：平台数据根/kxtodo/todo-note-data。
/// 环境变量缺失时回退相对目录名，交给调用方解析兜底。
pub fn default_data_dir() -> PathBuf {
    match platform_data_root() {
        Some(root) => root.join("kxtodo").join(DATA_DIR_NAME),
        None => PathBuf::from(DATA_DIR_NAME),
    }
}

/// kxtodo-server 默认数据目录：平台数据根/kxtodo/server。
pub fn default_server_dir() -> PathBuf {
    match platform_data_root() {
        Some(root) => root.join("kxtodo").join("server"),
        None => PathBuf::from("server"),
    }
}

/// 内置同步服务器（GUI/APK 的「本机作为服务器」）的数据目录：todo 数据目录下的 `server/`。
///
/// 刻意**不**用 [`default_server_dir`]：那样同机跑两个实例（以及用自定义数据目录的
/// e2e 测试）会共用一份服务器配置与数据库——测试就会覆盖开发机真实的 admin 凭据。
/// 放在数据目录里面，实例之间天然隔离，并且跟着 `--data-dir` 一起走。
pub fn embedded_server_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("server")
}

pub const IDEMPOTENCY_MAX_RECORDS: usize = 1000;
pub const IDEMPOTENCY_MAX_AGE_DAYS: i64 = 30;
pub const BACKUP_KEEP: usize = 5;
pub const SCHEDULE_HISTORY_MAX_BYTES: u64 = 20 * 1024 * 1024;
pub const SCHEDULE_HISTORY_PER_TASK: usize = 100;
pub const SCHEDULE_OUTPUT_MAX_BYTES: usize = 64 * 1024;
pub const AUDIT_MAX_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    Data,
    Settings,
    Schedule,
}

impl Domain {
    pub fn as_str(self) -> &'static str {
        match self {
            Domain::Data => "data",
            Domain::Settings => "settings",
            Domain::Schedule => "schedule",
        }
    }

    pub fn file_name(self) -> &'static str {
        match self {
            Domain::Data => DATA_FILE,
            Domain::Settings => SETTINGS_FILE,
            Domain::Schedule => SCHEDULE_FILE,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Layout {
    pub root: PathBuf,
}

impl Layout {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn data_file(&self) -> PathBuf {
        self.root.join(DATA_FILE)
    }
    pub fn settings_file(&self) -> PathBuf {
        self.root.join(SETTINGS_FILE)
    }
    pub fn schedule_file(&self) -> PathBuf {
        self.root.join(SCHEDULE_FILE)
    }
    pub fn lock_file(&self) -> PathBuf {
        self.root.join(LOCK_FILE)
    }
    pub fn history_dir(&self) -> PathBuf {
        self.root.join(HISTORY_DIR)
    }
    pub fn schedule_history(&self) -> PathBuf {
        self.history_dir().join(SCHEDULE_HISTORY)
    }
    pub fn audit_history(&self) -> PathBuf {
        self.history_dir().join(AUDIT_HISTORY)
    }
    pub fn backup_dir(&self) -> PathBuf {
        self.root.join(BACKUP_DIR)
    }
    pub fn runtime_dir(&self) -> PathBuf {
        self.root.join(RUNTIME_DIR)
    }
    pub fn host_descriptor(&self) -> PathBuf {
        self.runtime_dir().join(HOST_DESCRIPTOR)
    }
    pub fn host_launch_lock(&self) -> PathBuf {
        self.runtime_dir().join(HOST_LAUNCH_LOCK)
    }
    pub fn host_owner_lock(&self) -> PathBuf {
        self.runtime_dir().join(HOST_OWNER_LOCK)
    }
    pub fn recovery_file(&self) -> PathBuf {
        self.runtime_dir().join(RECOVERY_FILE)
    }
    pub fn img_dir(&self) -> PathBuf {
        self.root.join(IMG_DIR)
    }
    pub fn entry_img_dir(&self, entry_id: &str) -> PathBuf {
        self.img_dir().join("data").join(entry_id)
    }
    /// markdown 插图根目录（每个条目一个子目录）。
    pub fn entry_img_root(&self) -> PathBuf {
        self.img_dir().join("data")
    }
    /// 头像目录（"avator" 是历史拼写，GUI 侧同名，不要改）。
    pub fn avatar_img_dir(&self) -> PathBuf {
        self.img_dir().join("avator")
    }
    pub fn background_img_dir(&self) -> PathBuf {
        self.img_dir().join("background")
    }

    pub fn ensure(&self) -> CoreResult<()> {
        fs::create_dir_all(&self.root)?;
        fs::create_dir_all(self.img_dir().join("avator"))?;
        fs::create_dir_all(self.img_dir().join("background"))?;
        fs::create_dir_all(self.img_dir().join("data"))?;
        Ok(())
    }
}

/// Cross-process exclusive transaction lock (§4.3.1).
pub struct RepoLock {
    file: File,
}

impl RepoLock {
    pub fn acquire(layout: &Layout) -> CoreResult<Self> {
        layout.ensure()?;
        let path = layout.lock_file();
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|error| {
                CoreError::io(format!("无法打开仓库锁 {}：{error}", path.display()))
            })?;
        file.lock_exclusive().map_err(|error| {
            CoreError::io(format!(
                "无法获取仓库锁 {}：{error}（可能有其他 KXToDo 进程卡住）",
                path.display()
            ))
        })?;
        Ok(Self { file })
    }

    /// Read-only diagnostic probe for an already-existing lock file. It never
    /// creates repository directories or files.
    pub fn try_acquire_existing(layout: &Layout) -> CoreResult<Option<Self>> {
        let path = layout.lock_file();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|error| {
                CoreError::io(format!("无法打开仓库锁 {}：{error}", path.display()))
            })?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(Self { file })),
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.raw_os_error() == Some(33) =>
            {
                Ok(None)
            }
            Err(error) => Err(CoreError::io(format!(
                "仓库锁不可用 {}：{error}",
                path.display()
            ))),
        }
    }

    /// Non-blocking acquire: Ok(None) when another process holds the lock.
    pub fn try_acquire(layout: &Layout) -> CoreResult<Option<Self>> {
        layout.ensure()?;
        let path = layout.lock_file();
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|error| {
                CoreError::io(format!("无法打开仓库锁 {}：{error}", path.display()))
            })?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(Self { file })),
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.raw_os_error() == Some(33) =>
            {
                // WouldBlock on Unix; ERROR_LOCK_VIOLATION (33) on Windows.
                Ok(None)
            }
            Err(error) => Err(CoreError::io(format!(
                "仓库锁不可用 {}：{error}",
                path.display()
            ))),
        }
    }
}

impl Drop for RepoLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

/// Lifetime ownership guard for one Background Host per normalized data dir.
pub struct HostOwnerLock {
    file: File,
}

impl HostOwnerLock {
    pub fn acquire(layout: &Layout) -> CoreResult<Self> {
        layout.ensure()?;
        fs::create_dir_all(layout.runtime_dir())?;
        let path = layout.host_owner_lock();
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&path)?;
        file.try_lock_exclusive().map_err(|error| {
            CoreError::conflict(
                "HOST_ALREADY_RUNNING",
                format!(
                    "数据目录已有 Background Host：{}（{error}）",
                    layout.root.display()
                ),
            )
        })?;
        Ok(Self { file })
    }
}

impl Drop for HostOwnerLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

/// Atomic write: temp file + flush + rename with replace semantics.
pub fn atomic_write(path: &Path, contents: &str) -> CoreResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| CoreError::internal(format!("路径无父目录：{}", path.display())))?;
    fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("data");
    let tmp = parent.join(format!(".{file_name}.{}.tmp", std::process::id()));
    {
        let mut file = File::create(&tmp)
            .map_err(|error| CoreError::io(format!("无法写临时文件 {}：{error}", tmp.display())))?;
        use std::io::Write;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
    }
    replace_file(&tmp, path).map_err(|error| {
        let _ = fs::remove_file(&tmp);
        error
    })?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn replace_file(tmp: &Path, target: &Path) -> CoreResult<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_REPLACE_EXISTING};
    let from: Vec<u16> = tmp.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    let ok = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_REPLACE_EXISTING) };
    if ok == 0 {
        return Err(CoreError::io(format!(
            "原子替换失败 {} -> {}",
            tmp.display(),
            target.display()
        )));
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn replace_file(tmp: &Path, target: &Path) -> CoreResult<()> {
    fs::rename(tmp, target).map_err(|error| {
        CoreError::io(format!(
            "原子替换失败 {} -> {}：{error}",
            tmp.display(),
            target.display()
        ))
    })
}

pub fn read_json_value(path: &Path) -> CoreResult<Value> {
    if !path.exists() {
        return Ok(Value::Null);
    }
    let raw = fs::read_to_string(path)
        .map_err(|error| CoreError::io(format!("无法读取 {}：{error}", path.display())))?;
    serde_json::from_str(&raw).map_err(|error| {
        CoreError::new(
            crate::error::ErrorKind::Io,
            "DATA_CORRUPTED",
            format!("数据文件损坏 {}：{error}", path.display()),
        )
        .with_hint("运行 kxtodo-cli doctor 检查；可从 backups/ 恢复最近备份")
    })
}

pub fn write_json_atomic(path: &Path, value: &Value) -> CoreResult<()> {
    let raw = serde_json::to_string_pretty(value)?;
    atomic_write(path, &raw)
}

pub struct Repository {
    pub layout: Layout,
}

pub struct WriteOutcome {
    pub revision: u64,
    pub replayed: bool,
    pub replay_summary: Option<Value>,
    pub warnings: Vec<Value>,
}

impl Repository {
    pub fn open(root: PathBuf) -> CoreResult<Self> {
        let layout = Layout::new(root);
        layout.ensure()?;
        Ok(Self { layout })
    }

    pub fn open_readonly(root: PathBuf) -> Self {
        Self {
            layout: Layout::new(root),
        }
    }

    /// Load + migrate (if needed) under exclusive lock. Safe to call on every process start.
    pub fn load_all(&self) -> CoreResult<(DataFile, SettingsFile, ScheduleFile)> {
        let _lock = RepoLock::acquire(&self.layout)?;
        crate::migrate::migrate_if_needed(&self.layout)?;
        Ok((
            self.load_data()?,
            self.load_settings()?,
            self.load_schedule()?,
        ))
    }

    /// GUI/Host 启动时调用：缺失的领域文件立即以默认值落盘。
    /// CLI 依赖 data.json 存在来判定数据目录有效；首次启动 GUI 后即满足。
    pub fn ensure_initialized(&self) -> CoreResult<()> {
        if !self.layout.data_file().exists() {
            self.write_data(None, None, "host.init", |_file| {
                Ok(serde_json::json!({ "initialized": true }))
            })?;
        }
        if !self.layout.settings_file().exists() {
            self.write_settings(None, None, "host.init", |_file| {
                Ok(serde_json::json!({ "initialized": true }))
            })?;
        }
        if !self.layout.schedule_file().exists() {
            self.write_schedule(None, None, "host.init", |_file| {
                Ok(serde_json::json!({ "initialized": true }))
            })?;
        }
        Ok(())
    }

    pub fn load_data(&self) -> CoreResult<DataFile> {
        let value = read_json_value(&self.layout.data_file())?;
        if value.is_null() {
            return Ok(default_data_file());
        }
        serde_json::from_value(value).map_err(|error| {
            CoreError::new(
                crate::error::ErrorKind::Io,
                "DATA_CORRUPTED",
                format!("data.json 结构无效：{error}"),
            )
        })
    }

    pub fn load_settings(&self) -> CoreResult<SettingsFile> {
        let value = read_json_value(&self.layout.settings_file())?;
        if value.is_null() {
            return Ok(SettingsFile {
                meta: DomainMeta {
                    revision: 0,
                    schema_version: Some(SETTINGS_SCHEMA_VERSION),
                    idempotency: Vec::new(),
                    tombstones: Vec::new(),
                    extra: Map::new(),
                },
                ..Default::default()
            });
        }
        serde_json::from_value(value).map_err(|error| {
            CoreError::new(
                crate::error::ErrorKind::Io,
                "DATA_CORRUPTED",
                format!("settings.json 结构无效：{error}"),
            )
        })
    }

    pub fn load_schedule(&self) -> CoreResult<ScheduleFile> {
        let value = read_json_value(&self.layout.schedule_file())?;
        if value.is_null() {
            return Ok(ScheduleFile {
                meta: DomainMeta {
                    revision: 0,
                    schema_version: Some(SCHEDULE_SCHEMA_VERSION),
                    idempotency: Vec::new(),
                    tombstones: Vec::new(),
                    extra: Map::new(),
                },
                ..Default::default()
            });
        }
        serde_json::from_value(value).map_err(|error| {
            CoreError::new(
                crate::error::ErrorKind::Io,
                "DATA_CORRUPTED",
                format!("tasks.json 结构无效：{error}"),
            )
        })
    }

    pub fn lookup_schedule_idempotency(
        &self,
        command: &str,
        key: Option<&str>,
    ) -> CoreResult<Option<(u64, Value)>> {
        let Some(key) = key else {
            return Ok(None);
        };
        let _lock = RepoLock::acquire(&self.layout)?;
        crate::migrate::migrate_if_needed(&self.layout)?;
        let file = self.load_schedule()?;
        Ok(file
            .meta
            .idempotency
            .iter()
            .find(|record| record.command == command && record.key == key)
            .map(|record| (file.meta.revision, record.summary.clone())))
    }

    pub fn lookup_settings_idempotency(
        &self,
        command: &str,
        key: Option<&str>,
    ) -> CoreResult<Option<(u64, Value)>> {
        let Some(key) = key else {
            return Ok(None);
        };
        let _lock = RepoLock::acquire(&self.layout)?;
        crate::migrate::migrate_if_needed(&self.layout)?;
        let file = self.load_settings()?;
        Ok(file
            .meta
            .idempotency
            .iter()
            .find(|record| record.command == command && record.key == key)
            .map(|record| (file.meta.revision, record.summary.clone())))
    }

    pub fn lookup_data_idempotency(
        &self,
        command: &str,
        key: Option<&str>,
    ) -> CoreResult<Option<(u64, Value)>> {
        let Some(key) = key else {
            return Ok(None);
        };
        let _lock = RepoLock::acquire(&self.layout)?;
        crate::migrate::migrate_if_needed(&self.layout)?;
        let file = self.load_data()?;
        Ok(file
            .meta
            .idempotency
            .iter()
            .find(|record| record.command == command && record.key == key)
            .map(|record| (file.meta.revision, record.summary.clone())))
    }

    /// Transactional write (§4.3): lock → reload → revision check → idempotency → mutate →
    /// bump revision → append ledger → audit → atomic persist.
    ///
    /// `mutate` receives the freshly loaded domain JSON (typed) and returns the
    /// idempotency summary to store (if an idempotency key is used).
    pub fn write_data<F>(
        &self,
        expected_revision: Option<u64>,
        idempotency_key: Option<&str>,
        command: &str,
        mutate: F,
    ) -> CoreResult<(DataFile, WriteOutcome)>
    where
        F: FnOnce(&mut DataFile) -> CoreResult<Value>,
    {
        let _lock = RepoLock::acquire(&self.layout)?;
        crate::migrate::migrate_if_needed(&self.layout)?;
        let mut file = self.load_data()?;
        let outcome = self.prepare_write(
            Domain::Data,
            &file.meta,
            expected_revision,
            idempotency_key,
            command,
        )?;
        if let Some(summary) = outcome.replay_summary.clone() {
            return Ok((file, outcome.with_summary(summary)));
        }
        let summary = mutate(&mut file)?;
        file.schema_version = DATA_SCHEMA_VERSION;
        file.meta.revision += 1;
        file.meta.schema_version = None;
        finalize_meta(&mut file.meta, idempotency_key, command, summary.clone());
        let raw = serde_json::to_string_pretty(&file)?;
        atomic_write(&self.layout.data_file(), &raw)?;
        let revision = file.meta.revision;
        let mut outcome = outcome.finish(revision);
        if let Err(error) = self.audit(command, Domain::Data, revision, &summary) {
            outcome.warnings.push(audit_warning(&error));
        }
        Ok((file, outcome))
    }

    /// High-impact data transaction: idempotency check and consistent backup
    /// happen while the repository lock is held; a durable recovery record is
    /// published before the logical JSON commit.
    pub fn write_data_with_recovery<F>(
        &self,
        expected_revision: Option<u64>,
        idempotency_key: Option<&str>,
        command: &str,
        mut recovery: Value,
        mutate: F,
    ) -> CoreResult<(DataFile, WriteOutcome, Option<String>)>
    where
        F: FnOnce(&mut DataFile) -> CoreResult<Value>,
    {
        let _lock = RepoLock::acquire(&self.layout)?;
        crate::migrate::migrate_if_needed(&self.layout)?;
        let mut file = self.load_data()?;
        let outcome = self.prepare_write(
            Domain::Data,
            &file.meta,
            expected_revision,
            idempotency_key,
            command,
        )?;
        if let Some(summary) = outcome.replay_summary.clone() {
            return Ok((file, outcome.with_summary(summary), None));
        }

        let backup = self.backup_locked("cascade-remove")?;
        let summary = mutate(&mut file)?;
        file.schema_version = DATA_SCHEMA_VERSION;
        file.meta.revision += 1;
        file.meta.schema_version = None;
        finalize_meta(&mut file.meta, idempotency_key, command, summary.clone());

        let recovery_id = recovery
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(crate::ids::request_id);
        recovery["id"] = Value::String(recovery_id.clone());
        recovery["command"] = Value::String(command.to_string());
        recovery["createdAt"] = Value::String(now_iso());
        recovery["status"] = Value::String("prepared".to_string());
        recovery["backup"] = Value::String(backup.display().to_string());
        recovery["targetRevision"] = serde_json::json!(file.meta.revision);
        self.upsert_recovery_locked(recovery.clone())?;

        let raw = serde_json::to_string_pretty(&file)?;
        if let Err(error) = atomic_write(&self.layout.data_file(), &raw) {
            let _ = self.remove_recovery_locked(&recovery_id);
            return Err(error);
        }

        let revision = file.meta.revision;
        let mut outcome = outcome.finish(revision);
        recovery["status"] = Value::String("committed".to_string());
        recovery["committedAt"] = Value::String(now_iso());
        if let Err(error) = self.upsert_recovery_locked(recovery) {
            outcome.warnings.push(serde_json::json!({
                "code": "RECOVERY_RECORD_UPDATE_FAILED",
                "message": format!("业务删除已提交，但恢复记录状态更新失败：{}", error.message),
                "recoveryId": recovery_id,
            }));
        }
        if let Err(error) = self.audit(command, Domain::Data, revision, &summary) {
            outcome.warnings.push(audit_warning(&error));
        }
        Ok((file, outcome, Some(recovery_id)))
    }

    pub fn read_recovery_records(&self) -> CoreResult<Vec<Value>> {
        let path = self.layout.recovery_file();
        let value = read_json_value(&path)?;
        if value.is_null() {
            return Ok(Vec::new());
        }
        let version = value.get("version").and_then(Value::as_u64);
        let records = value
            .get("records")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                CoreError::new(
                    crate::error::ErrorKind::Io,
                    "RECOVERY_CORRUPTED",
                    format!("恢复记录结构无效：{}", path.display()),
                )
            })?;
        if version != Some(1)
            || records.iter().any(|record| {
                !record.is_object()
                    || record.get("id").and_then(Value::as_str).is_none()
                    || record.get("kind").and_then(Value::as_str).is_none()
            })
        {
            return Err(CoreError::new(
                crate::error::ErrorKind::Io,
                "RECOVERY_CORRUPTED",
                format!("恢复记录版本或条目无效：{}", path.display()),
            ));
        }
        Ok(records.clone())
    }

    pub fn finish_recovery(
        &self,
        id: &str,
        error: Option<&str>,
        pending_paths: &[String],
    ) -> CoreResult<()> {
        let _lock = RepoLock::acquire(&self.layout)?;
        if let Some(message) = error {
            let mut records = self.read_recovery_records()?;
            if let Some(record) = records
                .iter_mut()
                .find(|record| record.get("id").and_then(Value::as_str) == Some(id))
            {
                record["status"] = Value::String("pending".to_string());
                record["lastError"] = Value::String(message.to_string());
                record["pendingPaths"] = serde_json::json!(pending_paths);
                record["updatedAt"] = Value::String(now_iso());
            }
            self.write_recovery_records_locked(&records)
        } else {
            self.remove_recovery_locked(id)
        }
    }

    fn upsert_recovery_locked(&self, record: Value) -> CoreResult<()> {
        let id = record.get("id").and_then(Value::as_str).unwrap_or_default();
        let mut records = self.read_recovery_records()?;
        if let Some(existing) = records
            .iter_mut()
            .find(|item| item.get("id").and_then(Value::as_str) == Some(id))
        {
            *existing = record;
        } else {
            records.push(record);
        }
        self.write_recovery_records_locked(&records)
    }

    fn remove_recovery_locked(&self, id: &str) -> CoreResult<()> {
        let mut records = self.read_recovery_records()?;
        records.retain(|record| record.get("id").and_then(Value::as_str) != Some(id));
        self.write_recovery_records_locked(&records)
    }

    fn write_recovery_records_locked(&self, records: &[Value]) -> CoreResult<()> {
        if records.is_empty() {
            if self.layout.recovery_file().exists() {
                fs::remove_file(self.layout.recovery_file())?;
            }
            return Ok(());
        }
        write_json_atomic(
            &self.layout.recovery_file(),
            &serde_json::json!({ "version": 1, "records": records }),
        )
    }

    pub fn write_settings<F>(
        &self,
        expected_revision: Option<u64>,
        idempotency_key: Option<&str>,
        command: &str,
        mutate: F,
    ) -> CoreResult<(SettingsFile, WriteOutcome)>
    where
        F: FnOnce(&mut SettingsFile) -> CoreResult<Value>,
    {
        let _lock = RepoLock::acquire(&self.layout)?;
        crate::migrate::migrate_if_needed(&self.layout)?;
        let mut file = self.load_settings()?;
        let outcome = self.prepare_write(
            Domain::Settings,
            &file.meta,
            expected_revision,
            idempotency_key,
            command,
        )?;
        if let Some(summary) = outcome.replay_summary.clone() {
            return Ok((file, outcome.with_summary(summary)));
        }
        let summary = mutate(&mut file)?;
        file.meta.revision += 1;
        file.meta.schema_version = Some(SETTINGS_SCHEMA_VERSION);
        finalize_meta(&mut file.meta, idempotency_key, command, summary.clone());
        let raw = serde_json::to_string_pretty(&file)?;
        atomic_write(&self.layout.settings_file(), &raw)?;
        let revision = file.meta.revision;
        let mut outcome = outcome.finish(revision);
        if let Err(error) = self.audit(command, Domain::Settings, revision, &summary) {
            outcome.warnings.push(audit_warning(&error));
        }
        Ok((file, outcome))
    }

    pub fn write_schedule<F>(
        &self,
        expected_revision: Option<u64>,
        idempotency_key: Option<&str>,
        command: &str,
        mutate: F,
    ) -> CoreResult<(ScheduleFile, WriteOutcome)>
    where
        F: FnOnce(&mut ScheduleFile) -> CoreResult<Value>,
    {
        self.write_schedule_impl(expected_revision, idempotency_key, command, true, mutate)
    }

    /// Scheduler state-machine bookkeeping is persisted transactionally but is
    /// intentionally excluded from the user-level audit trail.
    pub fn write_schedule_internal<F>(
        &self,
        command: &str,
        mutate: F,
    ) -> CoreResult<(ScheduleFile, WriteOutcome)>
    where
        F: FnOnce(&mut ScheduleFile) -> CoreResult<Value>,
    {
        self.write_schedule_impl(None, None, command, false, mutate)
    }

    fn write_schedule_impl<F>(
        &self,
        expected_revision: Option<u64>,
        idempotency_key: Option<&str>,
        command: &str,
        audit: bool,
        mutate: F,
    ) -> CoreResult<(ScheduleFile, WriteOutcome)>
    where
        F: FnOnce(&mut ScheduleFile) -> CoreResult<Value>,
    {
        let _lock = RepoLock::acquire(&self.layout)?;
        crate::migrate::migrate_if_needed(&self.layout)?;
        let mut file = self.load_schedule()?;
        let outcome = self.prepare_write(
            Domain::Schedule,
            &file.meta,
            expected_revision,
            idempotency_key,
            command,
        )?;
        if let Some(summary) = outcome.replay_summary.clone() {
            return Ok((file, outcome.with_summary(summary)));
        }
        let summary = mutate(&mut file)?;
        file.meta.revision += 1;
        file.meta.schema_version = Some(SCHEDULE_SCHEMA_VERSION);
        finalize_meta(&mut file.meta, idempotency_key, command, summary.clone());
        let raw = serde_json::to_string_pretty(&file)?;
        atomic_write(&self.layout.schedule_file(), &raw)?;
        let revision = file.meta.revision;
        let mut outcome = outcome.finish(revision);
        if audit {
            if let Err(error) = self.audit(command, Domain::Schedule, revision, &summary) {
                outcome.warnings.push(audit_warning(&error));
            }
        }
        Ok((file, outcome))
    }

    fn prepare_write(
        &self,
        domain: Domain,
        meta: &DomainMeta,
        expected_revision: Option<u64>,
        idempotency_key: Option<&str>,
        command: &str,
    ) -> CoreResult<WriteOutcome> {
        // Exact replays win over the original revision guard: a successful
        // first request necessarily advanced the domain revision.
        if let Some(key) = idempotency_key {
            if let Some(record) = meta
                .idempotency
                .iter()
                .find(|item| item.key == key && item.command == command)
            {
                return Ok(WriteOutcome {
                    revision: meta.revision,
                    replayed: true,
                    replay_summary: Some(record.summary.clone()),
                    warnings: Vec::new(),
                });
            }
        }
        if let Some(expected) = expected_revision {
            if expected != meta.revision {
                return Err(CoreError::conflict(
                    "REVISION_CONFLICT",
                    format!(
                        "{} 域当前 revision 为 {}，与 --if-revision {} 不匹配",
                        domain.as_str(),
                        meta.revision,
                        expected
                    ),
                )
                .with_hint("重新读取最新数据后重试"));
            }
        }
        Ok(WriteOutcome {
            revision: meta.revision,
            replayed: false,
            replay_summary: None,
            warnings: Vec::new(),
        })
    }

    fn audit(
        &self,
        command: &str,
        domain: Domain,
        revision: u64,
        summary: &Value,
    ) -> CoreResult<()> {
        let entry = serde_json::json!({
            "at": now_iso(),
            "command": command,
            "domain": domain.as_str(),
            "revision": revision,
            "summary": compact_audit_summary(summary),
        });
        crate::history::append_bounded_jsonl(
            &self.layout.audit_history(),
            &entry,
            AUDIT_MAX_BYTES,
            None,
        )
    }

    /// Full JSON backup set of the three domain files (§4.2.3).
    pub fn backup(&self, reason: &str) -> CoreResult<PathBuf> {
        let _lock = RepoLock::acquire(&self.layout)?;
        self.backup_locked(reason)
    }

    fn backup_locked(&self, reason: &str) -> CoreResult<PathBuf> {
        let stamp = now_iso().replace([':', '.'], "-");
        let dir = self.layout.backup_dir().join(format!("{stamp}-{reason}"));
        fs::create_dir_all(&dir)?;
        for file in [DATA_FILE, SETTINGS_FILE, SCHEDULE_FILE] {
            let src = self.layout.root.join(file);
            if src.exists() {
                fs::copy(&src, dir.join(file))?;
            }
        }
        prune_backups(&self.layout.backup_dir(), BACKUP_KEEP)?;
        Ok(dir)
    }
}

impl WriteOutcome {
    fn with_summary(self, summary: Value) -> Self {
        Self {
            replay_summary: Some(summary),
            ..self
        }
    }

    fn finish(self, revision: u64) -> Self {
        Self {
            revision,
            replayed: false,
            replay_summary: None,
            warnings: self.warnings,
        }
    }
}

fn audit_warning(error: &CoreError) -> Value {
    serde_json::json!({
        "code": "AUDIT_WRITE_FAILED",
        "message": format!("业务数据已提交，但审计日志写入失败：{}", error.message),
    })
}

fn compact_audit_summary(summary: &Value) -> Value {
    // Audit keeps only identifying fields, never full markdown or script bodies.
    match summary {
        Value::Object(map) => {
            let mut compact = Map::new();
            for key in ["id", "ids", "type", "action", "path", "count", "name"] {
                if let Some(value) = map.get(key) {
                    compact.insert(key.to_string(), value.clone());
                }
            }
            if compact.is_empty() {
                Value::Null
            } else {
                Value::Object(compact)
            }
        }
        _ => Value::Null,
    }
}

fn finalize_meta(
    meta: &mut DomainMeta,
    idempotency_key: Option<&str>,
    command: &str,
    summary: Value,
) {
    prune_idempotency(meta);
    if let Some(key) = idempotency_key {
        meta.idempotency.push(IdempotencyRecord {
            key: key.to_string(),
            command: command.to_string(),
            created_at: now_iso(),
            summary,
        });
    }
}

fn prune_idempotency(meta: &mut DomainMeta) {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(IDEMPOTENCY_MAX_AGE_DAYS);
    meta.idempotency.retain(|record| {
        crate::time::parse_stored_instant(&record.created_at)
            .map(|at| at > cutoff)
            .unwrap_or(false)
    });
    let overflow = meta
        .idempotency
        .len()
        .saturating_sub(IDEMPOTENCY_MAX_RECORDS);
    if overflow > 0 {
        meta.idempotency.drain(0..overflow);
    }
}

fn prune_backups(dir: &Path, keep: usize) -> CoreResult<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok().map(|item| item.path()))
        .filter(|path| path.is_dir())
        .collect();
    entries.sort();
    while entries.len() > keep {
        let oldest = entries.remove(0);
        let _ = fs::remove_dir_all(oldest);
    }
    Ok(())
}

/// List crash leftovers without mutating the repository (doctor is read-only).
pub fn list_temp_files(root: &Path) -> CoreResult<Vec<PathBuf>> {
    let mut paths = Vec::new();
    if !root.is_dir() {
        return Ok(paths);
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') && name.ends_with(".tmp") {
            paths.push(entry.path());
        }
    }
    Ok(paths)
}

/// Explicit cleanup primitive; callers must enforce high-risk confirmation.
pub fn cleanup_temp_files(root: &Path) -> CoreResult<Vec<PathBuf>> {
    let mut removed = Vec::new();
    for path in list_temp_files(root)? {
        if fs::remove_file(&path).is_ok() {
            removed.push(path);
        }
    }
    Ok(removed)
}

pub fn default_data_file() -> DataFile {
    use crate::model::{Node, NodeKind};
    let now = now_iso();
    let mut nodes: Vec<Node> = crate::model::SYSTEM_NODE_IDS
        .iter()
        .enumerate()
        .map(|(index, id)| Node {
            id: id.to_string(),
            kind: NodeKind::System,
            name: match *id {
                "my-day" => "我的一天".to_string(),
                "planned" => "计划内".to_string(),
                "important" => "收藏".to_string(),
                _ => "定时任务".to_string(),
            },
            icon: match *id {
                "my-day" => "sun".to_string(),
                "planned" => "calendar".to_string(),
                "important" => "star".to_string(),
                _ => "clock".to_string(),
            },
            parent_id: None,
            order: index as f64,
            collapsed: None,
            created_at: now.clone(),
            updated_at: Some(now.clone()),
            extra: Map::new(),
        })
        .collect();
    let inbox_id = crate::ids::gen_id("entry");
    nodes.push(Node {
        id: inbox_id.clone(),
        kind: NodeKind::Entry,
        name: "收集箱".to_string(),
        icon: "inbox".to_string(),
        parent_id: None,
        order: crate::model::SYSTEM_NODE_IDS.len() as f64,
        collapsed: None,
        created_at: now.clone(),
        updated_at: Some(now.clone()),
        extra: Map::new(),
    });
    let mut backgrounds = Map::new();
    backgrounds.insert(
        inbox_id.clone(),
        serde_json::json!({ "color": "#f4f1ea", "imageOpacity": 0.28 }),
    );
    DataFile {
        schema_version: DATA_SCHEMA_VERSION,
        meta: DomainMeta {
            revision: 0,
            schema_version: None,
            idempotency: Vec::new(),
            tombstones: Vec::new(),
            extra: Map::new(),
        },
        nodes,
        tasks: Vec::new(),
        selected_node_id: inbox_id,
        backgrounds,
        extra: Map::new(),
    }
}

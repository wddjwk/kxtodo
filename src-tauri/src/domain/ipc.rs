//! Local IPC between CLI and Background Host (§4.4).
//! Length-prefixed JSON frames over local sockets; owner-session only.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use interprocess::local_socket::{prelude::*, GenericNamespaced, ListenerOptions, Stream};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::core::{Controls, ExecOutcome};
use crate::domain::error::{CoreError, CoreResult};

pub const PROTOCOL_VERSION: u32 = 2;
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostDescriptor {
    pub protocol_version: u32,
    pub pid: u32,
    pub data_dir: String,
    pub endpoint: String,
    pub mode: String,
    pub started_at: String,
    pub token: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IpcRequest {
    pub protocol_version: u32,
    pub request_id: String,
    pub data_dir: String,
    pub cwd: String,
    pub token: String,
    pub command: String,
    pub params: Value,
    pub controls: IpcControls,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct IpcControls {
    pub dry_run: bool,
    pub yes: bool,
    pub idempotency_key: Option<String>,
    pub if_revision: Option<u64>,
}

impl From<&Controls> for IpcControls {
    fn from(controls: &Controls) -> Self {
        Self {
            dry_run: controls.dry_run,
            yes: controls.yes,
            idempotency_key: controls.idempotency_key.clone(),
            if_revision: controls.if_revision,
        }
    }
}

pub fn normalize_absolute_path(path: &Path, base: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };
    let mut out = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

pub fn normalize_data_dir(data_dir: &Path) -> PathBuf {
    let base = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    normalize_absolute_path(data_dir, &base)
}

pub fn same_data_dir(a: &Path, b: &Path) -> bool {
    let a = normalize_data_dir(a);
    let b = normalize_data_dir(b);
    #[cfg(target_os = "windows")]
    {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    }
    #[cfg(not(target_os = "windows"))]
    {
        a == b
    }
}

pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Deterministic IPC endpoint name for a data dir.
pub fn endpoint_for(data_dir: &Path) -> String {
    let canonical = normalize_data_dir(data_dir);
    let mut hasher = DefaultHasher::new();
    #[cfg(target_os = "windows")]
    canonical.to_string_lossy().to_lowercase().hash(&mut hasher);
    #[cfg(not(target_os = "windows"))]
    canonical.hash(&mut hasher);
    format!("kxtodo-host-{:016x}", hasher.finish())
}

fn ns_name(endpoint: &str) -> CoreResult<interprocess::local_socket::Name<'static>> {
    endpoint
        .to_string()
        .to_ns_name::<GenericNamespaced>()
        .map_err(|error| CoreError::io(format!("IPC 端点名称无效：{error}")))
}

pub fn read_host_descriptor(path: &Path) -> Option<HostDescriptor> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

pub fn write_host_descriptor(path: &Path, descriptor: &HostDescriptor) -> CoreResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    let raw = serde_json::to_string_pretty(descriptor)?;
    crate::domain::repo::atomic_write(path, &raw)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
pub fn remove_host_descriptor(path: &Path) {
    let _ = std::fs::remove_file(path);
}

#[cfg(target_os = "windows")]
pub fn host_process_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return false;
    }
    unsafe {
        CloseHandle(handle);
    }
    true
}

#[cfg(not(target_os = "windows"))]
pub fn host_process_alive(pid: u32) -> bool {
    PathBuf::from(format!("/proc/{pid}")).exists()
}

/// Discover a live Host for `data_dir`: descriptor → protocol/pid sanity → IPC ping.
pub fn discover_host(data_dir: &Path) -> Option<(HostDescriptor, PathBuf)> {
    let descriptor_path = data_dir
        .join(crate::domain::repo::RUNTIME_DIR)
        .join(crate::domain::repo::HOST_DESCRIPTOR);
    let descriptor = read_host_descriptor(&descriptor_path)?;
    if descriptor.protocol_version != PROTOCOL_VERSION {
        return None;
    }
    if !host_process_alive(descriptor.pid) {
        return None;
    }
    Some((descriptor, descriptor_path))
}

pub struct IpcClient {
    stream: Stream,
}

impl IpcClient {
    pub fn connect(endpoint: &str) -> CoreResult<Self> {
        let name = ns_name(endpoint)?;
        let stream = Stream::connect(name).map_err(|error| {
            CoreError::io(format!("无法连接 Host IPC 端点 {endpoint}：{error}"))
        })?;
        Ok(Self { stream })
    }

    /// Ping: send a `host.ping` request and require an `ok` reply.
    pub fn ping(&mut self, data_dir: &Path, token: &str) -> bool {
        let normalized = normalize_data_dir(data_dir);
        let request = IpcRequest {
            protocol_version: PROTOCOL_VERSION,
            request_id: crate::domain::ids::request_id(),
            data_dir: normalized.to_string_lossy().to_string(),
            cwd: normalized.to_string_lossy().to_string(),
            token: token.to_string(),
            command: "host.ping".to_string(),
            params: Value::Null,
            controls: IpcControls::default(),
        };
        match self.roundtrip(&request) {
            Ok(response) => response.get("ok").and_then(Value::as_bool).unwrap_or(false),
            Err(_) => false,
        }
    }

    pub fn invoke(&mut self, request: &IpcRequest) -> CoreResult<Value> {
        self.roundtrip(request)
    }

    fn roundtrip(&mut self, request: &IpcRequest) -> CoreResult<Value> {
        let payload = serde_json::to_vec(request)?;
        write_frame(&mut self.stream, &payload)?;
        let response = read_frame(&mut self.stream)?;
        serde_json::from_slice(&response)
            .map_err(|error| CoreError::io(format!("Host 响应无法解析：{error}")))
    }
}

fn write_frame(stream: &mut Stream, payload: &[u8]) -> CoreResult<()> {
    if payload.len() > MAX_FRAME_BYTES {
        return Err(CoreError::validation(
            "REQUEST_TOO_LARGE",
            format!("IPC 请求超过 {} 字节上限", MAX_FRAME_BYTES),
        ));
    }
    let len = (payload.len() as u32).to_be_bytes();
    stream.write_all(&len)?;
    stream.write_all(payload)?;
    stream.flush()?;
    Ok(())
}

fn read_exact(stream: &mut Stream, buf: &mut [u8]) -> CoreResult<()> {
    let mut filled = 0;
    while filled < buf.len() {
        let n = stream.read(&mut buf[filled..])?;
        if n == 0 {
            return Err(CoreError::io("IPC 连接被对端关闭"));
        }
        filled += n;
    }
    Ok(())
}

fn read_frame(stream: &mut Stream) -> CoreResult<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    read_exact(stream, &mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(CoreError::validation(
            "RESPONSE_TOO_LARGE",
            format!("IPC 帧超过 {} 字节上限", MAX_FRAME_BYTES),
        ));
    }
    let mut payload = vec![0u8; len];
    read_exact(stream, &mut payload)?;
    Ok(payload)
}

/// Blocking IPC server bound to a data-dir endpoint.
pub struct IpcServer {
    listener: interprocess::local_socket::Listener,
    pub endpoint: String,
}

impl IpcServer {
    pub fn bind(data_dir: &Path) -> CoreResult<Self> {
        let endpoint = endpoint_for(data_dir);
        let name = ns_name(&endpoint)?;
        let listener = ListenerOptions::new()
            .name(name)
            .create_sync()
            .map_err(|error| {
                CoreError::io(format!(
                    "无法绑定 Host IPC 端点 {endpoint}：{error}（可能有其他 Host 占用）"
                ))
            })?;
        Ok(Self { listener, endpoint })
    }

    /// Accept one raw connection (spawn a thread per connection for concurrency).
    pub fn accept_raw(&self) -> CoreResult<Stream> {
        self.listener
            .accept()
            .map_err(|error| CoreError::io(format!("IPC accept 失败：{error}")))
    }

    /// Accept one connection and serve requests on it until the client
    /// disconnects (call from a loop/thread).
    pub fn accept<F>(&self, handler: F) -> CoreResult<()>
    where
        F: Fn(IpcRequest) -> Value,
    {
        let mut stream = self.accept_raw()?;
        serve_connection(&mut stream, &handler)
    }
}

/// Read → handle → write frames on one connection until EOF or fatal error.
pub fn serve_connection<F>(stream: &mut Stream, handler: &F) -> CoreResult<()>
where
    F: Fn(IpcRequest) -> Value,
{
    loop {
        let payload = match read_frame(stream) {
            Ok(payload) => payload,
            Err(_) => return Ok(()), // 客户端断开
        };
        let request: IpcRequest = match serde_json::from_slice(&payload) {
            Ok(request) => request,
            Err(error) => {
                let failure = crate::domain::envelope::failure(
                    "ipc",
                    &CoreError::validation("IPC_BAD_REQUEST", format!("IPC 请求无法解析：{error}")),
                    crate::domain::envelope::Meta::default(),
                );
                let raw = serde_json::to_vec(&failure)?;
                write_frame(stream, &raw)?;
                continue;
            }
        };
        let response = handler(request);
        let raw = serde_json::to_vec(&response)?;
        write_frame(stream, &raw)?;
    }
}

/// Validate an incoming request against this host.
pub fn validate_request(request: &IpcRequest, data_dir: &Path, token: &str) -> CoreResult<()> {
    if request.protocol_version != PROTOCOL_VERSION {
        return Err(CoreError::validation(
            "IPC_PROTOCOL_MISMATCH",
            format!(
                "IPC 协议版本不匹配：请求 {}，Host {}",
                request.protocol_version, PROTOCOL_VERSION
            ),
        ));
    }
    if request.request_id.trim().is_empty() || request.request_id.len() > 128 {
        return Err(CoreError::validation(
            "IPC_REQUEST_ID_INVALID",
            "IPC requestId 不能为空且不得超过 128 字节",
        ));
    }
    let authenticated = request.token.len() == token.len()
        && request
            .token
            .as_bytes()
            .iter()
            .zip(token.as_bytes())
            .fold(true, |equal, (a, b)| equal & (a == b));
    if !authenticated {
        return Err(CoreError::validation(
            "IPC_UNAUTHORIZED",
            "IPC 请求未通过 Host 认证",
        ));
    }
    if !same_data_dir(data_dir, &PathBuf::from(&request.data_dir)) {
        return Err(CoreError::validation(
            "IPC_DATA_DIR_MISMATCH",
            "IPC 请求的数据目录与本 Host 不一致",
        ));
    }
    let cwd = PathBuf::from(&request.cwd);
    if request.cwd.trim().is_empty()
        || request.cwd.contains('\0')
        || !cwd.is_absolute()
        || !cwd.is_dir()
    {
        return Err(CoreError::validation(
            "IPC_CWD_INVALID",
            "IPC cwd 必须是存在的绝对目录",
        ));
    }
    Ok(())
}

/// Convert an ExecOutcome into the wire envelope (identical shape to standalone CLI).
pub fn outcome_envelope(outcome: &ExecOutcome) -> Value {
    let mut envelope = outcome.envelope.clone();
    envelope["meta"]["exitCode"] = serde_json::json!(outcome.code);
    envelope
}

use std::{
    collections::VecDeque,
    ffi::OsStr,
    io,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Mutex, MutexGuard},
    time::Duration,
};

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader},
    process::{Child, Command},
    sync::oneshot,
    time,
};

use super::{command_path, discover, executable};

const MIB: usize = 1024 * 1024;
const MAX_STATUS_BYTES: usize = 4 * MIB;
const MAX_SCAN_BYTES: usize = 16 * MIB;
const MAX_ERROR_BYTES: usize = 32 * 1024;
const STATUS_TIMEOUT: Duration = Duration::from_secs(12);
const ANALYZE_TIMEOUT: Duration = Duration::from_secs(120);
const SCAN_CANCELLED: &str = "已取消本次扫描。";
const RECENT_CANCELLATIONS: usize = 32;

#[derive(Default)]
pub(crate) struct MoleScan(Mutex<ScanRegistry>);

#[derive(Default)]
struct ScanRegistry {
    active: Option<ActiveScan>,
    early_cancellations: VecDeque<ScanKey>,
}

struct ActiveScan {
    key: ScanKey,
    cancel: Option<oneshot::Sender<()>>,
}

#[derive(PartialEq, Eq)]
struct ScanKey {
    webview: String,
    request_id: String,
}

impl ScanKey {
    fn new(webview: &str, request_id: String) -> Result<Self, String> {
        if !(8..=64).contains(&request_id.len())
            || !request_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err("扫描请求编号无效。".to_string());
        }
        Ok(Self {
            webview: webview.to_owned(),
            request_id,
        })
    }
}

impl MoleScan {
    fn registry(&self) -> MutexGuard<'_, ScanRegistry> {
        self.0.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn start(&self, key: ScanKey) -> Result<(ScanSlot<'_>, oneshot::Receiver<()>), String> {
        let mut registry = self.registry();
        if let Some(index) = registry
            .early_cancellations
            .iter()
            .position(|cancelled| cancelled == &key)
        {
            registry.early_cancellations.remove(index);
            return Err(SCAN_CANCELLED.to_string());
        }
        if registry.active.is_some() {
            return Err("已有磁盘扫描正在运行。".to_string());
        }
        let (cancel, signal) = oneshot::channel();
        registry.active = Some(ActiveScan {
            key,
            cancel: Some(cancel),
        });
        Ok((ScanSlot(self), signal))
    }

    fn cancel(&self, key: ScanKey) {
        let mut registry = self.registry();
        if let Some(scan) = registry.active.as_mut().filter(|scan| scan.key == key) {
            if let Some(cancel) = scan.cancel.take() {
                let _ = cancel.send(());
            }
        } else if !registry.early_cancellations.contains(&key) {
            // 页面销毁后发出的取消 IPC 有可能先于启动 IPC 被处理。
            if registry.early_cancellations.len() >= RECENT_CANCELLATIONS {
                registry.early_cancellations.pop_front();
            }
            registry.early_cancellations.push_back(key);
        }
    }
}

struct ScanSlot<'a>(&'a MoleScan);

impl Drop for ScanSlot<'_> {
    fn drop(&mut self) {
        self.0.registry().active = None;
    }
}

#[derive(Serialize)]
pub(crate) struct MoleInfo {
    path: Option<String>,
    version: Option<String>,
    home: String,
    bundled: bool,
}

#[tauri::command]
pub(crate) async fn mole_info(app: AppHandle) -> Result<MoleInfo, String> {
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    let mut info = MoleInfo {
        path: None,
        version: None,
        home: home.to_string_lossy().into_owned(),
        bundled: false,
    };
    if let Some(mole) = discover(&app)? {
        let version = run_output(&mole.path, &["--version"], Duration::from_secs(5), 4096)
            .await
            .and_then(|stdout| {
                version_from_output(&stdout).ok_or_else(|| "无法识别 Mole 版本。".to_string())
            });
        info.version = match version {
            Ok(version) => Some(version),
            Err(error) if mole.bundled => {
                return Err(format!("内置 Mole 无法启动。请重新安装 HostGuard。{error}"));
            }
            Err(_) => None,
        };
        info.path = Some(mole.path.to_string_lossy().into_owned());
        info.bundled = mole.bundled;
    }
    Ok(info)
}

#[tauri::command]
pub(crate) async fn mole_read_status(app: AppHandle) -> Result<Value, String> {
    let executable = executable(&app)?;
    if let Some(snapshot) = warm_status(&executable).await? {
        return Ok(snapshot);
    }
    // 旧版 Mole 只有一次性 JSON。继续显示其他指标，避免把无采样基线的
    // 读写和网络速率误显示为 0。
    let mut snapshot = json_output(
        &executable,
        &["status", "--json"],
        STATUS_TIMEOUT,
        MAX_STATUS_BYTES,
    )
    .await?;
    if let Some(status) = snapshot.as_object_mut() {
        status.remove("disk_io");
        status.remove("network_history");
        if let Some(interfaces) = status.get_mut("network").and_then(Value::as_array_mut) {
            for interface in interfaces.iter_mut().filter_map(Value::as_object_mut) {
                interface.remove("rx_rate_mbs");
                interface.remove("tx_rate_mbs");
            }
        }
    }
    Ok(snapshot)
}

#[tauri::command]
pub(crate) async fn mole_analyze(
    app: AppHandle,
    webview: WebviewWindow,
    scans: State<'_, MoleScan>,
    path: String,
    request_id: String,
) -> Result<Value, String> {
    let (_slot, cancelled) = scans.start(ScanKey::new(webview.label(), request_id)?)?;
    let executable = executable(&app)?;
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    let target = scan_path(&path, &home)?;
    let args = [
        OsStr::new("analyze"),
        OsStr::new("--json"),
        target.as_os_str(),
    ];
    tokio::select! {
        biased;
        _ = cancelled => Err(SCAN_CANCELLED.to_string()),
        result = json_output(&executable, &args, ANALYZE_TIMEOUT, MAX_SCAN_BYTES) => result,
    }
}

#[tauri::command]
pub(crate) fn mole_cancel_analyze(
    webview: WebviewWindow,
    scans: State<'_, MoleScan>,
    request_id: String,
) -> Result<(), String> {
    scans.cancel(ScanKey::new(webview.label(), request_id)?);
    Ok(())
}

#[tauri::command]
pub(crate) async fn mole_list_apps(app: AppHandle) -> Result<Value, String> {
    let executable = executable(&app)?;
    let help = run_output(
        &executable,
        &["uninstall", "--help"],
        Duration::from_secs(5),
        MAX_ERROR_BYTES,
    )
    .await?;
    // 某些旧版卸载器把未知参数当作应用名。先确认支持只读列表参数。
    if !String::from_utf8_lossy(&help)
        .lines()
        .any(|line| line.split_whitespace().next() == Some("--list"))
    {
        return Err("当前 Mole 版本不支持读取应用列表，请更新 Mole 后重试。".to_string());
    }
    // Mole 根据 stdout 是否连接终端选择输出格式。管道输出为 JSON。
    json_output(
        &executable,
        &["uninstall", "--list"],
        Duration::from_secs(30),
        4 * MIB,
    )
    .await
}

#[tauri::command]
pub(crate) async fn mole_history(app: AppHandle) -> Result<Value, String> {
    json_output(
        &executable(&app)?,
        &["history", "--json"],
        Duration::from_secs(12),
        4 * MIB,
    )
    .await
}

pub(crate) async fn status_snapshot(app: &AppHandle, timeout: Duration) -> Result<Value, String> {
    json_output(
        &executable(app)?,
        &["status", "--json"],
        timeout,
        MAX_STATUS_BYTES,
    )
    .await
}

fn scan_path(input: &str, home: &Path) -> Result<PathBuf, String> {
    if input == "~" {
        return Ok(home.to_owned());
    }
    if let Some(relative) = input.strip_prefix("~/") {
        return Ok(home.join(relative.trim_start_matches('/')));
    }
    let path = Path::new(input);
    if !path.is_absolute() {
        return Err("请输入绝对目录路径，或使用 ~/ 表示个人目录。".to_string());
    }
    Ok(path.to_owned())
}

fn version_from_output(stdout: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(stdout).ok()?;
    text.lines().find_map(|line| {
        let line = line.trim().strip_prefix("Mole ")?;
        line.strip_prefix("version ")
            .unwrap_or(line)
            .split_whitespace()
            .next()
            .filter(|token| {
                token
                    .trim_start_matches(['v', 'V'])
                    .starts_with(|c: char| c.is_ascii_digit())
            })
            .map(str::to_owned)
    })
}

async fn json_output(
    executable: &Path,
    args: &[impl AsRef<OsStr>],
    timeout: Duration,
    limit: usize,
) -> Result<Value, String> {
    parse_json(&run_output(executable, args, timeout, limit).await?)
}

fn parse_json(stdout: &[u8]) -> Result<Value, String> {
    serde_json::from_slice(stdout).map_err(|e| format!("Mole 返回了无法识别的 JSON：{e}"))
}

// JSON 快照的磁盘读写速率会由冷启动的计数差得到 0。连续取三帧，先让
// Mole 收集完整指标，再等一个间隔读出真实的速率。Mole 会随帧携带上一帧的慢指标。
async fn warm_status(executable: &Path) -> Result<Option<Value>, String> {
    let mut process = MoleChild::spawn(
        executable,
        &["status", "--watch", "--interval", "2s"],
        Stdio::null(),
    )?;
    let result = time::timeout(STATUS_TIMEOUT, async {
        let stdout = process.child.stdout.take().ok_or("Mole 输出流不可用。")?;
        let mut reader = BufReader::new(stdout.take(MAX_STATUS_BYTES as u64 + 1));
        let mut line = Vec::new();
        let mut snapshot = None;
        for _ in 0..3 {
            line.clear();
            if reader
                .read_until(b'\n', &mut line)
                .await
                .map_err(io_error)?
                == 0
            {
                return Ok(None);
            }
            if reader.get_ref().limit() == 0 {
                return Err("Mole 状态数据超过大小限制。".to_string());
            }
            match serde_json::from_slice::<Value>(&line) {
                Ok(record) if record.is_object() => snapshot = Some(record),
                _ => return Ok(None),
            }
        }
        Ok(snapshot)
    })
    .await
    .map_err(|_| timeout_error(STATUS_TIMEOUT));
    process.stop().await;
    result?
}

async fn run_output(
    executable: &Path,
    args: &[impl AsRef<OsStr>],
    timeout: Duration,
    limit: usize,
) -> Result<Vec<u8>, String> {
    let mut process = MoleChild::spawn(executable, args, Stdio::piped())?;
    let result = time::timeout(timeout, async {
        let stdout = process.child.stdout.take().ok_or("Mole 输出流不可用。")?;
        let stderr = process.child.stderr.take().ok_or("Mole 错误流不可用。")?;
        let (stdout, warnings) = tokio::try_join!(
            read_capped(stdout, limit),
            read_capped(stderr, MAX_ERROR_BYTES),
        )?;
        let status = process.wait().await?;
        if status.success() {
            Ok(stdout)
        } else {
            let detail = String::from_utf8_lossy(&warnings);
            match detail.lines().map(str::trim).find(|line| !line.is_empty()) {
                Some(line) => Err(format!("Mole 执行失败：{line}")),
                None => Err(format!("Mole 执行失败（{status}）。")),
            }
        }
    })
    .await
    .map_err(|_| timeout_error(timeout));
    if !matches!(&result, Ok(Ok(_))) {
        process.stop().await;
    }
    result?
}

async fn read_capped(input: impl AsyncRead + Unpin, maximum: usize) -> Result<Vec<u8>, String> {
    let mut buffer = Vec::new();
    input
        .take(maximum as u64 + 1)
        .read_to_end(&mut buffer)
        .await
        .map_err(io_error)?;
    if buffer.len() > maximum {
        return Err("Mole 输出超过大小限制。请缩小扫描目录后重试。".to_string());
    }
    Ok(buffer)
}

fn io_error(error: io::Error) -> String {
    format!("无法读取 Mole：{error}")
}

fn timeout_error(timeout: Duration) -> String {
    format!("Mole 超过 {} 秒未返回。请稍后重试。", timeout.as_secs())
}

struct MoleChild {
    child: Child,
    group: Option<libc::pid_t>,
}

impl MoleChild {
    fn spawn(executable: &Path, args: &[impl AsRef<OsStr>], stderr: Stdio) -> Result<Self, String> {
        let child = Command::new(executable)
            .args(args)
            .env("PATH", command_path(executable))
            .env_remove("BASH_ENV")
            .env_remove("ENV")
            // Mole 的分析器优先读该变量。使用应用中显式选择的目录。
            .env_remove("MO_ANALYZE_PATH")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(stderr)
            .process_group(0)
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("无法启动 Mole：{e}"))?;
        let group = child.id().and_then(|id| libc::pid_t::try_from(id).ok());
        Ok(Self { child, group })
    }

    async fn wait(&mut self) -> Result<std::process::ExitStatus, String> {
        let status = self.child.wait().await.map_err(io_error)?;
        self.group = None;
        Ok(status)
    }

    async fn stop(&mut self) {
        if self.group.is_some() {
            self.signal_group();
            let _ = time::timeout(Duration::from_secs(1), self.wait()).await;
        }
    }

    fn signal_group(&self) {
        if let Some(group) = self.group {
            // 子命令启动时创建专用进程组，结束它时一并停止其采集子进程。
            unsafe { libc::killpg(group, libc::SIGKILL) };
        }
    }
}

impl Drop for MoleChild {
    fn drop(&mut self) {
        self.signal_group();
    }
}

#[cfg(test)]
mod tests {
    use super::{scan_path, MoleScan, ScanKey, SCAN_CANCELLED};
    use std::path::Path;

    #[test]
    fn scan_arguments_cannot_be_parsed_as_command_flags() {
        let home = Path::new("/Users/test");
        assert!(scan_path("--help", home).is_err());
        assert_eq!(
            scan_path("~//My Folder ", home).unwrap(),
            home.join("My Folder ")
        );
        assert_eq!(
            scan_path("/My Folder ", home).unwrap(),
            Path::new("/My Folder ")
        );
    }

    #[test]
    fn a_cancellation_can_arrive_before_the_scan_starts() {
        let scans = MoleScan::default();
        let key = || ScanKey::new("main", "cancel-before-start".to_string()).unwrap();
        scans.cancel(key());
        assert!(matches!(scans.start(key()), Err(error) if error == SCAN_CANCELLED));
    }

    #[test]
    fn cancellation_is_scoped_to_the_starting_webview_and_request() {
        let scans = MoleScan::default();
        let key =
            |webview, request_id: &str| ScanKey::new(webview, request_id.to_string()).unwrap();
        let (slot, mut cancelled) = scans.start(key("main", "first-scan")).unwrap();
        scans.cancel(key("other", "first-scan"));
        scans.cancel(key("main", "other-scan"));
        assert!(matches!(
            cancelled.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        scans.cancel(key("main", "first-scan"));
        assert_eq!(cancelled.try_recv(), Ok(()));
        assert!(scans.start(key("main", "next-scan")).is_err());
        drop(slot);
        assert!(scans.start(key("main", "next-scan")).is_ok());
    }
}

use std::ffi::{OsStr, OsString};
use std::path::Path;

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// 占用某端口的一个进程（前端据此渲染确认弹窗，无需知道平台细节）。
#[derive(serde::Serialize)]
pub struct PortProcess {
    /// 进程 ID。
    pub pid: u32,
    /// 进程名（如 "node.exe" / "node"）。
    pub name: String,
    /// 协议（"TCP" / "UDP"）。
    pub proto: String,
    /// 本地监听地址（如 "0.0.0.0:1420" / "[::]:1420" / "*:1420"）。
    pub local_addr: String,
    /// 可执行文件路径。受系统权限限制时可能拿不到，因此保持可选。
    pub executable_path: Option<String>,
    /// 完整命令行。sysinfo 返回 argv 切片，这里重组为可展示的单行字符串。
    pub command_line: Option<String>,
    /// 当前工作目录。受系统权限限制时可能为空。
    pub cwd: Option<String>,
    /// 父进程 ID。拿不到时返回 None，而不是让端口检测失败。
    pub parent_pid: Option<u32>,
}

/// 一个 shell 启动配置文件的描述（前端据此渲染，无需知道平台细节）。
#[derive(serde::Serialize)]
pub struct ShellConfig {
    /// 稳定标识，作命令参数（如 "zshrc" / "powershell" / "cmd"）。
    pub name: String,
    /// 展示名（如 ".zshrc" / "PowerShell" / "命令提示符 (cmd)"）。
    pub label: String,
    /// 完整路径（展示用）。
    pub path: String,
    pub exists: bool,
    /// 重载提示命令（如 "source ~/.zshrc" / ". $PROFILE"）。
    pub reload_hint: String,
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use self::macos as sys;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use self::windows as sys;

// 直接由命令调用的“平台专属”入口，原样再导出。
pub use sys::{
    check_port, dns_flush_granted, flush_dns, grant_dns_flush_access, grant_hosts_access,
    hosts_writable, kill_pid, shell_configs,
};

/// 用 sysinfo 按 PID 尽力补齐进程元信息；受保护/系统进程拿不到字段时保持 None，
/// 避免把「可见性不足」升级成 check_port 失败。
pub(crate) fn enrich_port_processes(processes: &mut [PortProcess]) {
    if processes.is_empty() {
        return;
    }

    let pids: Vec<Pid> = processes.iter().map(|p| Pid::from_u32(p.pid)).collect();
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&pids),
        false,
        ProcessRefreshKind::nothing()
            .with_cwd(UpdateKind::OnlyIfNotSet)
            .with_cmd(UpdateKind::OnlyIfNotSet)
            .with_exe(UpdateKind::OnlyIfNotSet),
    );

    for process in processes {
        let Some(info) = system.process(Pid::from_u32(process.pid)) else {
            continue;
        };
        process.executable_path = non_empty_path(info.exe());
        process.command_line = join_command_line(info.cmd());
        process.cwd = non_empty_path(info.cwd());
        process.parent_pid = info
            .parent()
            .map(|pid| pid.as_u32())
            .filter(|pid| *pid != 0);
    }
}

fn non_empty_path(path: Option<&Path>) -> Option<String> {
    path.and_then(|value| non_empty_os_str(value.as_os_str()))
}

fn non_empty_os_str(value: &OsStr) -> Option<String> {
    let text = value.to_string_lossy().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn join_command_line(args: &[OsString]) -> Option<String> {
    if args.is_empty() {
        return None;
    }
    let parts: Vec<String> = args
        .iter()
        .filter_map(|arg| non_empty_os_str(arg.as_os_str()).map(|text| quote_command_arg(&text)))
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

#[cfg(target_os = "windows")]
fn quote_command_arg(arg: &str) -> String {
    if arg.is_empty() {
        return "\"\"".into();
    }
    if !arg.chars().any(|ch| ch.is_whitespace() || ch == '"') {
        return arg.to_string();
    }

    let mut out = String::from("\"");
    let mut backslashes = 0usize;
    for ch in arg.chars() {
        match ch {
            '\\' => backslashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(backslashes));
                backslashes = 0;
                out.push(ch);
            }
        }
    }
    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

#[cfg(not(target_os = "windows"))]
fn quote_command_arg(arg: &str) -> String {
    if arg.is_empty() {
        return "''".into();
    }
    let simple = arg.bytes().all(|b| {
        matches!(
            b,
            b'a'..=b'z'
                | b'A'..=b'Z'
                | b'0'..=b'9'
                | b'/'
                | b'.'
                | b'_'
                | b'-'
                | b':'
                | b'@'
                | b'+'
                | b','
                | b'='
        )
    });
    if simple {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enriches_current_process_without_affecting_port_identity() {
        let mut processes = vec![PortProcess {
            pid: std::process::id(),
            name: "self".into(),
            proto: "TCP".into(),
            local_addr: "127.0.0.1:8080".into(),
            executable_path: None,
            command_line: None,
            cwd: None,
            parent_pid: None,
        }];

        enrich_port_processes(&mut processes);

        let process = &processes[0];
        assert_eq!(process.name, "self");
        assert_eq!(process.proto, "TCP");
        assert_eq!(process.local_addr, "127.0.0.1:8080");
        assert!(process.executable_path.as_deref().is_some_and(|v| !v.is_empty()));
        assert!(process.command_line.as_deref().is_some_and(|v| !v.is_empty()));
        assert!(process.cwd.as_deref().is_some_and(|v| !v.is_empty()));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn quotes_windows_command_arguments_for_display() {
        assert_eq!(quote_command_arg(r"C:\Program Files\node.exe"), r#""C:\Program Files\node.exe""#);
        assert_eq!(quote_command_arg(r#"say "hello""#), r#""say \"hello\"""#);
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn quotes_unix_command_arguments_for_display() {
        assert_eq!(quote_command_arg("/usr/bin/node"), "/usr/bin/node");
        assert_eq!(quote_command_arg("hello world"), "'hello world'");
        assert_eq!(quote_command_arg("it's"), "'it'\\''s'");
    }
}

/// 当前平台的 hosts 路径（字符串，供前端文案）。
pub fn hosts_path_string() -> String {
    sys::hosts_path().to_string_lossy().to_string()
}

/// 读取 hosts（全平台可读，无需提权）。
pub fn read_hosts() -> Result<String, String> {
    std::fs::read_to_string(sys::hosts_path()).map_err(|e| e.to_string())
}

/// 写 hosts：无写权限返回 NO_PERMISSION；写前备份到家目录。
pub fn write_hosts(content: String) -> Result<(), String> {
    if !sys::hosts_writable() {
        return Err("NO_PERMISSION".into());
    }
    if let (Ok(current), Some(bak)) = (
        std::fs::read_to_string(sys::hosts_path()),
        sys::backup_path(".hostguard.hosts.bak"),
    ) {
        let _ = std::fs::write(bak, current);
    }
    std::fs::write(sys::hosts_path(), content).map_err(|e| e.to_string())
}

/// 读 shell 配置；不存在返回空串。
pub fn read_shell_config(name: String) -> Result<String, String> {
    let path = sys::shell_path(&name).ok_or("无效的配置文件")?;
    if !path.exists() {
        return Ok(String::new());
    }
    std::fs::read_to_string(path).map_err(|e| e.to_string())
}

/// 写 shell 配置：先建父目录（PowerShell profile 目录可能不存在），写前备份，
/// 写后调用平台钩子（Windows+cmd 设置注册表 AutoRun；其余 noop）。
pub fn write_shell_config(name: String, content: String) -> Result<(), String> {
    let path = sys::shell_path(&name).ok_or("无效的配置文件")?;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let (Ok(current), Some(bak)) = (
        std::fs::read_to_string(&path),
        sys::backup_path(&format!(".hostguard.{name}.bak")),
    ) {
        let _ = std::fs::write(bak, current);
    }
    std::fs::write(&path, content).map_err(|e| e.to_string())?;
    sys::on_shell_saved(&name, &path)
}

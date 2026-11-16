use std::{
    io::{self, Read, Write},
    sync::{Arc, Mutex},
    time::Duration,
};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
use tauri::{ipc::Channel, AppHandle, Manager};

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MoleAction {
    Clean,
    Uninstall,
    Optimize,
    Purge,
    Installer,
    Slim,
}

impl MoleAction {
    fn arguments(self, dry_run: bool) -> Vec<&'static str> {
        let mut args = vec![match self {
            Self::Clean => "clean",
            Self::Uninstall => "uninstall",
            Self::Optimize => "optimize",
            Self::Purge => "purge",
            Self::Installer => "installer",
            Self::Slim => "slim",
        }];
        if dry_run {
            args.push("--dry-run");
        }
        args
    }
}

#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum TerminalEvent {
    Data { data: Vec<u8> },
    Exit { code: Option<u32> },
}

#[derive(Default)]
pub(crate) struct MoleTerminal(Mutex<TerminalState>);

#[derive(Default)]
struct TerminalState {
    next_id: u32,
    active: Option<Session>,
}

type Writer = Arc<Mutex<Box<dyn Write + Send>>>;

struct Session {
    id: u32,
    group: libc::pid_t,
    master: Box<dyn MasterPty + Send>,
    writer: Writer,
    stopping: bool,
}

impl Session {
    fn signal(&self, signal: libc::c_int) -> io::Result<()> {
        // portable-pty 在 exec 前建立独立会话，子进程 PID 也是进程组 ID。
        // 发送信号和回收子进程共用状态锁，避免向复用的 PID 发送信号。
        if unsafe { libc::kill(-self.group, signal) } == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        Err(error)
    }
}

impl MoleTerminal {
    fn state(&self) -> std::sync::MutexGuard<'_, TerminalState> {
        self.0.lock().unwrap_or_else(|error| error.into_inner())
    }

    pub(crate) fn stop_on_exit(&self) {
        if let Some(session) = &self.state().active {
            let _ = session.signal(libc::SIGTERM);
        }
    }
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows: rows.clamp(6, 150),
        cols: cols.clamp(20, 300),
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn start(
    app: &AppHandle,
    action: MoleAction,
    dry_run: bool,
    size: PtySize,
    on_event: Channel<TerminalEvent>,
) -> Result<u32, String> {
    let executable = super::executable(app)?;
    let home = app.path().home_dir().map_err(|error| error.to_string())?;
    let terminal = app.state::<MoleTerminal>();
    let mut state = terminal.state();
    if state.active.is_some() {
        return Err("已有一项 Mole 操作正在运行。".into());
    }

    let pair = native_pty_system()
        .openpty(size)
        .map_err(|e| e.to_string())?;
    let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    let mut command = CommandBuilder::new(&executable);
    command.args(action.arguments(dry_run));
    command.cwd(&home);
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    command.env_remove("BASH_ENV");
    command.env_remove("ENV");

    command.env("PATH", super::command_path(&executable));

    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(|e| e.to_string())?;
    let Some(group) = child
        .process_id()
        .and_then(|pid| libc::pid_t::try_from(pid).ok())
        .filter(|pid| *pid > 1)
    else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("无法取得 Mole 进程标识。".into());
    };
    drop(pair.slave);
    state.next_id = state.next_id.wrapping_add(1).max(1);
    let id = state.next_id;
    state.active = Some(Session {
        id,
        group,
        master: pair.master,
        writer: Arc::new(Mutex::new(writer)),
        stopping: false,
    });

    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        stream_output(&handle, id, reader, child, on_event)
    });
    Ok(id)
}

fn stream_output(
    app: &AppHandle,
    id: u32,
    mut reader: Box<dyn Read + Send>,
    mut child: Box<dyn Child + Send + Sync>,
    on_event: Channel<TerminalEvent>,
) {
    let mut buffer = [0; 4096];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                if on_event
                    .send(TerminalEvent::Data {
                        data: buffer[..count].to_vec(),
                    })
                    .is_err()
                {
                    let _ = stop_session(app, id);
                    break;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    drop(reader);

    let terminal = app.state::<MoleTerminal>();
    // 已读完 PTY 后回收子进程。在锁外等待，避免阻塞后续中止信号。
    let code = loop {
        {
            let mut state = terminal.state();
            match child.try_wait() {
                Ok(None) => {}
                result => {
                    state.active = None;
                    break result.ok().flatten().map(|status| status.exit_code());
                }
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    let _ = on_event.send(TerminalEvent::Exit { code });
}

fn stop_session(app: &AppHandle, id: u32) -> Result<(), String> {
    {
        let terminal = app.state::<MoleTerminal>();
        let mut state = terminal.state();
        let Some(session) = state.active.as_mut().filter(|session| session.id == id) else {
            return Ok(());
        };
        if session.stopping {
            return Ok(());
        }
        session.signal(libc::SIGTERM).map_err(|e| e.to_string())?;
        session.stopping = true;
    }

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        // 让 Mole 的信号处理先执行，退出未完成时再回收进程组。
        tokio::time::sleep(Duration::from_secs(8)).await;
        let terminal = handle.state::<MoleTerminal>();
        let state = terminal.state();
        if let Some(session) = state.active.as_ref().filter(|session| session.id == id) {
            let _ = session.signal(libc::SIGKILL);
        }
    });
    Ok(())
}

#[tauri::command]
pub(crate) async fn mole_terminal_start(
    app: AppHandle,
    action: MoleAction,
    dry_run: bool,
    cols: u16,
    rows: u16,
    on_event: Channel<TerminalEvent>,
) -> Result<u32, String> {
    tauri::async_runtime::spawn_blocking(move || {
        start(&app, action, dry_run, size(cols, rows), on_event)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn mole_terminal_write(
    app: AppHandle,
    id: u32,
    data: String,
) -> Result<(), String> {
    if data.len() > 4096 {
        return Err("终端输入过长。".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let terminal = app.state::<MoleTerminal>();
        let writer = terminal
            .state()
            .active
            .as_ref()
            .filter(|session| session.id == id && !session.stopping)
            .map(|session| Arc::clone(&session.writer))
            .ok_or("Mole 终端已退出。")?;
        let mut writer = writer.lock().map_err(|e| e.to_string())?;
        writer.write_all(data.as_bytes()).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) fn mole_terminal_resize(
    terminal: tauri::State<'_, MoleTerminal>,
    id: u32,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let state = terminal.state();
    if let Some(session) = state.active.as_ref().filter(|session| session.id == id) {
        session
            .master
            .resize(size(cols, rows))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn mole_terminal_stop(app: AppHandle, id: u32) -> Result<(), String> {
    stop_session(&app, id)
}

#[cfg(test)]
mod tests {
    use super::MoleAction;

    #[test]
    fn preview_only_passes_the_official_read_only_flag() {
        let actions = [
            (MoleAction::Clean, "clean"),
            (MoleAction::Uninstall, "uninstall"),
            (MoleAction::Optimize, "optimize"),
            (MoleAction::Purge, "purge"),
            (MoleAction::Installer, "installer"),
            (MoleAction::Slim, "slim"),
        ];
        for (action, subcommand) in actions {
            assert_eq!(action.arguments(true), [subcommand, "--dry-run"]);
            assert_eq!(action.arguments(false), [subcommand]);
        }
    }
}

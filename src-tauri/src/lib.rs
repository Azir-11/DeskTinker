mod acme;
#[cfg(target_os = "macos")]
mod menu_bar;
#[cfg(target_os = "macos")]
mod mole;
#[cfg(target_os = "macos")]
mod notifications;
mod platform;
mod telemetry;

use crate::acme::{acme_check_dns, acme_finalize_order, acme_list_certificates, acme_start_order};
use crate::telemetry::Telemetry;

// ───────────────────────── Hosts ─────────────────────────
#[tauri::command]
fn hosts_writable() -> bool {
    platform::hosts_writable()
}

#[tauri::command]
fn hosts_path() -> String {
    platform::hosts_path_string()
}

#[tauri::command]
fn read_hosts() -> Result<String, String> {
    platform::read_hosts()
}

#[tauri::command]
async fn grant_hosts_access() -> Result<(), String> {
    platform::grant_hosts_access().await
}

#[tauri::command]
fn write_hosts(content: String) -> Result<(), String> {
    platform::write_hosts(content)
}

// ───────────────────────── Shell ─────────────────────────
#[tauri::command]
fn list_shell_configs() -> Vec<platform::ShellConfig> {
    platform::shell_configs()
}

#[tauri::command]
fn read_shell_config(name: String) -> Result<String, String> {
    platform::read_shell_config(name)
}

#[tauri::command]
fn write_shell_config(name: String, content: String) -> Result<(), String> {
    platform::write_shell_config(name, content)
}

// ───────────────────────── Port (杀端口工具) ─────────────────────────
#[tauri::command]
async fn check_port(port: u16) -> Result<Vec<platform::PortProcess>, String> {
    platform::check_port(port).await
}

#[tauri::command]
async fn kill_process(pid: u32) -> Result<(), String> {
    platform::kill_pid(pid).await
}

// ───────────────────────── DNS cache flush ─────────────────────────
#[tauri::command]
async fn dns_flush_granted() -> bool {
    platform::dns_flush_granted().await
}

#[tauri::command]
async fn grant_dns_flush_access() -> Result<(), String> {
    platform::grant_dns_flush_access().await
}

#[tauri::command]
async fn flush_dns_cache() -> Result<(), String> {
    platform::flush_dns().await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(Telemetry::new())
        .manage(acme::AcmeState::default())
        .invoke_handler(tauri::generate_handler![
            hosts_writable,
            hosts_path,
            read_hosts,
            grant_hosts_access,
            write_hosts,
            check_port,
            kill_process,
            list_shell_configs,
            read_shell_config,
            write_shell_config,
            dns_flush_granted,
            grant_dns_flush_access,
            flush_dns_cache,
            telemetry::read_system_stats,
            telemetry::read_cpu_history,
            telemetry::read_cpu_history_retention,
            telemetry::set_cpu_history_retention,
            telemetry::read_process_stats,
            telemetry::read_host_info,
            telemetry::read_disks,
            telemetry::read_cpu_temp,
            acme_start_order,
            acme_check_dns,
            acme_finalize_order,
            acme_list_certificates,
            #[cfg(target_os = "macos")]
            mole::read::mole_info,
            #[cfg(target_os = "macos")]
            mole::read::mole_read_status,
            #[cfg(target_os = "macos")]
            mole::read::mole_analyze,
            #[cfg(target_os = "macos")]
            mole::read::mole_cancel_analyze,
            #[cfg(target_os = "macos")]
            mole::read::mole_list_apps,
            #[cfg(target_os = "macos")]
            mole::read::mole_history,
            #[cfg(target_os = "macos")]
            mole::terminal::mole_terminal_start,
            #[cfg(target_os = "macos")]
            mole::terminal::mole_terminal_write,
            #[cfg(target_os = "macos")]
            mole::terminal::mole_terminal_resize,
            #[cfg(target_os = "macos")]
            mole::terminal::mole_terminal_stop,
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            if let Err(error) = menu_bar::setup(app) {
                eprintln!("无法初始化 macOS 菜单栏：{error}");
            }
            telemetry::start_background_sampling(app.handle());
            Ok(())
        });

    #[cfg(target_os = "macos")]
    let builder = builder
        .manage(mole::read::MoleScan::default())
        .manage(mole::terminal::MoleTerminal::default())
        .on_window_event(menu_bar::on_window_event);

    builder
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                telemetry::flush_cpu_history_on_exit(app);
                #[cfg(target_os = "macos")]
                {
                    use tauri::Manager;
                    app.state::<mole::terminal::MoleTerminal>().stop_on_exit();
                }
            }
            #[cfg(target_os = "macos")]
            menu_bar::on_app_event(app, &event);
        });
}

use std::sync::Mutex;

use tauri::{
    image::Image,
    menu::{Menu, MenuBuilder, MenuEvent, MenuItem},
    tray::TrayIconBuilder,
    ActivationPolicy, App, AppHandle, Manager, RunEvent, Window, WindowEvent,
};

const TRAY_ID: &str = "system-monitor";
const MAIN_WINDOW: &str = "main";
const CLOSE_WINDOW: &str = "app-close-window";
const OPEN_HOSTGUARD: &str = "menu-bar-open-hostguard";
const OPEN_MONITOR: &str = "menu-bar-open-monitor";
const QUIT: &str = "menu-bar-quit";
const NO_CPU_ALERT: &str = "暂无 CPU 异常占用";

struct DisplayedTitle(Mutex<String>);

struct CpuAlertMenu {
    item: MenuItem<tauri::Wry>,
    displayed: Mutex<Option<String>>,
}

pub fn app_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::default(app)?;
    let close_title = "Close Window";
    let close = MenuItem::with_id(app, CLOSE_WINDOW, close_title, true, Some("Cmd+W"))?;
    // 无边框窗口没有原生关闭按钮，默认的 performClose: 无法触发关闭事件。
    for submenu in menu.items()?.iter().filter_map(|item| item.as_submenu()) {
        for (index, item) in submenu.items()?.iter().enumerate() {
            let Some(predefined) = item.as_predefined_menuitem() else {
                continue;
            };
            if predefined.text()? == close_title {
                submenu.remove(predefined)?;
                submenu.insert(&close, index)?;
            }
        }
    }
    Ok(menu)
}

pub fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    if event.id().as_ref() != CLOSE_WINDOW {
        return;
    }
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.close();
    }
}

pub fn setup(app: &App) -> tauri::Result<()> {
    let alert = MenuItem::with_id(app, "menu-bar-cpu-alert", NO_CPU_ALERT, false, None::<&str>)?;
    let menu = MenuBuilder::new(app)
        .item(&alert)
        .text(OPEN_HOSTGUARD, "打开 HostGuard")
        .text(OPEN_MONITOR, "打开系统监控")
        .separator()
        .text(QUIT, "退出 HostGuard")
        .build()?;
    let title = format_title(None, None, None);
    TrayIconBuilder::with_id(TRAY_ID)
        .title(&title)
        .tooltip("HostGuard · CPU 占用 · 可用内存 · CPU 温度")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            OPEN_HOSTGUARD => show_main_window(app, false),
            OPEN_MONITOR => show_main_window(app, true),
            QUIT => app.exit(0),
            _ => {}
        })
        .build(app)?;
    app.manage(DisplayedTitle(Mutex::new(title)));
    app.manage(CpuAlertMenu {
        item: alert,
        displayed: Mutex::new(None),
    });
    Ok(())
}

pub fn update(app: &AppHandle, cpu_usage: Option<f32>, mem_available: u64, cpu_temp: Option<f32>) {
    let current = format_title(cpu_usage, Some(mem_available), cpu_temp);
    let handle = app.clone();
    // 菜单跟踪期间主线程可能延后处理 UI 事件，采样线程不等待菜单栏重绘。
    let _ = app.run_on_main_thread(move || {
        let (Some(displayed), Some(tray)) = (
            handle.try_state::<DisplayedTitle>(),
            handle.tray_by_id(TRAY_ID),
        ) else {
            return;
        };
        let Ok(mut previous) = displayed.0.lock() else {
            return;
        };
        if *previous != current && tray.set_title(Some(&current)).is_ok() {
            *previous = current;
        }
    });
}

pub fn update_cpu_alert(app: &AppHandle, current: Option<String>, notification: Option<String>) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(alert) = handle.try_state::<CpuAlertMenu>() {
            let mut displayed = alert.displayed.lock().unwrap_or_else(|e| e.into_inner());
            if *displayed != current {
                if displayed.is_some() != current.is_some() {
                    if let Some(tray) = handle.tray_by_id(TRAY_ID) {
                        let _ = tray.set_icon(current.as_ref().map(|_| red_dot()));
                    }
                }
                if alert
                    .item
                    .set_text(current.as_deref().unwrap_or(NO_CPU_ALERT))
                    .is_ok()
                {
                    *displayed = current;
                }
            }
        }

        if let Some(message) = notification {
            crate::notifications::show_cpu_alert(message);
        }
    });
}

fn red_dot() -> Image<'static> {
    // 托盘在 macOS 把图标缩放到 18 pt 高。透明留白把圆点保留在约 7 pt。
    const WIDTH: u32 = 18;
    const HEIGHT: u32 = 36;
    let mut rgba = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let dx = x as f32 + 0.5 - WIDTH as f32 / 2.0;
            let dy = y as f32 + 0.5 - HEIGHT as f32 / 2.0;
            let coverage = (7.5 - dx.hypot(dy)).clamp(0.0, 1.0);
            rgba.extend_from_slice(&[255, 69, 58, (coverage * 255.0).round() as u8]);
        }
    }
    Image::new_owned(rgba, WIDTH, HEIGHT)
}

pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        let app = window.app_handle();
        // 菜单栏入口就绪后，关闭时隐藏窗口和程序坞图标，应用继续运行。
        if window.label() == MAIN_WINDOW
            && app.tray_by_id(TRAY_ID).is_some()
            && window.hide().is_ok()
        {
            api.prevent_close();
            let _ = app.set_activation_policy(ActivationPolicy::Accessory);
        }
    }
}

pub fn on_app_event(app: &AppHandle, event: &RunEvent) {
    if matches!(event, RunEvent::Reopen { .. }) {
        show_main_window(app, false);
    }
}

fn show_main_window(app: &AppHandle, open_monitor: bool) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };
    let _ = app.set_activation_policy(ActivationPolicy::Regular);
    let _ = app.show();
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
    if open_monitor {
        let _ = window.eval("location.hash = '/monitor'");
    }
}

fn format_title(
    cpu_usage: Option<f32>,
    mem_available: Option<u64>,
    cpu_temp: Option<f32>,
) -> String {
    let cpu = cpu_usage
        .filter(|value| value.is_finite())
        .map_or_else(|| "—".to_string(), |value| format!("{value:.0}%"));
    let memory = mem_available.map_or_else(
        || "—".to_string(),
        |bytes| format!("{:.1} GiB", bytes as f64 / (1u64 << 30) as f64),
    );
    let temperature = cpu_temp
        .filter(|value| value.is_finite())
        .map_or_else(|| "— °C".to_string(), |value| format!("{value:.0} °C"));
    format!("CPU {cpu}  内存 {memory}  {temperature}")
}

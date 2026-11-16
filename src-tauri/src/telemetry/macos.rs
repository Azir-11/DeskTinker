use std::time::{Duration, Instant};

use serde_json::Value;
use tauri::{AppHandle, Manager};
use tokio::time::{self, MissedTickBehavior};

use super::{
    collect_cpu_temp,
    cpu_alert::{CpuAlert, CpuMonitor},
    valid_temperature, Telemetry,
};

const LIVE_INTERVAL: Duration = Duration::from_secs(2);
const TEMP_INTERVAL: Duration = Duration::from_secs(6);
const PROCESS_INTERVAL: Duration = Duration::from_secs(5);
const MOLE_INTERVAL: Duration = Duration::from_secs(30);
const MOLE_TIMEOUT: Duration = Duration::from_secs(4);

pub(crate) fn start_background_sampling(app: &AppHandle) {
    tauri::async_runtime::spawn(sample_live(app.clone()));
    tauri::async_runtime::spawn(sample_temperature(app.clone()));
    tauri::async_runtime::spawn(sample_processes(app.clone()));
}

async fn sample_processes(app: AppHandle) {
    let mut timer = time::interval(PROCESS_INTERVAL);
    timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut monitor = CpuMonitor::new();

    loop {
        timer.tick().await;
        // 进程采样独立于 CPU / 内存快照，且不阻塞异步运行时。
        let sample = tauri::async_runtime::spawn_blocking(move || {
            let (current, notification) = monitor.sample();
            (monitor, current, notification)
        })
        .await;
        let Ok((next_monitor, current, notification)) = sample else {
            crate::menu_bar::update_cpu_alert(&app, None, None);
            return;
        };
        monitor = next_monitor;
        crate::menu_bar::update_cpu_alert(
            &app,
            current.map(cpu_alert_message),
            notification.map(cpu_alert_message),
        );
    }
}

fn cpu_alert_message(alert: CpuAlert) -> String {
    format!("{} 异常占用 CPU {:.0}%", alert.name, alert.cpu_usage)
}

async fn sample_live(app: AppHandle) {
    // 先等 CPU 累积足够的计数增量，再给菜单栏显示第一次用量。
    time::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL + Duration::from_millis(10)).await;
    let mut timer = time::interval(LIVE_INTERVAL);
    timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let state = app.state::<Telemetry>();
    loop {
        timer.tick().await;
        let (cpu_usage, mem_available) = {
            let Ok(mut sampler) = state.sampler.lock() else {
                return;
            };
            let stats = sampler.sample();
            (stats.cpu_usage, stats.mem_available)
        };
        let temperature = state.temperature.lock().ok().and_then(|temp| *temp);
        crate::menu_bar::update(&app, Some(cpu_usage), mem_available, temperature);
    }
}

async fn sample_temperature(app: AppHandle) {
    let mut timer = time::interval(TEMP_INTERVAL);
    timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let state = app.state::<Telemetry>();
    let mut last_mole_poll: Option<Instant> = None;
    let mut mole_temp = None;

    loop {
        timer.tick().await;
        let native_temp = tauri::async_runtime::spawn_blocking(collect_cpu_temp)
            .await
            .ok()
            .flatten();

        // Mole 快照还会读取进程、磁盘等数据。仅在原生传感器不可用时低频查询，
        // 且独立于 CPU / 内存采样，避免外部命令拖慢菜单栏。
        if native_temp.is_some() {
            mole_temp = None;
        } else if last_mole_poll.is_none_or(|last| last.elapsed() >= MOLE_INTERVAL) {
            last_mole_poll = Some(Instant::now());
            mole_temp = read_mole_temperature(&app).await;
        }

        let Ok(mut temperature) = state.temperature.lock() else {
            return;
        };
        *temperature = native_temp.or(mole_temp);
    }
}

fn parse_mole_temperature(snapshot: &Value) -> Option<f32> {
    valid_temperature(snapshot.get("thermal")?.get("cpu_temp")?.as_f64()? as f32)
}

async fn read_mole_temperature(app: &AppHandle) -> Option<f32> {
    let snapshot = crate::mole::read::status_snapshot(app, MOLE_TIMEOUT)
        .await
        .ok()?;
    parse_mole_temperature(&snapshot)
}

#[cfg(test)]
mod tests {
    use super::parse_mole_temperature;
    use serde_json::json;

    #[test]
    fn mole_status_reads_only_cpu_temperature() {
        assert_eq!(
            parse_mole_temperature(&json!({"thermal":{"cpu_temp":49.2,"gpu_temp":57}})),
            Some(49.2)
        );
        for unavailable in [
            json!({}),
            json!({"thermal":{"gpu_temp":57}}),
            json!({"thermal":{"cpu_temp":0,"gpu_temp":57}}),
            json!({"thermal":{"cpu_temp":500}}),
            json!("Mole not installed"),
        ] {
            assert_eq!(parse_mole_temperature(&unavailable), None);
        }
    }
}

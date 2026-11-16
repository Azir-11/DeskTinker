#[cfg(target_os = "macos")]
mod cpu_alert;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub(crate) use macos::start_background_sampling;

use std::{
    sync::Mutex,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use sysinfo::{Components, System, MINIMUM_CPU_UPDATE_INTERVAL};

pub(crate) struct Telemetry {
    sampler: Mutex<Sampler>,
    #[cfg(target_os = "macos")]
    temperature: Mutex<Option<f32>>,
}

impl Telemetry {
    pub(crate) fn new() -> Self {
        let mut system = System::new();
        system.refresh_cpu_all();
        system.refresh_memory();
        let snapshot = SystemStats::from_system(&system, 0);

        Self {
            sampler: Mutex::new(Sampler {
                system,
                snapshot,
                last_sample: Instant::now(),
            }),
            #[cfg(target_os = "macos")]
            temperature: Mutex::new(None),
        }
    }
}

struct Sampler {
    system: System,
    snapshot: SystemStats,
    last_sample: Instant,
}

impl Sampler {
    fn sample(&mut self) -> &SystemStats {
        if self.last_sample.elapsed() > MINIMUM_CPU_UPDATE_INTERVAL {
            #[cfg(target_os = "macos")]
            self.system.refresh_cpu_usage();
            #[cfg(not(target_os = "macos"))]
            self.system.refresh_cpu_all();
            self.system.refresh_memory();
            self.last_sample = Instant::now();
            let sampled_at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            self.snapshot = SystemStats::from_system(&self.system, sampled_at);
        }

        &self.snapshot
    }
}

#[derive(Clone, serde::Serialize)]
pub(crate) struct SystemStats {
    sampled_at: u64,
    cpu_usage: f32,
    per_core: Vec<f32>,
    cpu_freq: u64, // MHz
    mem_total: u64,
    mem_used: u64,
    mem_available: u64,
    swap_total: u64,
    swap_used: u64,
    load_one: f64,
    load_five: f64,
    load_fifteen: f64,
    uptime: u64,
}

impl SystemStats {
    fn from_system(system: &System, sampled_at: u64) -> Self {
        let load = System::load_average();
        let mem_total = system.total_memory();
        let mem_used = system.used_memory().min(mem_total);

        // sysinfo 0.33 在 macOS 上从可用页数减去已压缩页，可能把可用量
        // 错报为零。已用量已经包含压缩内存，用物理总量减去已用量。
        #[cfg(target_os = "macos")]
        let mem_available = mem_total - mem_used;
        #[cfg(not(target_os = "macos"))]
        let mem_available = system.available_memory().min(mem_total);

        Self {
            sampled_at,
            cpu_usage: system.global_cpu_usage(),
            per_core: system.cpus().iter().map(|c| c.cpu_usage()).collect(),
            cpu_freq: system.cpus().first().map(|c| c.frequency()).unwrap_or(0),
            mem_total,
            mem_used,
            mem_available,
            swap_total: system.total_swap(),
            swap_used: system.used_swap(),
            load_one: load.one,
            load_five: load.five,
            load_fifteen: load.fifteen,
            uptime: System::uptime(),
        }
    }
}

#[tauri::command]
pub(crate) fn read_system_stats(state: tauri::State<'_, Telemetry>) -> Result<SystemStats, String> {
    let sampler = state.sampler.lock().map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    let snapshot = sampler.snapshot.clone();
    #[cfg(not(target_os = "macos"))]
    let snapshot = {
        let mut sampler = sampler;
        sampler.sample().clone()
    };
    Ok(snapshot)
}

/// 静态/缓变的主机信息，前端只在页面挂载时取一次。
#[derive(serde::Serialize)]
pub(crate) struct HostInfo {
    os: String,
    os_version: String,
    kernel_version: String,
    long_os_version: String,
    arch: String,
    host_name: String,
    cpu_brand: String,
    core_count: usize,
    physical_core_count: Option<usize>,
    boot_time: u64,
}

#[tauri::command]
pub(crate) fn read_host_info(state: tauri::State<'_, Telemetry>) -> Result<HostInfo, String> {
    let sampler = state.sampler.lock().map_err(|e| e.to_string())?;
    let sys = &sampler.system;
    Ok(HostInfo {
        os: std::env::consts::OS.to_string(),
        os_version: System::os_version().unwrap_or_default(),
        kernel_version: System::kernel_version().unwrap_or_default(),
        long_os_version: System::long_os_version().unwrap_or_default(),
        arch: std::env::consts::ARCH.to_string(),
        host_name: System::host_name().unwrap_or_default(),
        cpu_brand: sys
            .cpus()
            .first()
            .map(|c| c.brand().trim().to_string())
            .unwrap_or_default(),
        core_count: sys.cpus().len(),
        physical_core_count: sys.physical_core_count(),
        boot_time: System::boot_time(),
    })
}

/// 单个磁盘/分区的容量信息。
#[derive(serde::Serialize)]
pub(crate) struct DiskInfo {
    name: String,
    mount: String,
    fs: String,
    total: u64,
    available: u64,
    used: u64,
}

/// 仅查询卷容量，按需低频刷新。
#[tauri::command]
pub(crate) async fn read_disks() -> Result<Vec<DiskInfo>, String> {
    tauri::async_runtime::spawn_blocking(collect_disks)
        .await
        .map_err(|e| e.to_string())
}

fn collect_disks() -> Vec<DiskInfo> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    #[cfg(target_os = "macos")]
    let has_startup_volume = disks
        .iter()
        .any(|d| d.mount_point() == std::path::Path::new("/") && d.total_space() > 0);

    disks
        .iter()
        .filter(|d| {
            #[cfg(target_os = "macos")]
            // 系统卷与 Data 共享同一 APFS 容器空间，启动盘只显示一次。
            if has_startup_volume && d.mount_point() == std::path::Path::new("/System/Volumes/Data")
            {
                return false;
            }

            d.total_space() > 0
        })
        .map(|d| {
            let total = d.total_space();
            let available = d.available_space().min(total);
            DiskInfo {
                name: d.name().to_string_lossy().into_owned(),
                mount: d.mount_point().to_string_lossy().into_owned(),
                fs: d.file_system().to_string_lossy().into_owned(),
                total,
                available,
                used: total.saturating_sub(available),
            }
        })
        .collect()
}

/// 摄氏温度可能无法读取，仅展示有 CPU 标签且在有效范围内的传感器。
#[cfg(target_os = "macos")]
#[tauri::command]
pub(crate) fn read_cpu_temp(state: tauri::State<'_, Telemetry>) -> Option<f32> {
    *state.temperature.lock().ok()?
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub(crate) async fn read_cpu_temp() -> Option<f32> {
    tauri::async_runtime::spawn_blocking(collect_cpu_temp)
        .await
        .ok()
        .flatten()
}

fn collect_cpu_temp() -> Option<f32> {
    let components = Components::new_with_refreshed_list();
    // 优先芯片/核心传感器（包括 Apple Silicon 的 PMU tdie），Intel 的
    // CPU Proximity 仅作近似值。GPU、电池、SSD 等温度不作为 CPU 温度。
    components
        .iter()
        .filter_map(|c| {
            let label = c.label().to_ascii_lowercase();
            let priority = if label == "cpu proximity" {
                0
            } else if ["cpu", "core", "package", "tdie", "tctl", "acc mtr temp"]
                .iter()
                .any(|part| label.contains(part))
            {
                1
            } else if cfg!(target_os = "windows") && label == "computer" {
                // sysinfo 的 Windows WMI 采集器返回 ACPI 热区温度。
                0
            } else {
                return None;
            };

            let temp = c.temperature().and_then(valid_temperature)?;
            Some((priority, temp))
        })
        .max_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.total_cmp(&b.1)))
        .map(|(_, temp)| temp)
}

fn valid_temperature(temp: f32) -> Option<f32> {
    (temp > 0.0 && temp <= 125.0).then_some(temp)
}

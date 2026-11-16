use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use sysinfo::{
    CpuRefreshKind, Pid, ProcessRefreshKind, ProcessesToUpdate, System, MINIMUM_CPU_UPDATE_INTERVAL,
};

// 两次采样相隔太久时，丢弃这段时间的 CPU 均值。
const MAX_CPU_SAMPLE_GAP: Duration = Duration::from_secs(12);
// sysinfo 0.33 在 macOS 上发现新进程时只记录内存，下一次刷新才建立 CPU 基线。
#[cfg(target_os = "macos")]
const CPU_BASELINE_SAMPLES: u8 = 2;
#[cfg(not(target_os = "macos"))]
const CPU_BASELINE_SAMPLES: u8 = 1;

#[derive(Clone, serde::Serialize)]
pub(crate) struct ProcessStats {
    pid: u32,
    name: String,
    cpu_usage: Option<f32>,
    memory: u64,
}

pub(super) struct ProcessSampler {
    system: System,
    observations: HashMap<(Pid, u64), u8>,
    snapshot: Vec<ProcessStats>,
    last_sample: Option<Instant>,
}

impl Default for ProcessSampler {
    fn default() -> Self {
        let mut system = System::new();
        if cfg!(target_os = "macos") {
            // Windows 在首次刷新进程 CPU 时就会初始化逻辑核心列表。
            system.refresh_cpu_list(CpuRefreshKind::nothing());
        }
        Self {
            system,
            observations: HashMap::new(),
            snapshot: Vec::new(),
            last_sample: None,
        }
    }
}

impl ProcessSampler {
    pub(super) fn sample(&mut self) -> Result<&[ProcessStats], String> {
        let gap = self.last_sample.map(|last| last.elapsed());
        if gap.is_some_and(|elapsed| elapsed <= MINIMUM_CPU_UPDATE_INTERVAL) {
            return Ok(&self.snapshot);
        }

        let refreshed = self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
        if refreshed == 0 {
            self.observations.clear();
            return Err("无法读取系统进程".into());
        }

        let logical_cpus = self.system.cpus().len();
        let sample_cpu =
            logical_cpus > 0 && gap.is_some_and(|elapsed| elapsed <= MAX_CPU_SAMPLE_GAP);
        let mut observations = HashMap::with_capacity(self.system.processes().len());
        self.snapshot = self
            .system
            .processes()
            .values()
            .map(|process| {
                let identity = (process.pid(), process.start_time());
                let seen = self.observations.get(&identity).copied().unwrap_or(0);
                observations.insert(identity, (seen + 1).min(CPU_BASELINE_SAMPLES));

                let usage = process.cpu_usage();
                // 与总览同口径，100% 表示占满全部逻辑核心。
                let cpu_usage = (sample_cpu
                    && seen >= CPU_BASELINE_SAMPLES
                    && process.start_time() != 0
                    && usage.is_finite()
                    && usage >= 0.0)
                    .then(|| (usage / logical_cpus as f32).min(100.0));

                ProcessStats {
                    pid: process.pid().as_u32(),
                    name: process.name().to_string_lossy().into_owned(),
                    cpu_usage,
                    memory: process.memory(),
                }
            })
            .collect();
        self.observations = observations;
        self.last_sample = Some(Instant::now());
        Ok(&self.snapshot)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::ProcessSampler;

    #[test]
    fn lists_current_process_and_waits_for_a_cpu_baseline() {
        let mut sampler = ProcessSampler::default();
        let pid = std::process::id();

        let first_sample = sampler.sample().unwrap();
        let current = first_sample
            .iter()
            .find(|process| process.pid == pid)
            .unwrap();
        assert!(!current.name.is_empty());
        assert!(current.memory >= 4096);
        assert_eq!(current.cpu_usage, None);

        // macOS 的 sysinfo 直到第一次更新已有进程时才记录 CPU 时间基线。
        #[cfg(target_os = "macos")]
        {
            std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL + Duration::from_millis(20));
            let next_sample = sampler.sample().unwrap();
            let current = next_sample
                .iter()
                .find(|process| process.pid == pid)
                .unwrap();
            assert_eq!(current.cpu_usage, None);
        }

        let work_until =
            Instant::now() + sysinfo::MINIMUM_CPU_UPDATE_INTERVAL + Duration::from_millis(20);
        while Instant::now() < work_until {
            std::hint::spin_loop();
        }
        let measured = sampler.sample().unwrap();
        let current = measured.iter().find(|process| process.pid == pid).unwrap();
        assert!(current
            .cpu_usage
            .is_some_and(|cpu| cpu > 0.0 && cpu <= 100.0));
    }
}

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

// 和 macOS 活动监视器一样，100% 表示占满一个逻辑核心。
const HIGH_CPU_PERCENT: f32 = 80.0;
const RECOVERY_CPU_PERCENT: f32 = 60.0;
const SUSTAINED_DURATION: Duration = Duration::from_secs(30);
const MAX_SAMPLE_GAP: Duration = Duration::from_secs(12);

#[derive(Clone, Debug, PartialEq)]
pub(super) struct CpuAlert {
    pub(super) name: String,
    pub(super) cpu_usage: f32,
}

pub(super) struct CpuMonitor {
    system: System,
    tracker: AlertTracker,
}

impl CpuMonitor {
    pub(super) fn new() -> Self {
        Self {
            system: System::new(),
            tracker: AlertTracker::default(),
        }
    }

    /// 返回占用最高的告警应用及本次需要弹出的新告警。
    pub(super) fn sample(&mut self) -> (Option<CpuAlert>, Option<CpuAlert>) {
        let time = SampleTime {
            monotonic: Instant::now(),
            wall: SystemTime::now(),
        };
        let refreshed = self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_cpu()
                .with_exe(UpdateKind::OnlyIfNotSet),
        );
        if refreshed == 0 {
            self.tracker = AlertTracker::default();
            return (None, None);
        }

        let mut usage = HashMap::new();
        for process in self.system.processes().values() {
            let Some(bundle) = process.exe().and_then(application_bundle) else {
                continue;
            };
            let cpu_usage = process.cpu_usage();
            if cpu_usage.is_finite() && cpu_usage >= 0.0 {
                *usage.entry(bundle).or_default() += cpu_usage;
            }
        }
        self.tracker.observe(time, usage)
    }
}

#[derive(Clone, Copy)]
struct SampleTime {
    monotonic: Instant,
    wall: SystemTime,
}

#[derive(Clone, Copy)]
enum Episode {
    HighSince(Instant),
    Alerting,
}

#[derive(Default)]
struct AlertTracker {
    last_sample: Option<SampleTime>,
    episodes: HashMap<PathBuf, Episode>,
}

impl AlertTracker {
    fn observe(
        &mut self,
        time: SampleTime,
        usage: HashMap<&Path, f32>,
    ) -> (Option<CpuAlert>, Option<CpuAlert>) {
        let Some(last) = self.last_sample.replace(time) else {
            // sysinfo 的首批进程读数仅用于建立 CPU 时间基线。
            return (None, None);
        };
        // macOS 休眠期间单调时钟可能暂停，另检查墙钟以丢弃跨休眠的 CPU 均值。
        if time.monotonic.duration_since(last.monotonic) > MAX_SAMPLE_GAP
            || time
                .wall
                .duration_since(last.wall)
                .is_ok_and(|elapsed| elapsed > MAX_SAMPLE_GAP)
        {
            self.episodes.clear();
            return (None, None);
        }

        self.episodes.retain(|bundle, episode| {
            usage
                .get(bundle.as_path())
                .is_some_and(|&cpu| match episode {
                    Episode::HighSince(_) => cpu >= HIGH_CPU_PERCENT,
                    Episode::Alerting => cpu >= RECOVERY_CPU_PERCENT,
                })
        });

        let mut active = None;
        let mut notification = None;
        for (bundle, cpu) in usage {
            match self.episodes.get(bundle).copied() {
                Some(Episode::Alerting) => update_highest(&mut active, bundle, cpu),
                Some(Episode::HighSince(since))
                    if time.monotonic.duration_since(since) >= SUSTAINED_DURATION =>
                {
                    self.episodes.insert(bundle.to_owned(), Episode::Alerting);
                    update_highest(&mut active, bundle, cpu);
                    update_highest(&mut notification, bundle, cpu);
                }
                None if cpu >= HIGH_CPU_PERCENT => {
                    self.episodes
                        .insert(bundle.to_owned(), Episode::HighSince(time.monotonic));
                }
                _ => {}
            }
        }
        (
            active.map(alert_from_bundle),
            notification.map(alert_from_bundle),
        )
    }
}

fn application_bundle(executable: &Path) -> Option<&Path> {
    if !executable.is_absolute() {
        return None;
    }
    // Chromium / Electron 的渲染进程常驻在外层 .app 内部的辅助 .app 中。
    executable
        .ancestors()
        .skip(1)
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
        })
        .last()
}

fn update_highest<'a>(highest: &mut Option<(&'a Path, f32)>, bundle: &'a Path, cpu: f32) {
    if highest.is_none_or(|(current_bundle, current_cpu)| {
        cpu > current_cpu || (cpu == current_cpu && bundle < current_bundle)
    }) {
        *highest = Some((bundle, cpu));
    }
}

fn alert_from_bundle((bundle, cpu_usage): (&Path, f32)) -> CpuAlert {
    CpuAlert {
        name: bundle
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        cpu_usage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BROWSER: &str = "/Applications/Test Browser.app";
    const EDITOR: &str = "/Applications/Test Editor.app";

    fn time_from(start: Instant, secs: u64) -> SampleTime {
        SampleTime {
            monotonic: start + Duration::from_secs(secs),
            wall: SystemTime::UNIX_EPOCH + Duration::from_secs(secs),
        }
    }

    fn readings(apps: &[(&'static str, f32)]) -> HashMap<&'static Path, f32> {
        apps.iter()
            .map(|&(name, percentage)| (Path::new(name), percentage))
            .collect()
    }

    #[test]
    fn alerts_once_after_a_sustained_episode_and_rearms_after_recovery() {
        let mut tracker = AlertTracker::default();
        let start = Instant::now();
        let mut sample =
            |secs, cpu| tracker.observe(time_from(start, secs), readings(&[(BROWSER, cpu)]));

        assert_eq!(sample(0, 145.0), (None, None));
        for secs in (5..35).step_by(5) {
            assert_eq!(sample(secs, 145.0), (None, None));
        }
        let alert = CpuAlert {
            name: "Test Browser".to_owned(),
            cpu_usage: 145.0,
        };
        assert_eq!(
            sample(35, 145.0),
            (Some(alert.clone()), Some(alert.clone()))
        );
        assert_eq!(sample(40, 145.0), (Some(alert), None));
        assert_eq!(sample(45, 72.0).0.unwrap().cpu_usage, 72.0);

        assert_eq!(sample(50, 56.0), (None, None));
        for secs in (55..85).step_by(5) {
            assert_eq!(sample(secs, 140.0), (None, None));
        }
        assert_eq!(sample(85, 140.0).1.unwrap().cpu_usage, 140.0);
    }

    #[test]
    fn a_spike_or_missing_application_does_not_keep_an_episode_alive() {
        let mut tracker = AlertTracker::default();
        let start = Instant::now();
        for secs in (0..30).step_by(5) {
            assert_eq!(
                tracker.observe(time_from(start, secs), readings(&[(BROWSER, 95.0)])),
                (None, None),
            );
        }
        assert_eq!(
            tracker.observe(time_from(start, 30), readings(&[(BROWSER, 75.0)])),
            (None, None),
        );
        for secs in (35..65).step_by(5) {
            assert_eq!(
                tracker.observe(time_from(start, secs), readings(&[(BROWSER, 95.0)])),
                (None, None),
            );
        }
        assert!(tracker
            .observe(time_from(start, 65), readings(&[(BROWSER, 95.0)]))
            .1
            .is_some());
        assert_eq!(
            tracker.observe(time_from(start, 70), readings(&[])),
            (None, None)
        );
        assert_eq!(
            tracker.observe(time_from(start, 75), readings(&[(BROWSER, 95.0)])),
            (None, None),
        );
    }

    #[test]
    fn a_sleep_gap_does_not_extend_an_episode() {
        let mut tracker = AlertTracker::default();
        let start = Instant::now();
        for secs in (0..35).step_by(5) {
            assert_eq!(
                tracker.observe(time_from(start, secs), readings(&[(BROWSER, 95.0)])),
                (None, None),
            );
        }
        let before_sleep = time_from(start, 35);
        let after_sleep = |secs| SampleTime {
            monotonic: before_sleep.monotonic + Duration::from_secs(secs),
            wall: before_sleep.wall + Duration::from_secs(600 + secs),
        };
        for secs in (0..35).step_by(5) {
            assert_eq!(
                tracker.observe(after_sleep(secs), readings(&[(BROWSER, 95.0)])),
                (None, None),
            );
        }
        assert!(tracker
            .observe(after_sleep(35), readings(&[(BROWSER, 95.0)]))
            .1
            .is_some());
    }

    #[test]
    fn simultaneous_episodes_send_one_notification_for_the_highest_usage() {
        let mut tracker = AlertTracker::default();
        let start = Instant::now();
        for secs in (0..35).step_by(5) {
            assert_eq!(
                tracker.observe(
                    time_from(start, secs),
                    readings(&[(BROWSER, 85.0), (EDITOR, 120.0)])
                ),
                (None, None),
            );
        }
        let (active, notification) = tracker.observe(
            time_from(start, 35),
            readings(&[(BROWSER, 85.0), (EDITOR, 120.0)]),
        );
        assert_eq!(active, notification);
        assert_eq!(notification.unwrap().name, "Test Editor");
        let (active, notification) = tracker.observe(
            time_from(start, 40),
            readings(&[(BROWSER, 120.0), (EDITOR, 5.0)]),
        );
        assert_eq!(active.unwrap().name, "Test Browser");
        assert_eq!(notification, None);
    }

    #[test]
    fn helpers_are_attributed_to_their_outer_application() {
        let bundle = Path::new(BROWSER);
        let helper = bundle.join("Contents/Frameworks/Web Helper.app/Contents/MacOS/Web Helper");
        assert_eq!(application_bundle(&helper), Some(bundle));
        assert_eq!(application_bundle(Path::new("/usr/bin/yes")), None);
    }
}

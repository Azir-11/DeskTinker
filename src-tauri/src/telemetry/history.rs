use std::{collections::VecDeque, time::Duration};

pub(super) const LIVE_INTERVAL: Duration = Duration::from_secs(2);
pub(super) const CHART_DURATION: Duration = Duration::from_secs(6 * 60 * 60);
pub(super) const MAX_SAMPLE_GAP: Duration = Duration::from_secs(6);
pub(super) const MAX_POINTS: usize =
    (CHART_DURATION.as_secs() / LIVE_INTERVAL.as_secs()) as usize + 1;

#[derive(Clone, Copy, serde::Serialize)]
pub(crate) struct CpuHistoryPoint {
    pub(super) time: u64,
    pub(super) usage: f32,
    pub(super) starts_segment: bool,
}

#[derive(Default)]
pub(super) struct CpuHistory {
    points: VecDeque<CpuHistoryPoint>,
    last_time: Option<u64>,
}

impl CpuHistory {
    pub(super) fn from_points(points: Vec<CpuHistoryPoint>) -> Self {
        Self {
            points: points.into(),
            last_time: None,
        }
    }

    pub(super) fn record(
        &mut self,
        time: u64,
        usage: f32,
        elapsed: Duration,
    ) -> Option<CpuHistoryPoint> {
        if time == 0 {
            return None;
        }
        let previous_time = self.last_time.replace(time);
        let last_point_time = self.points.back().map(|point| point.time);
        if previous_time
            .or(last_point_time)
            .is_some_and(|previous| time < previous)
        {
            self.points.clear();
        }

        // 跳过休眠后第一次读数，避免把整个间隔的 CPU 均值画成新采样点。
        let wall_gap = previous_time.is_some_and(|previous| {
            time.saturating_sub(previous) > MAX_SAMPLE_GAP.as_millis() as u64
        });
        let point = (!wall_gap
            && elapsed <= MAX_SAMPLE_GAP
            && previous_time != Some(time)
            && last_point_time != Some(time)
            && usage.is_finite())
        .then(|| CpuHistoryPoint {
            time,
            usage: usage.clamp(0.0, 100.0),
            // 重启或中断后的首个有效采样开启新线段，即使间隔很短也不连线。
            starts_segment: previous_time.is_none()
                || previous_time != last_point_time
                || previous_time.is_some_and(|previous| time < previous),
        });
        if let Some(point) = point {
            self.points.push_back(point);
        }

        let cutoff = time.saturating_sub(CHART_DURATION.as_millis() as u64);
        while self
            .points
            .front()
            .is_some_and(|point| point.time < cutoff || self.points.len() > MAX_POINTS)
        {
            self.points.pop_front();
        }
        point
    }

    pub(super) fn after(&self, since: Option<u64>) -> Vec<CpuHistoryPoint> {
        let index = match (since, self.points.back()) {
            (Some(since), Some(latest)) if since <= latest.time => {
                self.points.partition_point(|point| point.time <= since)
            }
            // 前端游标来自回拨前的时间线，返回当前的完整记录。
            _ => 0,
        };
        self.points.range(index..).copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_a_new_segment_after_restart_even_for_a_short_gap() {
        let mut history = CpuHistory::default();
        history.record(2_000, 25.0, LIVE_INTERVAL);
        history.record(4_000, 30.0, LIVE_INTERVAL);

        let mut restarted = CpuHistory::from_points(history.after(None));
        let resumed = restarted.record(8_000, 40.0, LIVE_INTERVAL).unwrap();
        assert!(resumed.starts_segment);
        assert_eq!(
            restarted
                .after(None)
                .iter()
                .map(|point| point.time)
                .collect::<Vec<_>>(),
            [2_000, 4_000, 8_000]
        );
    }

    #[test]
    fn retains_six_hours_of_samples_and_returns_only_requested_delta() {
        let mut history = CpuHistory::default();
        let interval = LIVE_INTERVAL.as_millis() as u64;
        let duration = CHART_DURATION.as_millis() as u64;
        let newest = duration + 3 * interval;
        for time in (interval..=newest).step_by(interval as usize) {
            history.record(time, 25.0, LIVE_INTERVAL);
        }

        let points = history.after(None);
        assert_eq!(points.first().unwrap().time, newest - duration);
        assert_eq!(points.last().unwrap().time, newest);
        assert_eq!(points.last().unwrap().usage, 25.0);
        let delta = history.after(Some(newest - interval));
        assert_eq!(delta.len(), 1);
        assert_eq!(delta[0].time, newest);
    }

    #[test]
    fn skips_cpu_averaged_across_suspend_and_resets_after_clock_rollback() {
        let mut history = CpuHistory::default();
        history.record(2_000, 10.0, LIVE_INTERVAL);
        history.record(20_000, 5.0, LIVE_INTERVAL);
        history.record(22_000, 18.0, LIVE_INTERVAL);
        let points = history.after(None);
        assert_eq!(
            points.iter().map(|point| point.time).collect::<Vec<_>>(),
            [2_000, 22_000]
        );

        history.record(5_000, 20.0, LIVE_INTERVAL);
        let points = history.after(Some(22_000));
        assert_eq!(
            points.iter().map(|point| point.time).collect::<Vec<_>>(),
            [5_000]
        );
    }
}

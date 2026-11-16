use std::{
    collections::{BTreeMap, VecDeque},
    path::Path,
    time::{Duration, Instant},
};

use rusqlite::{params, Connection, Result, Row};

use super::history::{CpuHistoryPoint, CHART_DURATION, MAX_POINTS, MAX_SAMPLE_GAP};

const DEFAULT_RETENTION_DAYS: u32 = 7;
const RETENTION_DAYS: std::ops::RangeInclusive<u32> = 1..=30;
const DAY_MS: u64 = 24 * 60 * 60_000;
const SAVE_INTERVAL: Duration = Duration::from_secs(15);
const ARCHIVE_BUCKETS: u64 = 1_200;

pub(super) struct CpuHistoryStore {
    db: Connection,
    retention_days: u32,
    pending: VecDeque<CpuHistoryPoint>,
    next_save: Instant,
}

impl CpuHistoryStore {
    pub(super) fn open(path: &Path, now: u64) -> Result<Self> {
        let db = Connection::open(path)?;
        db.busy_timeout(Duration::from_millis(500))?;
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS cpu_samples (
                time INTEGER PRIMARY KEY CHECK (time > 0),
                usage REAL NOT NULL,
                starts_segment INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS cpu_settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                retention_days INTEGER NOT NULL
            );",
        )?;
        db.execute(
            "INSERT OR IGNORE INTO cpu_settings VALUES (1, ?1)",
            [DEFAULT_RETENTION_DAYS],
        )?;
        let retention_days = db.query_row(
            "SELECT retention_days FROM cpu_settings WHERE id = 1",
            [],
            |row| row.get(0),
        )?;
        let store = Self {
            db,
            retention_days,
            pending: VecDeque::new(),
            next_save: Instant::now() + SAVE_INTERVAL,
        };
        store.db.execute(
            "DELETE FROM cpu_samples WHERE time < ?1",
            [retention_cutoff(now, retention_days) as i64],
        )?;
        Ok(store)
    }

    pub(super) fn retention_days(&self) -> u32 {
        self.retention_days
    }

    pub(super) fn recent_points(&self, now: u64) -> Result<Vec<CpuHistoryPoint>> {
        let start = now.saturating_sub(CHART_DURATION.as_millis() as u64);
        let mut query = self.db.prepare(
            "SELECT time, usage, starts_segment FROM cpu_samples
             WHERE time BETWEEN ?1 AND ?2 ORDER BY time DESC LIMIT ?3",
        )?;
        let mut points: Vec<_> = query
            .query_map(
                params![start as i64, now as i64, MAX_POINTS as i64],
                read_point,
            )?
            .collect::<Result<_>>()?;
        points.reverse();
        Ok(points)
    }

    pub(super) fn range_points(
        &self,
        start: u64,
        end: u64,
        now: u64,
    ) -> Result<Vec<CpuHistoryPoint>> {
        let start = start.max(retention_cutoff(now, self.retention_days));
        let end = end.min(now);
        if start > end {
            return Ok(Vec::new());
        }

        // 等待写入的记录可能在校时后乱序；按时间排序，同一时刻以最后一次采样为准。
        let pending = self
            .pending
            .iter()
            .filter(|point| (start..=end).contains(&point.time))
            .map(|point| (point.time, *point))
            .collect::<BTreeMap<_, _>>();
        let mut pending = pending.into_values().peekable();
        let mut query = self.db.prepare(
            "SELECT time, usage, starts_segment FROM cpu_samples
             WHERE time BETWEEN ?1 AND ?2 ORDER BY time",
        )?;
        let rows = query.query_map(params![start as i64, end as i64], read_point)?;
        let mut selected = ArchivePoints::new(start, end);
        for row in rows {
            let point = row?;
            while let Some(earlier) = pending.next_if(|entry| entry.time < point.time) {
                selected.push(earlier);
            }
            selected.push(
                pending
                    .next_if(|entry| entry.time == point.time)
                    .unwrap_or(point),
            );
        }
        for point in pending {
            selected.push(point);
        }
        Ok(selected.finish())
    }

    pub(super) fn queue(&mut self, point: CpuHistoryPoint) {
        // 持续写入失败时也限制内存队列大小。
        if self.pending.len() == MAX_POINTS {
            self.pending.pop_front();
        }
        self.pending.push_back(point);
    }

    pub(super) fn flush_if_due(&mut self, now: u64) -> Result<()> {
        if Instant::now() >= self.next_save {
            self.next_save = Instant::now() + SAVE_INTERVAL;
            self.flush(now)?;
        }
        Ok(())
    }

    pub(super) fn flush(&mut self, now: u64) -> Result<()> {
        self.save(now, self.retention_days)
    }

    pub(super) fn set_retention_days(&mut self, days: u32, now: u64) -> Result<(), String> {
        if !RETENTION_DAYS.contains(&days) {
            return Err("保存时间必须为 1 到 30 天".into());
        }
        self.save(now, days).map_err(|e| e.to_string())?;
        self.retention_days = days;
        Ok(())
    }

    fn save(&mut self, now: u64, days: u32) -> Result<()> {
        let cutoff = retention_cutoff(now, days);
        let transaction = self.db.transaction()?;
        {
            let mut insert = transaction.prepare(
                "INSERT OR REPLACE INTO cpu_samples (time, usage, starts_segment)
                 VALUES (?1, ?2, ?3)",
            )?;
            for point in self.pending.iter().filter(|point| point.time >= cutoff) {
                insert.execute(params![
                    point.time as i64,
                    point.usage,
                    point.starts_segment
                ])?;
            }
        }
        transaction.execute("DELETE FROM cpu_samples WHERE time < ?1", [cutoff as i64])?;
        if days != self.retention_days {
            transaction.execute(
                "UPDATE cpu_settings SET retention_days = ?1 WHERE id = 1",
                [days],
            )?;
        }
        transaction.commit()?;
        self.pending.clear();
        Ok(())
    }
}

fn read_point(row: &Row<'_>) -> Result<CpuHistoryPoint> {
    Ok(CpuHistoryPoint {
        time: row.get::<_, i64>(0)? as u64,
        usage: row.get(1)?,
        starts_segment: row.get(2)?,
    })
}

// 按时间分桶减少长区间的传输量。遇到断点立即结束桶，避免跨空白连线。
struct ArchivePoints {
    start: u64,
    bucket_ms: u64,
    points: Vec<CpuHistoryPoint>,
    bucket: Option<ArchiveBucket>,
}

impl ArchivePoints {
    fn new(start: u64, end: u64) -> Self {
        Self {
            start,
            bucket_ms: (end - start).div_ceil(ARCHIVE_BUCKETS).max(1),
            points: Vec::new(),
            bucket: None,
        }
    }

    fn push(&mut self, mut point: CpuHistoryPoint) {
        let number = (point.time - self.start) / self.bucket_ms;
        point.starts_segment |= self
            .bucket
            .as_ref()
            .is_none_or(|bucket| point.time - bucket.last.time > MAX_SAMPLE_GAP.as_millis() as u64);
        if point.starts_segment
            || self
                .bucket
                .as_ref()
                .is_some_and(|bucket| bucket.number != number)
        {
            self.end_bucket();
        }
        if let Some(bucket) = &mut self.bucket {
            bucket.push(point);
        } else {
            self.bucket = Some(ArchiveBucket {
                number,
                first: point,
                low: point,
                high: point,
                last: point,
            });
        }
    }

    fn end_bucket(&mut self) {
        if let Some(bucket) = self.bucket.take() {
            let mut points = [bucket.first, bucket.low, bucket.high, bucket.last];
            points.sort_unstable_by_key(|point| point.time);
            for point in points {
                if self
                    .points
                    .last()
                    .is_none_or(|last| last.time != point.time)
                {
                    self.points.push(point);
                }
            }
        }
    }

    fn finish(mut self) -> Vec<CpuHistoryPoint> {
        self.end_bucket();
        self.points
    }
}

struct ArchiveBucket {
    number: u64,
    first: CpuHistoryPoint,
    low: CpuHistoryPoint,
    high: CpuHistoryPoint,
    last: CpuHistoryPoint,
}

impl ArchiveBucket {
    fn push(&mut self, point: CpuHistoryPoint) {
        if point.usage < self.low.usage {
            self.low = point;
        }
        if point.usage > self.high.usage {
            self.high = point;
        }
        self.last = point;
    }
}

fn retention_cutoff(now: u64, days: u32) -> u64 {
    now.saturating_sub(u64::from(days) * DAY_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved_times(store: &CpuHistoryStore) -> Vec<u64> {
        store
            .db
            .prepare("SELECT time FROM cpu_samples ORDER BY time")
            .unwrap()
            .query_map([], |row| Ok(row.get::<_, i64>(0)? as u64))
            .unwrap()
            .collect::<Result<_>>()
            .unwrap()
    }

    fn sample(time: u64, usage: f32) -> CpuHistoryPoint {
        CpuHistoryPoint {
            time,
            usage,
            starts_segment: false,
        }
    }

    #[test]
    fn searches_reopened_history_with_inclusive_bounds() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.sqlite3");
        let now = 10 * DAY_MS;
        let start = now - 2 * DAY_MS;
        let end = start + 4_000;
        let mut store = CpuHistoryStore::open(&path, now).unwrap();
        for time in [start - 2_000, start, start + 2_000, end, end + 2_000] {
            store.queue(sample(time, 32.0));
        }
        store.flush(now).unwrap();
        drop(store);

        let store = CpuHistoryStore::open(&path, now).unwrap();
        assert!(store.recent_points(now).unwrap().is_empty());
        let points = store.range_points(start, end, now).unwrap();
        assert_eq!(
            points.iter().map(|point| point.time).collect::<Vec<_>>(),
            [start, start + 2_000, end]
        );
        assert!(points[0].starts_segment);
        assert!(!points[1].starts_segment);
    }

    #[test]
    fn includes_pending_samples_and_respects_current_retention_and_time() {
        let now = 10 * DAY_MS;
        let cutoff = now - 7 * DAY_MS;
        let mut store = CpuHistoryStore::open(Path::new(":memory:"), now - 1).unwrap();
        store.queue(sample(cutoff - 1, 20.0));
        store.queue(sample(cutoff, 30.0));
        store.queue(sample(now - 4_000, 40.0));
        store.flush(now - 1).unwrap();
        store.queue(sample(now - 2_000, 50.0));
        store.queue(sample(now, 60.0));
        store.queue(sample(now - 2_000, 70.0));
        store.queue(sample(now + 2_000, 80.0));

        let points = store
            .range_points(cutoff - 2_000, now + 4_000, now)
            .unwrap();
        assert_eq!(
            points
                .iter()
                .map(|point| (point.time, point.usage))
                .collect::<Vec<_>>(),
            [
                (cutoff, 30.0),
                (now - 4_000, 40.0),
                (now - 2_000, 70.0),
                (now, 60.0)
            ]
        );
        assert!(store
            .range_points(cutoff - 1, cutoff - 1, now)
            .unwrap()
            .is_empty());
        assert!(store
            .range_points(cutoff - 2_000, cutoff - 1, now)
            .unwrap()
            .is_empty());
        assert!(store
            .range_points(now + 1, now + 4_000, now)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn compresses_a_long_range_without_bridging_gaps_or_hiding_extremes() {
        let now = 10 * DAY_MS;
        let start = now - 2 * DAY_MS;
        let explicit_break = start + 7_200_000;
        let before_gap = start + 14_400_000;
        let after_gap = before_gap + 16_000;
        let peak = start + 82_000;
        let low = start + 846_000;
        let end = after_gap + 14_400_000;
        let mut store = CpuHistoryStore::open(Path::new(":memory:"), now).unwrap();
        for time in (start..=end).step_by(2_000) {
            if time > before_gap && time < after_gap {
                continue;
            }
            let usage = if time == peak {
                100.0
            } else if time == low {
                0.0
            } else {
                40.0
            };
            let mut point = sample(time, usage);
            point.starts_segment = time == explicit_break;
            store.queue(point);
            if time % 600_000 == 0 {
                store.flush(now).unwrap();
            }
        }
        store.flush(now).unwrap();

        let points = store.range_points(start, now, now).unwrap();
        assert!(points.len() < 1_000);
        assert_eq!(points.last().unwrap().time, end);
        assert_eq!(
            points
                .iter()
                .filter(|point| point.starts_segment)
                .map(|point| point.time)
                .collect::<Vec<_>>(),
            [start, explicit_break, after_gap]
        );
        for time in [peak, low, explicit_break - 2_000, before_gap] {
            assert!(points.iter().any(|point| point.time == time));
        }
        assert!(points.windows(2).all(|pair| pair[0].time < pair[1].time));
    }

    #[test]
    fn keeps_configured_days_on_disk_and_recovers_the_recent_chart_window() {
        let directory = tempfile::tempdir().unwrap();
        let now = 10 * DAY_MS;
        let path = directory.path().join("history.sqlite3");
        let mut store = CpuHistoryStore::open(&path, now).unwrap();
        assert_eq!(store.retention_days(), 7);
        for time in [
            now - 8 * DAY_MS,
            now - 6 * DAY_MS,
            now - 2 * DAY_MS,
            now - 2_000,
        ] {
            store.queue(CpuHistoryPoint {
                time,
                usage: 30.0,
                starts_segment: true,
            });
        }
        store.flush(now).unwrap();
        drop(store);

        let mut store = CpuHistoryStore::open(&path, now).unwrap();
        let recent = store.recent_points(now).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].time, now - 2_000);
        assert!(recent[0].starts_segment);
        assert_eq!(
            saved_times(&store),
            [now - 6 * DAY_MS, now - 2 * DAY_MS, now - 2_000]
        );
        store.set_retention_days(3, now).unwrap();
        assert!(store.set_retention_days(0, now).is_err());
        assert_eq!(saved_times(&store), [now - 2 * DAY_MS, now - 2_000]);
        drop(store);

        let store = CpuHistoryStore::open(&path, now + 4 * DAY_MS).unwrap();
        assert_eq!(store.retention_days(), 3);
        assert!(saved_times(&store).is_empty());
    }
}

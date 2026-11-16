use std::{
    collections::VecDeque,
    path::Path,
    time::{Duration, Instant},
};

use rusqlite::{params, Connection, Result};

use super::history::{CpuHistoryPoint, CHART_DURATION, MAX_POINTS};

const DEFAULT_RETENTION_DAYS: u32 = 7;
const RETENTION_DAYS: std::ops::RangeInclusive<u32> = 1..=30;
const DAY_MS: u64 = 24 * 60 * 60_000;
const SAVE_INTERVAL: Duration = Duration::from_secs(15);

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
                |row| {
                    Ok(CpuHistoryPoint {
                        time: row.get::<_, i64>(0)? as u64,
                        usage: row.get(1)?,
                        starts_segment: row.get(2)?,
                    })
                },
            )?
            .collect::<Result<_>>()?;
        points.reverse();
        Ok(points)
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

//! The journal: append-only logical events in SQLite. Projections, such as the ledger, are rebuilt from
//! it. A local journal is not tamper-proof, and v1 makes no such claim.

use rusqlite::Connection;
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub id: i64,
    pub kind: String,
    pub payload: Value,
    pub at: u64,
}

pub struct Journal {
    conn: Connection,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Settings {
    pub journal_mode: String,
    pub synchronous: i64,
    pub foreign_keys: i64,
    pub fullfsync: i64,
    pub checkpoint_fullfsync: i64,
}

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL CHECK (length(kind) > 0),
    payload TEXT NOT NULL,
    at INTEGER NOT NULL
);";

fn err(e: rusqlite::Error) -> String {
    e.to_string()
}

impl Journal {
    /// WAL, `synchronous=FULL`, full fsync on commit and checkpoint, foreign keys on (plan section 11).
    pub fn open(path: &Path) -> Result<Journal, String> {
        let conn = Connection::open(path).map_err(err)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(err)?;
        for (k, v) in [
            ("synchronous", "FULL"),
            ("fullfsync", "ON"),
            ("checkpoint_fullfsync", "ON"),
            ("foreign_keys", "ON"),
        ] {
            conn.pragma_update(None, k, v).map_err(err)?;
        }
        conn.execute_batch(SCHEMA).map_err(err)?;
        Ok(Journal { conn })
    }

    pub fn open_in_memory() -> Result<Journal, String> {
        let conn = Connection::open_in_memory().map_err(err)?;
        conn.execute_batch(SCHEMA).map_err(err)?;
        Ok(Journal { conn })
    }

    pub fn settings(&self) -> Result<Settings, String> {
        let int = |name: &str| {
            self.conn
                .pragma_query_value(None, name, |r| r.get::<_, i64>(0))
                .map_err(err)
        };
        Ok(Settings {
            journal_mode: self
                .conn
                .pragma_query_value(None, "journal_mode", |r| r.get::<_, String>(0))
                .map_err(err)?,
            synchronous: int("synchronous")?,
            foreign_keys: int("foreign_keys")?,
            fullfsync: int("fullfsync")?,
            checkpoint_fullfsync: int("checkpoint_fullfsync")?,
        })
    }

    /// Appends one event and returns its ID. There is no update and no delete.
    pub fn append(&self, kind: &str, payload: &Value, at: u64) -> Result<i64, String> {
        self.conn
            .execute(
                "INSERT INTO events (kind, payload, at) VALUES (?1, ?2, ?3)",
                (kind, payload.to_string(), at as i64),
            )
            .map_err(err)?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Events with an ID above `after`, oldest first.
    pub fn events_since(&self, after: i64) -> Result<Vec<Event>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, kind, payload, at FROM events WHERE id > ?1 ORDER BY id")
            .map_err(err)?;
        let rows = stmt
            .query_map([after], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })
            .map_err(err)?;
        let mut out = Vec::new();
        for row in rows {
            let (id, kind, payload, at) = row.map_err(err)?;
            out.push(Event {
                id,
                kind,
                payload: serde_json::from_str(&payload).map_err(|e| e.to_string())?,
                at: at as u64,
            });
        }
        Ok(out)
    }

    pub fn events(&self) -> Result<Vec<Event>, String> {
        self.events_since(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("bff-journal-{}-{name}.db", std::process::id()))
    }

    #[test]
    fn events_come_back_in_order_with_their_payloads() {
        let j = Journal::open_in_memory().unwrap();
        let a = j
            .append("plan.proposed", &json!({"stages": 2}), 10)
            .unwrap();
        let b = j
            .append("plan.approved", &json!({"by": "Nitish"}), 11)
            .unwrap();
        assert!(b > a);
        let all = j.events().unwrap();
        assert_eq!(
            all.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(),
            ["plan.proposed", "plan.approved"]
        );
        assert_eq!(all[0].payload, json!({"stages": 2}));
        assert_eq!((all[1].id, all[1].at), (b, 11));
    }

    #[test]
    fn events_since_returns_only_newer_ones() {
        let j = Journal::open_in_memory().unwrap();
        let a = j.append("a", &json!(1), 1).unwrap();
        j.append("b", &json!(2), 2).unwrap();
        let rest = j.events_since(a).unwrap();
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].kind, "b");
    }

    #[test]
    fn an_empty_kind_is_refused() {
        let j = Journal::open_in_memory().unwrap();
        assert!(j.append("", &json!({}), 1).is_err());
        assert!(j.events().unwrap().is_empty());
    }

    #[test]
    fn the_journal_survives_a_reopen() {
        let p = temp("reopen");
        {
            let j = Journal::open(&p).unwrap();
            j.append("run.started", &json!({"id": "r1"}), 5).unwrap();
        }
        let j = Journal::open(&p).unwrap();
        let ev = j.events().unwrap();
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].payload["id"], "r1");
        let next = j.append("run.finished", &json!({}), 6).unwrap();
        assert!(next > ev[0].id);
    }

    #[test]
    fn durability_settings_follow_the_plan() {
        let p = temp("settings");
        let s = Journal::open(&p).unwrap().settings().unwrap();
        assert_eq!(
            s,
            Settings {
                journal_mode: "wal".into(),
                synchronous: 2,
                foreign_keys: 1,
                fullfsync: 1,
                checkpoint_fullfsync: 1
            }
        );
    }
}

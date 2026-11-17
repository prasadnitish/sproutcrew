//! Configuration that changes while a run is going. A change never reaches a task that has already started:
//! each task keeps the snapshot it was dispatched with, and the change applies to tasks dispatched after it,
//! which are new sessions. Versions only go up.

use serde_json::{Map, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub version: u32,
    pub values: Map<String, Value>,
}

#[derive(Default)]
pub struct Config {
    version: u32,
    values: Map<String, Value>,
    tasks: BTreeMap<String, Snapshot>,
}

impl Config {
    pub fn new(values: Map<String, Value>) -> Config {
        Config {
            version: 1,
            values,
            tasks: BTreeMap::new(),
        }
    }

    pub fn current(&self) -> Snapshot {
        Snapshot {
            version: self.version,
            values: self.values.clone(),
        }
    }

    /// Changes one setting. Returns the new version. Running tasks are not touched.
    pub fn set(&mut self, key: &str, value: Value) -> u32 {
        if self.values.get(key) != Some(&value) {
            self.values.insert(key.to_string(), value);
            self.version += 1;
        }
        self.version
    }

    /// A task starts: it takes the current snapshot and keeps it.
    pub fn start_task(&mut self, task: &str) -> Snapshot {
        let snap = self.current();
        self.tasks.entry(task.to_string()).or_insert(snap).clone()
    }

    pub fn snapshot_of(&self, task: &str) -> Option<&Snapshot> {
        self.tasks.get(task)
    }

    /// Tasks whose snapshot is older than the current configuration, which a person may want to restart.
    pub fn stale_tasks(&self) -> Vec<&str> {
        self.tasks
            .iter()
            .filter(|(_, s)| s.version != self.version)
            .map(|(t, _)| t.as_str())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cfg() -> Config {
        let mut m = Map::new();
        m.insert("max_output_tokens".into(), json!(1000));
        Config::new(m)
    }

    #[test]
    fn a_change_made_while_a_task_runs_reaches_only_tasks_started_afterwards() {
        let mut c = cfg();
        let first = c.start_task("t1");
        assert_eq!(first.version, 1);
        assert_eq!(c.set("max_output_tokens", json!(2000)), 2);
        assert_eq!(
            c.snapshot_of("t1").unwrap().values["max_output_tokens"],
            1000,
            "the running task keeps its settings"
        );
        let second = c.start_task("t2");
        assert_eq!(
            (second.version, &second.values["max_output_tokens"]),
            (2, &json!(2000))
        );
        assert_eq!(c.stale_tasks(), vec!["t1"]);
    }

    #[test]
    fn setting_the_same_value_changes_nothing_and_a_task_cannot_re_snapshot() {
        let mut c = cfg();
        assert_eq!(c.set("max_output_tokens", json!(1000)), 1);
        c.start_task("t1");
        c.set("x", json!(true));
        assert_eq!(
            c.start_task("t1").version,
            1,
            "starting again returns the original snapshot"
        );
        assert!(c.snapshot_of("none").is_none());
    }

    #[test]
    fn versions_only_go_up() {
        let mut c = cfg();
        let mut last = c.current().version;
        for i in 0..5 {
            let v = c.set("k", json!(i));
            assert!(v >= last);
            last = v;
        }
        assert_eq!(last, 6);
    }
}

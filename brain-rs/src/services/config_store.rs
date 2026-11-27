//! SQLite-backed runtime configuration: the single source of truth.
//!
//! Layout (in the same database as the discovery `devices` table):
//!
//! * `settings(key, value)`: one row per top-level `Config` section (`audio`, `wled`, `station`,
//!   `schedule`, ...), the value being that section as JSON. A section's JSON is stored as it was
//!   written (not round-tripped through the Rust structs), so keys the section leaves out keep
//!   falling back to the serde defaults and secrets (`station.provision.*`) survive. Keys that
//!   start with `_` are bookkeeping (`_seeded`), not configuration.
//! * `config_history(id, ts, key, old_value, new_value, source)`: appended on every change, pruned
//!   to the last [`HISTORY_CAP`] rows. `restore` undoes one entry.
//!
//! `config.yaml` is only a seed: [`ConfigStore::seed_from_yaml`] imports it once (first boot) and
//! never touches the file again. Bootstrap values that cannot live in the DB are the database
//! path (`DATABASE_URL`) and the HTTP bind/port (`--bind`, `--port`, or `BRAIN_SERVER`).

use crate::config::Config;
use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use serde_json::{Map, Value};
use sqlx::{Pool, Sqlite};
use std::path::Path;
use std::sync::Arc;
use tracing::{info, warn};

/// History rows kept; older ones are deleted on every write.
pub const HISTORY_CAP: i64 = 500;
/// Marker row: set once the YAML seed was imported (or the config was reset on purpose).
const SEEDED: &str = "_seeded";

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct HistoryRow {
    pub id: i64,
    pub ts: String,
    pub key: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub source: String,
}

/// Cheap to clone; clones share the writer lock. All writes go through [`commit`](Self::commit).
#[derive(Clone)]
pub struct ConfigStore {
    pool: Pool<Sqlite>,
    /// Serializes read-modify-write cycles so concurrent writers never lose each other's update.
    lock: Arc<tokio::sync::Mutex<()>>,
}

fn is_config_key(k: &str) -> bool {
    !k.starts_with('_')
}

impl ConfigStore {
    pub async fn new(pool: Pool<Sqlite>) -> Result<Self> {
        sqlx::query("CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
            .execute(&pool)
            .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS config_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                ts TEXT NOT NULL,
                key TEXT NOT NULL,
                old_value TEXT,
                new_value TEXT,
                source TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await?;
        Ok(Self { pool, lock: Arc::new(tokio::sync::Mutex::new(())) })
    }

    // ---------------------------------------------------------------- reads

    async fn sections_unlocked(&self) -> Result<Map<String, Value>> {
        let rows: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM settings")
            .fetch_all(&self.pool)
            .await?;
        let mut out = Map::new();
        for (k, v) in rows.into_iter().filter(|(k, _)| is_config_key(k)) {
            match serde_json::from_str::<Value>(&v) {
                Ok(v) => {
                    out.insert(k, v);
                }
                Err(e) => warn!("Config section '{k}' is not valid JSON ({e}); using defaults for it"),
            }
        }
        Ok(out)
    }

    /// Every stored section (raw JSON), keyed by name.
    pub async fn sections(&self) -> Result<Map<String, Value>> {
        let _g = self.lock.lock().await;
        self.sections_unlocked().await
    }

    pub async fn get_section(&self, key: &str) -> Result<Option<Value>> {
        Ok(self.sections().await?.remove(key))
    }

    /// The effective `Config`: stored sections over the serde defaults (and `BRAIN_*` env).
    pub async fn load_config(&self) -> Result<Config> {
        let sections = self.sections().await?;
        Config::from_sections(&sections).map_err(|e| anyhow!("stored config is invalid: {e}"))
    }

    /// Value at a dotted path (`audio.shout_margin_db`): stored if present, else the effective one.
    pub async fn get_path(&self, path: &str) -> Result<Option<Value>> {
        let sections = self.sections().await?;
        if let Some(v) = lookup(&Value::Object(sections.clone()), path) {
            return Ok(Some(v.clone()));
        }
        let cfg = Config::from_sections(&sections).map_err(|e| anyhow!("stored config is invalid: {e}"))?;
        Ok(lookup(&serde_json::to_value(cfg)?, path).cloned())
    }

    /// Stored sections laid over the defaults, as one JSON object (for `config dump`).
    pub async fn dump_value(&self) -> Result<Value> {
        let mut base = serde_json::to_value(Config::default())?;
        merge(&mut base, Value::Object(self.sections().await?));
        Ok(base)
    }

    pub async fn dump_yaml(&self) -> Result<String> {
        Ok(serde_yaml::to_string(&self.dump_value().await?)?)
    }

    pub async fn history(&self, limit: i64) -> Result<Vec<HistoryRow>> {
        Ok(sqlx::query_as::<_, HistoryRow>(
            "SELECT id, ts, key, old_value, new_value, source FROM config_history ORDER BY id DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?)
    }

    // --------------------------------------------------------------- writes

    /// Apply `changes` (`None` deletes the section) atomically. The resulting config must still
    /// parse, otherwise nothing is written. Unchanged sections produce no history.
    pub async fn commit(&self, changes: Vec<(String, Option<Value>)>, source: &str) -> Result<()> {
        let _g = self.lock.lock().await;
        self.commit_unlocked(changes, source).await
    }

    async fn commit_unlocked(&self, changes: Vec<(String, Option<Value>)>, source: &str) -> Result<()> {
        let mut candidate = self.sections_unlocked().await?;
        for (k, v) in &changes {
            if !is_config_key(k) {
                bail!("'{k}' is not a config section name");
            }
            match v {
                Some(v) => candidate.insert(k.clone(), v.clone()),
                None => candidate.remove(k),
            };
        }
        Config::from_sections(&candidate).map_err(|e| anyhow!("rejected, the config would not load: {e}"))?;

        let mut tx = self.pool.begin().await?;
        let ts = chrono::Utc::now().to_rfc3339();
        for (k, new) in changes {
            let old: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
                .bind(&k)
                .fetch_optional(&mut *tx)
                .await?;
            let old = old.map(|r| r.0);
            let new_text = new.as_ref().map(|v| v.to_string());
            let same = match (&old, &new) {
                (None, None) => true,
                (Some(o), Some(n)) => serde_json::from_str::<Value>(o).ok().as_ref() == Some(n),
                _ => false,
            };
            if same {
                continue;
            }
            match &new_text {
                Some(t) => sqlx::query(
                    "INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                )
                .bind(&k)
                .bind(t)
                .execute(&mut *tx)
                .await?,
                None => sqlx::query("DELETE FROM settings WHERE key = ?").bind(&k).execute(&mut *tx).await?,
            };
            sqlx::query("INSERT INTO config_history (ts, key, old_value, new_value, source) VALUES (?, ?, ?, ?, ?)")
                .bind(&ts)
                .bind(&k)
                .bind(&old)
                .bind(&new_text)
                .bind(source)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("DELETE FROM config_history WHERE id <= (SELECT COALESCE(MAX(id), 0) FROM config_history) - ?")
            .bind(HISTORY_CAP)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn set_section(&self, key: &str, value: Value, source: &str) -> Result<()> {
        self.commit(vec![(key.to_string(), Some(value))], source).await
    }

    /// Read-modify-write one section under the writer lock. `f` sees the stored JSON (an empty
    /// object when the section is not stored yet).
    pub async fn update_section<F>(&self, key: &str, source: &str, f: F) -> Result<()>
    where
        F: FnOnce(&mut Value) -> Result<(), String>,
    {
        let _g = self.lock.lock().await;
        let mut cur = self.sections_unlocked().await?.remove(key).unwrap_or_else(|| Value::Object(Map::new()));
        f(&mut cur).map_err(|e| anyhow!(e))?;
        self.commit_unlocked(vec![(key.to_string(), Some(cur))], source).await
    }

    /// Set the value at a dotted path (`wled.udp_fps`), creating objects on the way.
    pub async fn set_path(&self, path: &str, value: Value, source: &str) -> Result<()> {
        let (section, rest) = path.split_once('.').map_or((path, ""), |(a, b)| (a, b));
        if section.is_empty() {
            bail!("empty key");
        }
        if rest.is_empty() {
            return self.set_section(section, value, source).await;
        }
        let rest = rest.to_string();
        self.update_section(section, source, move |cur| {
            if !cur.is_object() {
                *cur = Value::Object(Map::new());
            }
            let mut node = cur;
            let parts: Vec<&str> = rest.split('.').collect();
            for (i, p) in parts.iter().enumerate() {
                let obj = node.as_object_mut().ok_or_else(|| format!("'{p}' is inside a non-object value"))?;
                if i == parts.len() - 1 {
                    obj.insert((*p).to_string(), value.clone());
                    break;
                }
                node = obj.entry((*p).to_string()).or_insert_with(|| Value::Object(Map::new()));
                if !node.is_object() {
                    *node = Value::Object(Map::new());
                }
            }
            Ok(())
        })
        .await
    }

    async fn mark_seeded(&self) -> Result<()> {
        sqlx::query("INSERT INTO settings (key, value) VALUES (?, '1') ON CONFLICT(key) DO NOTHING")
            .bind(SEEDED)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Import top-level sections from YAML text. `only_missing` skips sections already stored.
    /// Returns the imported section names.
    pub async fn import_yaml_str(&self, text: &str, source: &str, only_missing: bool) -> Result<Vec<String>> {
        let doc: serde_yaml::Value = serde_yaml::from_str(text).context("parse YAML")?;
        let json = serde_json::to_value(doc)?;
        let Value::Object(map) = json else {
            if json.is_null() {
                return Ok(vec![]);
            }
            bail!("YAML top level must be a mapping of sections");
        };
        let _g = self.lock.lock().await;
        let have = self.sections_unlocked().await?;
        let changes: Vec<_> = map
            .into_iter()
            .filter(|(k, v)| is_config_key(k) && !v.is_null() && !(only_missing && have.contains_key(k)))
            .map(|(k, v)| (k, Some(v)))
            .collect();
        let names = changes.iter().map(|c| c.0.clone()).collect();
        self.commit_unlocked(changes, source).await?;
        Ok(names)
    }

    pub async fn import_yaml_file(&self, path: &Path, source: &str, only_missing: bool) -> Result<Vec<String>> {
        let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        self.import_yaml_str(&text, source, only_missing).await
    }

    /// First boot: import `path` once. Returns whether an import happened. The file is only read.
    pub async fn seed_from_yaml(&self, path: &Path) -> Result<bool> {
        let seeded: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
            .bind(SEEDED)
            .fetch_optional(&self.pool)
            .await?;
        if seeded.is_some() {
            return Ok(false);
        }
        if !path.exists() {
            info!("Config: no stored config and no seed file {}; using built-in defaults", path.display());
            return Ok(false);
        }
        let names = self.import_yaml_file(path, "seed", true).await?;
        self.mark_seeded().await?;
        info!("Config: first boot, imported {} section(s) from {} into the database ({})", names.len(), path.display(), names.join(", "));
        Ok(true)
    }

    /// Delete every config section (history is kept) so the built-in defaults apply. The seed
    /// marker is set, so the YAML is not re-imported; use `config import` for that.
    pub async fn reset(&self, source: &str) -> Result<()> {
        let _g = self.lock.lock().await;
        let keys: Vec<_> = self.sections_unlocked().await?.into_iter().map(|(k, _)| (k, None)).collect();
        self.commit_unlocked(keys, source).await?;
        self.mark_seeded().await
    }

    /// Undo history entry `id`: put the key back to the value it had before that change
    /// (deleting it when it did not exist). Recorded in the history itself.
    pub async fn restore(&self, id: i64) -> Result<String> {
        let row: Option<HistoryRow> = sqlx::query_as::<_, HistoryRow>(
            "SELECT id, ts, key, old_value, new_value, source FROM config_history WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        let row = row.ok_or_else(|| anyhow!("no history entry {id} (pruned or never existed)"))?;
        let old = match &row.old_value {
            Some(t) => Some(serde_json::from_str::<Value>(t).context("old value in history is not JSON")?),
            None => None,
        };
        self.commit(vec![(row.key.clone(), old)], &format!("restore:{id}")).await?;
        Ok(row.key)
    }
}

fn lookup<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(v, |n, p| n.get(p))
}

/// Deep merge: objects merge key by key, anything else (arrays included) is replaced.
fn merge(base: &mut Value, over: Value) {
    match (base, over) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                match b.get_mut(&k) {
                    Some(bv) => merge(bv, v),
                    None => {
                        b.insert(k, v);
                    }
                }
            }
        }
        (b, o) => *b = o,
    }
}

/// CLI value: JSON first (`12.5`, `true`, `"x"`, `[1,2]`, `{"a":1}`), then a YAML scalar, then text.
pub fn parse_value(s: &str) -> Value {
    if let Ok(v) = serde_json::from_str(s) {
        return v;
    }
    if let Ok(v) = serde_yaml::from_str::<serde_yaml::Value>(s).map_err(anyhow::Error::from).and_then(|v| Ok(serde_json::to_value(v)?)) {
        return v;
    }
    Value::String(s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::database_manager::DatabaseManager;
    use serde_json::json;

    async fn store() -> ConfigStore {
        let db = DatabaseManager::new("sqlite::memory:").await.unwrap();
        (*db.config()).clone()
    }

    fn temp_db(name: &str) -> (std::path::PathBuf, String) {
        let p = std::env::temp_dir().join(format!("cfgstore-{}-{name}.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let url = format!("sqlite:{}", p.display());
        (p, url)
    }

    #[tokio::test]
    async fn empty_store_loads_defaults() {
        let s = store().await;
        let c = s.load_config().await.unwrap();
        assert_eq!(c.audio.shout_margin_db, Config::default().audio.shout_margin_db);
        assert_eq!(c.server.port, 8000);
    }

    #[tokio::test]
    async fn roundtrip_and_missing_keys_use_defaults() {
        let s = store().await;
        s.set_section("audio", json!({"shout_margin_db": 12.5}), "test").await.unwrap();
        let c = s.load_config().await.unwrap();
        assert_eq!(c.audio.shout_margin_db, 12.5);
        // a key the stored section leaves out keeps its default
        assert_eq!(c.audio.shout_sensitivity, Config::default().audio.shout_sensitivity);
        assert_eq!(s.get_path("audio.shout_margin_db").await.unwrap(), Some(json!(12.5)));
        // effective value is reported for keys not stored
        assert_eq!(s.get_path("server.port").await.unwrap(), Some(json!(8000)));
        s.set_path("wled.udp_fps", json!(20), "test").await.unwrap();
        assert_eq!(s.load_config().await.unwrap().wled.udp_fps, 20);
    }

    #[tokio::test]
    async fn invalid_value_is_rejected_and_nothing_written() {
        let s = store().await;
        assert!(s.set_path("audio.shout_margin_db", json!("loud"), "test").await.is_err());
        assert!(s.sections().await.unwrap().is_empty());
        assert!(s.history(10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn first_boot_imports_yaml_once_and_leaves_file_alone() {
        let (_p, url) = temp_db("seed");
        let yaml = std::env::temp_dir().join(format!("seed-{}.yaml", std::process::id()));
        let text = "server:\n  port: 9001\naudio:\n  shout_margin_db: 17.0\nstation:\n  provision:\n    provision_key: sekrit\n";
        std::fs::write(&yaml, text).unwrap();
        {
            let db = DatabaseManager::new(&url).await.unwrap();
            assert!(db.config().seed_from_yaml(&yaml).await.unwrap());
            let c = db.config().load_config().await.unwrap();
            assert_eq!(c.server.port, 9001);
            assert_eq!(c.audio.shout_margin_db, 17.0);
            assert_eq!(c.station.provision.provision_key, "sekrit", "secrets survive the DB");
        }
        // YAML edits after the first boot are ignored; the file is untouched
        std::fs::write(&yaml, "audio:\n  shout_margin_db: 1.0\n").unwrap();
        let db = DatabaseManager::new(&url).await.unwrap();
        assert!(!db.config().seed_from_yaml(&yaml).await.unwrap());
        assert_eq!(db.config().load_config().await.unwrap().audio.shout_margin_db, 17.0);
        assert_eq!(std::fs::read_to_string(&yaml).unwrap(), "audio:\n  shout_margin_db: 1.0\n");
        std::fs::remove_file(&yaml).ok();
    }

    #[tokio::test]
    async fn repo_config_yaml_seeds_to_the_same_config_as_loading_the_file() {
        let s = store().await;
        s.import_yaml_str(include_str!("../../config.yaml"), "seed", true).await.unwrap();
        let from_db = s.load_config().await.unwrap();
        let from_file = Config::load_with(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/config.yaml")), &[]).unwrap();
        assert_eq!(serde_json::to_value(&from_db).unwrap(), serde_json::to_value(&from_file).unwrap());
    }

    #[tokio::test]
    async fn seed_keeps_existing_legacy_schedule_row() {
        let db = DatabaseManager::new("sqlite::memory:").await.unwrap();
        db.set_setting("schedule", r#"{"enabled":true,"windows":[]}"#).await.unwrap();
        let s = db.config();
        s.import_yaml_str("schedule:\n  enabled: false\naudio:\n  shout_margin_db: 15.0\n", "seed", true).await.unwrap();
        let c = s.load_config().await.unwrap();
        assert!(c.schedule.enabled, "DB value wins over the seed");
        assert_eq!(c.audio.shout_margin_db, 15.0);
    }

    #[tokio::test]
    async fn history_records_changes_and_restore_undoes_one() {
        let s = store().await;
        s.set_path("audio.shout_margin_db", json!(10.0), "cli").await.unwrap();
        s.set_path("audio.shout_margin_db", json!(11.0), "cli").await.unwrap();
        s.set_path("audio.shout_margin_db", json!(11.0), "cli").await.unwrap(); // no-op, no row
        let h = s.history(10).await.unwrap();
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].source, "cli");
        // undo the latest change
        let key = s.restore(h[0].id).await.unwrap();
        assert_eq!(key, "audio");
        assert_eq!(s.load_config().await.unwrap().audio.shout_margin_db, 10.0);
        // undo the first one: the section did not exist before, so it is deleted
        let first_id = h[1].id;
        s.restore(first_id).await.unwrap();
        assert!(s.get_section("audio").await.unwrap().is_none());
        let h = s.history(10).await.unwrap();
        assert_eq!(h.len(), 4);
        assert_eq!(h[0].source, format!("restore:{first_id}"));
        assert!(s.restore(9999).await.is_err());
    }

    #[tokio::test]
    async fn history_is_capped() {
        let s = store().await;
        for i in 0..(HISTORY_CAP + 25) {
            s.set_path("wled.udp_fps", json!(10 + (i % 50)), "t").await.unwrap();
        }
        assert_eq!(s.history(10_000).await.unwrap().len() as i64, HISTORY_CAP);
    }

    #[tokio::test]
    async fn reset_wipes_config_but_keeps_history() {
        let s = store().await;
        s.set_path("audio.shout_margin_db", json!(10.0), "t").await.unwrap();
        s.reset("reset").await.unwrap();
        assert!(s.sections().await.unwrap().is_empty());
        assert_eq!(s.history(10).await.unwrap().len(), 2);
        // seeded marker set: a later boot does not re-import the YAML over the reset
        let yaml = std::env::temp_dir().join(format!("reset-{}.yaml", std::process::id()));
        std::fs::write(&yaml, "audio:\n  shout_margin_db: 1.0\n").unwrap();
        assert!(!s.seed_from_yaml(&yaml).await.unwrap());
        std::fs::remove_file(&yaml).ok();
    }

    #[tokio::test]
    async fn dump_yaml_reimports_to_the_same_config() {
        let s = store().await;
        s.import_yaml_str("audio:\n  shout_margin_db: 21.0\nstation:\n  provision:\n    wifi_pass: pw\n", "t", false).await.unwrap();
        let yaml = s.dump_yaml().await.unwrap();
        let s2 = store().await;
        s2.import_yaml_str(&yaml, "t", false).await.unwrap();
        let c = s2.load_config().await.unwrap();
        assert_eq!(c.audio.shout_margin_db, 21.0);
        assert_eq!(c.station.provision.wifi_pass, "pw");
    }

    #[tokio::test]
    async fn concurrent_writers_do_not_lose_sections_or_keys() {
        let (p, url) = temp_db("conc");
        let a = DatabaseManager::new(&url).await.unwrap();
        let mut tasks = vec![];
        for i in 0..20u32 {
            let s = a.config();
            tasks.push(tokio::spawn(async move {
                // same section, different keys, plus a section of its own (via set_path too)
                s.update_section("audio", "t", |v| {
                    v[format!("k{i}")] = json!(i);
                    Ok(())
                })
                .await
                .unwrap();
                s.set_path(&format!("audio.p{i}"), json!(i), "t").await.unwrap();
                s.set_section(&format!("extra{i}"), json!({"n": i}), "t").await.unwrap();
            }));
        }
        for t in tasks {
            t.await.unwrap();
        }
        let sections = a.config().sections().await.unwrap();
        for i in 0..20u32 {
            assert_eq!(sections[&format!("extra{i}")]["n"], json!(i));
            assert_eq!(sections["audio"][format!("k{i}")], json!(i));
            assert_eq!(sections["audio"][format!("p{i}")], json!(i));
        }
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn parse_value_accepts_json_and_yaml_scalars() {
        assert_eq!(parse_value("12.5"), json!(12.5));
        assert_eq!(parse_value("true"), json!(true));
        assert_eq!(parse_value("[1, 2]"), json!([1, 2]));
        assert_eq!(parse_value("hci1"), json!("hci1"));
        assert_eq!(parse_value("~"), Value::Null);
    }
}

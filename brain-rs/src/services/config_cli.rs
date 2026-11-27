//! `shadow-warrior-brain config <cmd>`: inspect and edit the stored configuration offline.
//!
//! Operates directly on the SQLite database (`DATABASE_URL`), so stop the service first
//! (the running brain keeps its loaded copy and would not see the change anyway).

use super::config_store::{parse_value, ConfigStore};
use super::database_manager::{database_url, DatabaseManager};
use anyhow::{anyhow, bail, Result};
use std::path::Path;

const USAGE: &str = "usage: shadow-warrior-brain config <command>
  get <key>              value at a dotted key (audio.shout_margin_db); stored, else effective
  set <key> <value>      JSON or YAML scalar (12.5, true, \"x\", [1,2]); validated before writing
  dump                   whole effective config as YAML on stdout
  import <file.yaml>     replace the sections present in the file
  history [N]            last N changes (default 20)
  restore <history-id>   undo that change (key goes back to its previous value)
  reset                  delete all stored sections (history kept; built-in defaults apply)
DATABASE_URL selects the database (default sqlite:shadow_warrior.db). Stop the service first.";

pub async fn cli(args: &[String]) -> Result<()> {
    let db = DatabaseManager::new(&database_url()).await?;
    run(&db.config(), args).await
}

pub async fn run(store: &ConfigStore, args: &[String]) -> Result<()> {
    let arg = |i: usize, what: &str| args.get(i).cloned().ok_or_else(|| anyhow!("missing {what}\n{USAGE}"));
    match args.first().map(String::as_str) {
        Some("get") => {
            let key = arg(1, "<key>")?;
            match store.get_path(&key).await? {
                Some(v) => println!("{}", serde_json::to_string_pretty(&v)?),
                None => bail!("no such key: {key}"),
            }
        }
        Some("set") => {
            let key = arg(1, "<key>")?;
            let value = parse_value(&arg(2, "<value>")?);
            store.set_path(&key, value, "cli").await?;
            println!("ok");
        }
        Some("dump") => print!("{}", store.dump_yaml().await?),
        Some("import") => {
            let file = arg(1, "<file.yaml>")?;
            let names = store.import_yaml_file(Path::new(&file), "cli-import", false).await?;
            println!("imported {} section(s): {}", names.len(), names.join(", "));
        }
        Some("history") => {
            let n = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(20);
            for h in store.history(n).await? {
                println!(
                    "{:>5}  {}  {:<10} [{}]\n       old: {}\n       new: {}",
                    h.id,
                    h.ts,
                    h.key,
                    h.source,
                    h.old_value.as_deref().map_or("(none)".into(), brief),
                    h.new_value.as_deref().map_or("(deleted)".into(), brief),
                );
            }
        }
        Some("restore") => {
            let id: i64 = arg(1, "<history-id>")?.parse().map_err(|_| anyhow!("history id must be a number"))?;
            let key = store.restore(id).await?;
            println!("restored '{key}' to its value before change {id}");
        }
        Some("reset") => {
            store.reset("cli-reset").await?;
            println!("all config sections deleted (history kept)");
        }
        _ => bail!("{USAGE}"),
    }
    Ok(())
}

fn brief(s: &str) -> String {
    if s.chars().count() > 160 {
        format!("{}...", s.chars().take(160).collect::<String>())
    } else {
        s.to_string()
    }
}

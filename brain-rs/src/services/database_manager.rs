use anyhow::{Context, Result};
use sqlx::{sqlite::{SqlitePoolOptions, SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous}, Pool, Sqlite};
use std::str::FromStr;
use std::time::Duration;
use std::sync::Arc;
use tracing::info;
use serde::Serialize;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct DeviceRecord {
    pub id: i64,
    pub name: String,
    pub device_type: String, // "wled", "tasmota", "punching_bag"
    pub host: String,
    pub port: i32,
    pub metadata: Option<String>,
}

pub struct DatabaseManager {
    pool: Pool<Sqlite>,
}

impl DatabaseManager {
    pub async fn new(database_url: &str) -> Result<Self> {
        info!("Initializing database: {}", database_url);
        
        let connection_options = SqliteConnectOptions::from_str(database_url)?
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5))
            .create_if_missing(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(connection_options)
            .await
            .context("Failed to connect to SQLite")?;

        let manager = Self { pool };
        manager.run_migrations().await?;
        
        Ok(manager)
    }

    async fn run_migrations(&self) -> Result<()> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS devices (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                device_type TEXT NOT NULL,
                host TEXT NOT NULL,
                port INTEGER NOT NULL,
                metadata TEXT,
                UNIQUE(device_type, host, port)
            )"
        )
        .execute(&self.pool)
        .await?;
        
        info!("Database migrations completed");
        Ok(())
    }

    pub async fn add_device(&self, name: &str, device_type: &str, host: &str, port: i32, metadata: Option<&str>) -> Result<i64> {
        let _ = sqlx::query(
            "INSERT INTO devices (name, device_type, host, port, metadata) 
             VALUES (?, ?, ?, ?, ?) 
             ON CONFLICT(device_type, host, port) DO UPDATE SET name=excluded.name, metadata=excluded.metadata"
        )
        .bind(name)
        .bind(device_type)
        .bind(host)
        .bind(port)
        .bind(metadata)
        .execute(&self.pool)
        .await?;
        
        Ok(0) // Return 0 as ID since we don't strictly need it for discovery registration
    }

    pub async fn get_devices(&self) -> Result<Vec<DeviceRecord>> {
        let records = sqlx::query_as::<_, DeviceRecord>(
            "SELECT id, name, device_type, host, port, metadata FROM devices"
        )
        .fetch_all(&self.pool)
        .await?;
        
        Ok(records)
    }

    pub async fn delete_device(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM devices WHERE id = ?").bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

pub type SharedDatabaseManager = Arc<DatabaseManager>;

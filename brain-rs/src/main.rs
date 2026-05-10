mod api;
mod config;
mod core;
mod models;
mod services;

use anyhow::Result;
use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

use crate::api::{arena, status, SharedState};
use crate::config::Config;
use crate::core::{EventBus, StateMachine};
use crate::services::{
    ArenaManager, AudioManager, BleManager, DatabaseManager, DiscoveryManager, MusicManager,
    TasmotaManager, WledManager,
};

#[tokio::main]
async fn main() -> Result<()> {
    // Load configuration
    let config = Config::load()?;

    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(
            config
                .server
                .log_level
                .parse::<Level>()
                .unwrap_or(Level::INFO),
        )
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("🥊 Shadow Warrior Brain starting...");
    info!("Configuration loaded successfully");

    // Initialize database
    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:shadow_warrior.db".to_string());
    if !std::path::Path::new("shadow_warrior.db").exists() {
        std::fs::File::create("shadow_warrior.db")?;
    }
    let db_manager: Arc<DatabaseManager> = Arc::new(DatabaseManager::new(&database_url).await?);
    info!("Database manager initialized");

    // Initialize event bus
    let event_bus = Arc::new(EventBus::new(1000));
    info!("Event bus initialized");

    // Initialize state machine
    let state_machine = Arc::new(StateMachine::new(event_bus.clone()));
    info!("State machine initialized");

    // Initialize hardware managers
    let ble_manager = Arc::new(BleManager::new(config.ble.clone(), event_bus.clone()));
    info!("BLE manager initialized");

    let audio_manager = Arc::new(AudioManager::new(config.audio.clone(), event_bus.clone()));
    info!("Audio manager initialized");

    let wled_manager = Arc::new(WledManager::new(config.wled.clone(), db_manager.clone(), event_bus.clone()));
    info!("WLED manager initialized");

    let tasmota_manager = Arc::new(TasmotaManager::new(
        config.tasmota.clone(),
        event_bus.clone(),
    ));
    info!("Tasmota manager initialized");

    let music_manager = Arc::new(MusicManager::new(config.music.clone(), event_bus.clone()));
    info!("Music manager initialized");

    let arena_manager = Arc::new(ArenaManager::new(
        config.clone(),
        state_machine.clone(),
        event_bus.clone(),
        audio_manager.clone(),
        ble_manager.clone(),
    ));
    info!("Arena manager initialized");

    // Start hardware managers
    info!("Starting hardware managers...");
    ble_manager.start().await?;
    audio_manager.start().await?;
    wled_manager.start().await?;
    wled_manager.clone().start_event_loop();
    tasmota_manager.clone().start_event_loop();
    music_manager.load_playlist().await?;

    // Handle music playback via events (moved here to avoid non-Send issues)
    let music_manager_clone = music_manager.clone();
    let mut music_event_rx = event_bus.subscribe();
    tokio::spawn(async move {
        use tracing::warn;
        loop {
            if let Ok(event) = music_event_rx.recv().await {
                match event {
                    core::Event::FightStarted { .. } => {
                        if let Err(e) = music_manager_clone.play().await {
                            warn!("Failed to start music: {}", e);
                        }
                    }
                    core::Event::SessionEnded { .. } => {
                        music_manager_clone.stop().await;
                    }
                    _ => {}
                }
            }
        }
    });

    // Create channel for API commands to arena manager
    let (arena_tx, arena_rx) = tokio::sync::mpsc::unbounded_channel();

    // Start arena manager
    info!("Starting arena manager...");
    arena_manager.clone().start(arena_rx).await?;

    // Publish startup event
    event_bus.publish(core::Event::SystemStartup);

    // Set initial state to IDLE
    state_machine
        .transition(core::ArenaState::Idle)
        .await?;
    info!("Initial state set to IDLE");

    // Initialize discovery manager
    let discovery_manager = Arc::new(DiscoveryManager::new(
        db_manager.clone(),
        ble_manager.clone(),
        event_bus.clone(),
    ));
    info!("Discovery manager initialized");

    // Build shared state for API handlers
    let app_state = Arc::new(SharedState {
        event_bus: event_bus.clone(),
        state_machine: state_machine.clone(),
        arena_tx,
        db: db_manager,
        discovery: discovery_manager,
        wled: wled_manager.clone(),
    });

    // Build application router
    let app = Router::new()
        // Serve web UI
        .route("/", get(serve_index))
        .nest_service("/static", ServeDir::new("src/web"))
        // API routes
        .route("/api/state", get(status::get_state))
        .route("/api/events", get(status::get_events))
        .route("/api/stats", get(status::get_statistics))
        .route("/api/arena/presence", post(arena::presence))
        .route("/api/arena/suspend", post(arena::suspend))
        .route("/api/arena/shout", post(arena::shout))
        .route("/api/arena/punch", post(arena::punch))
        .route("/api/arena/state", post(arena::set_state))
        .route("/api/arena/reset_stats", post(arena::reset_stats))
        .route("/api/arena/config", get(arena::get_config))
        .route("/api/arena/config", post(arena::update_config))
        // Discovery routes
        .route("/api/discovery/scan", post(api::discovery::scan_hardware))
        .route("/api/devices", get(api::discovery::get_devices))
        .route("/api/devices", post(api::discovery::add_device))
        .route("/api/devices/:id", post(api::discovery::delete_device)) // Use post for delete in some simple UIs or just use delete
        // Add state for all routes
        .with_state(app_state)
        // Add tracing middleware
        .layer(TraceLayer::new_for_http());

    // Start server
    let addr = format!("{}:{}", config.server.host, config.server.port);
    info!("🚀 Starting server on {}", addr);
    info!("🌐 Dashboard available at http://localhost:{}", config.server.port);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    // Cleanup on shutdown
    info!("Shutting down...");
    event_bus.publish(core::Event::SystemShutdown);

    Ok(())
}

// Serve index.html at root
async fn serve_index() -> axum::response::Html<String> {
    let html = std::fs::read_to_string("src/web/index.html")
        .unwrap_or_else(|_| "<h1>Error loading dashboard</h1>".to_string());
    axum::response::Html(html)
}

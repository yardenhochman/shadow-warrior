"""
Shadow Warrior Brain Controller - Main FastAPI Application

This is the central controller that orchestrates communication between:
- Punching bag sensors (BLE)
- LED controllers (BLE) 
- Audio interface (local)
- Web dashboard (HTTP/WebSocket)
"""

import asyncio
from contextlib import asynccontextmanager
from pathlib import Path

from fastapi import FastAPI
from fastapi.staticfiles import StaticFiles
from fastapi.responses import FileResponse

from shadow_warrior_brain.api import status, arena
from shadow_warrior_brain.services.arena_manager import ArenaManager
from shadow_warrior_brain.core.logging_config import get_logger

logger = get_logger(__name__)


@asynccontextmanager
async def lifespan(app: FastAPI):
    """Application lifespan management"""
    # Startup
    logger.info("Starting Shadow Warrior Brain Controller...")

    # Initialize services
    arena_manager = ArenaManager()

    # Store managers in app state
    app.state.arena_manager = arena_manager
    
    # Start background tasks
    # none for now

    logger.info("Brain Controller started successfully")

    yield

    # Shutdown
    logger.info("Shutting down Brain Controller...")

    # Signal SSE connections to close immediately
    status.signal_shutdown()

    # Give SSE connections a brief moment to close gracefully
    await asyncio.sleep(0.1)

    await arena_manager.cleanup()
    logger.info("Brain Controller shutdown complete")


# Create FastAPI application
app = FastAPI(
    title="Shadow Warrior Brain Controller",
    description="Central controller for Shadow Warrior boxing training system",
    version="0.1.0",
    lifespan=lifespan
)

# Include API routes
app.include_router(status.router, prefix="/api", tags=["monitoring"])
app.include_router(arena.router, prefix="/api", tags=["arena"])

# Mount static files
static_path = Path(__file__).parent / "static"
app.mount("/static", StaticFiles(directory=static_path), name="static")


@app.get("/")
async def dashboard():
    """Main dashboard interface"""
    static_path = Path(__file__).parent / "static" / "index.html"
    return FileResponse(static_path, media_type="text/html")



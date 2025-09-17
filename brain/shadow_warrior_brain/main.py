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
from fastapi.responses import HTMLResponse, FileResponse

from shadow_warrior_brain.api import status
from shadow_warrior_brain.services.ble_manager import BLEManager
from shadow_warrior_brain.services.audio_manager import AudioManager
from shadow_warrior_brain.services.session_manager import SessionManager


@asynccontextmanager
async def lifespan(app: FastAPI):
    """Application lifespan management"""
    # Startup
    print("Starting Shadow Warrior Brain Controller...")
    
    # Initialize services
    ble_manager = BLEManager()
    audio_manager = AudioManager()
    session_manager = SessionManager(ble_manager, audio_manager)
    
    # Store managers in app state
    app.state.ble_manager = ble_manager
    app.state.audio_manager = audio_manager
    app.state.session_manager = session_manager
    
    # Start background tasks
    asyncio.create_task(ble_manager.start_scanning())
    asyncio.create_task(audio_manager.start_monitoring())
    
    print("Brain Controller started successfully")
    
    yield
    
    # Shutdown
    print("Shutting down Brain Controller...")

    # Signal SSE connections to close immediately
    status.signal_shutdown()

    # Give SSE connections a brief moment to close gracefully
    await asyncio.sleep(0.1)

    await session_manager.cleanup()
    await ble_manager.cleanup()
    await audio_manager.cleanup()
    print("Brain Controller shutdown complete")


# Create FastAPI application
app = FastAPI(
    title="Shadow Warrior Brain Controller",
    description="Central controller for Shadow Warrior boxing training system",
    version="0.1.0",
    lifespan=lifespan
)

# Include API routes - single monitoring endpoint
app.include_router(status.router, prefix="/api", tags=["monitoring"])

# Mount static files
static_path = Path(__file__).parent / "static"
if static_path.exists():
    app.mount("/static", StaticFiles(directory=static_path), name="static")


@app.get("/")
async def dashboard():
    """Main dashboard interface"""
    static_path = Path(__file__).parent / "static" / "index.html"
    if static_path.exists():
        return FileResponse(static_path, media_type="text/html")
    else:
        return HTMLResponse(content="<h1>Dashboard not found</h1>", status_code=404)



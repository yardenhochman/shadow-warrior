"""
API for Arena Management
"""
from fastapi import APIRouter, Depends, Request
from pydantic import BaseModel

from shadow_warrior_brain.services.arena_manager import ArenaManager

router = APIRouter()

class PunchEvent(BaseModel):
    power: int

class ShoutEvent(BaseModel):
    intensity: float

class PresenceEvent(BaseModel):
    detected: bool

def get_arena_manager(request: Request) -> ArenaManager:
    """Get the ArenaManager instance from the app state."""
    return request.app.state.arena_manager

@router.post("/arena/punch")
async def punch(event: PunchEvent, arena_manager: ArenaManager = Depends(get_arena_manager)):
    """
    Handles a punch event.
    """
    await arena_manager.handle_punch(event.power)
    return {"status": "ok"}

@router.post("/arena/shout")
async def shout(event: ShoutEvent, arena_manager: ArenaManager = Depends(get_arena_manager)):
    """
    Handles a shout event.
    """
    await arena_manager.handle_shout(event.intensity)
    return {"status": "ok"}

@router.post("/arena/presence")
async def presence(event: PresenceEvent, arena_manager: ArenaManager = Depends(get_arena_manager)):
    """
    Handles a presence event.
    """
    await arena_manager.handle_presence(event.detected)
    return {"status": "ok"}

@router.get("/arena/state")
async def get_state(arena_manager: ArenaManager = Depends(get_arena_manager)):
    """
    Gets the current state of the arena.
    """
    return arena_manager.state_machine.get_status()

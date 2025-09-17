"""
System state monitoring API - single endpoint
"""

import time
import json
import asyncio
from datetime import datetime, timedelta
from typing import Dict, Any
from fastapi import APIRouter, Request
from fastapi.responses import StreamingResponse

from shadow_warrior_brain.models.api_responses import (
    BrainState, Statistics
)

router = APIRouter()

# Track application start time
_start_time = time.time()
_startup_timestamp = datetime.now()

# Global shutdown event for SSE connections
_shutdown_event = asyncio.Event()

# Track last state for change detection (per-connection)
# Note: This is a simple implementation. For production with many connections,
# consider using a more sophisticated state management system.


@router.get("/state", response_model=BrainState)
async def get_brain_state(request: Request) -> BrainState:
    """Get complete Brain Controller state - single monitoring endpoint"""

    # Get managers from app state
    ble_manager = getattr(request.app.state, 'ble_manager', None)
    audio_manager = getattr(request.app.state, 'audio_manager', None)
    session_manager = getattr(request.app.state, 'session_manager', None)

    # Build comprehensive state response (consistent with SSE format)
    brain_state = {
        "timestamp": datetime.now(),
        "startup_timestamp": _startup_timestamp.isoformat(),
        "system": {
            "status": "running",
            "version": "0.1.0"
        }
    }

    # Get punching bag status
    if ble_manager:
        brain_state["punching_bag"] = await ble_manager.get_punching_bag_status()
    else:
        brain_state["punching_bag"] = {"connected": False, "status": "manager_unavailable"}

    # Get audio status
    if audio_manager:
        brain_state["audio"] = await audio_manager.get_status()
    else:
        brain_state["audio"] = {"connected": False, "status": "manager_unavailable"}

    # Get session status
    if session_manager:
        brain_state["session"] = session_manager.get_session_status()
    else:
        brain_state["session"] = {"current_state": "unknown", "status": "manager_unavailable"}

    # LED status (placeholder - not implemented)
    brain_state["leds"] = {
        "connected_controllers": 0,
        "status": "not_implemented"
    }

    return BrainState(**brain_state)


def signal_shutdown():
    """Signal all SSE connections to shutdown"""
    global _shutdown_event
    _shutdown_event.set()


def extract_state_without_time_fields(state: Dict[str, Any]) -> Dict[str, Any]:
    """Extract state data excluding time-based fields for change detection"""
    state_copy = state.copy()

    # Remove time-based fields that don't represent actual state changes
    state_copy.pop('timestamp', None)

    # Remove session duration fields that change continuously
    if 'session' in state_copy and state_copy['session']:
        session = state_copy['session'].copy()
        session.pop('session_duration', None)
        state_copy['session'] = session

    # Remove timestamps from nested objects
    if 'punching_bag' in state_copy and state_copy['punching_bag']:
        pb = state_copy['punching_bag'].copy()
        if 'connection_time' in pb:
            pb.pop('connection_time', None)
        state_copy['punching_bag'] = pb

    if 'audio' in state_copy and state_copy['audio']:
        audio = state_copy['audio'].copy()
        if 'current_level' in audio and audio['current_level']:
            level = audio['current_level'].copy()
            level.pop('timestamp', None)
            audio['current_level'] = level
        state_copy['audio'] = audio

    return state_copy


async def get_state_data(request: Request) -> Dict[str, Any]:
    """Helper function to get current state data"""
    ble_manager = getattr(request.app.state, 'ble_manager', None)
    audio_manager = getattr(request.app.state, 'audio_manager', None)
    session_manager = getattr(request.app.state, 'session_manager', None)

    brain_state = {
        "timestamp": datetime.now().isoformat(),
        "startup_timestamp": _startup_timestamp.isoformat(),
        "system": {
            "status": "running",
            "version": "0.1.0"
        }
    }

    if ble_manager:
        brain_state["punching_bag"] = await ble_manager.get_punching_bag_status()
    else:
        brain_state["punching_bag"] = {"connected": False, "status": "manager_unavailable"}

    if audio_manager:
        brain_state["audio"] = await audio_manager.get_status()
    else:
        brain_state["audio"] = {"connected": False, "status": "manager_unavailable"}

    if session_manager:
        brain_state["session"] = session_manager.get_session_status()
    else:
        brain_state["session"] = {"current_state": "unknown", "status": "manager_unavailable"}

    brain_state["leds"] = {
        "connected_controllers": 0,
        "status": "not_implemented"
    }

    return brain_state


@router.get("/events")
async def stream_brain_state(request: Request):
    """Server-Sent Events stream for real-time state updates"""

    async def event_generator():
        last_state_without_uptime = None

        try:
            # Send initial state
            initial_state = await get_state_data(request)
            last_state_without_uptime = extract_state_without_time_fields(initial_state)

            event_data = json.dumps(initial_state, default=str)
            yield f"data: {event_data}\n\n"

            while True:
                # Check if client is still connected or if shutdown was signaled
                if await request.is_disconnected() or _shutdown_event.is_set():
                    break

                try:
                    # Get current state
                    current_state = await get_state_data(request)
                    current_state_for_comparison = extract_state_without_time_fields(current_state)

                    # Only send update if state has actually changed (excluding uptime/timestamps)
                    if current_state_for_comparison != last_state_without_uptime:
                        last_state_without_uptime = current_state_for_comparison

                        # Format as SSE event
                        event_data = json.dumps(current_state, default=str)
                        yield f"data: {event_data}\n\n"

                    # Wait before next check, but also check for shutdown
                    try:
                        await asyncio.wait_for(_shutdown_event.wait(), timeout=1.0)
                        # If we get here, shutdown was signaled
                        break
                    except asyncio.TimeoutError:
                        # Timeout is expected - continue the loop
                        continue

                except Exception as e:
                    # Send error event and continue
                    error_data = {"error": str(e), "timestamp": datetime.now().isoformat()}
                    yield f"data: {json.dumps(error_data)}\n\n"

        except asyncio.CancelledError:
            pass
        except Exception as e:
            # Send final error event
            error_data = {"error": str(e), "timestamp": datetime.now().isoformat()}
            yield f"data: {json.dumps(error_data)}\n\n"

    return StreamingResponse(
        event_generator(),
        media_type="text/event-stream",
        headers={
            "Cache-Control": "no-cache",
            "Connection": "keep-alive",
            "X-Accel-Buffering": "no"  # Disable nginx buffering
        }
    )


@router.get("/statistics", response_model=Statistics)
async def get_statistics(request: Request) -> Statistics:
    """Get statistics and sensor data (non-streaming)"""

    # Get managers from app state
    ble_manager = getattr(request.app.state, 'ble_manager', None)
    audio_manager = getattr(request.app.state, 'audio_manager', None)
    session_manager = getattr(request.app.state, 'session_manager', None)

    # Calculate uptime for statistics
    uptime_seconds = int(time.time() - _start_time)
    uptime_str = str(timedelta(seconds=uptime_seconds))

    statistics = {
        "timestamp": datetime.now().isoformat(),
        "uptime": uptime_str,
        "startup_timestamp": _startup_timestamp.isoformat(),
        "sensor_data": {},
        "session_statistics": {},
        "device_statistics": {}
    }

    # Get sensor data from BLE manager
    if ble_manager:
        # Get recent acceleration data
        recent_acceleration = await ble_manager.get_recent_acceleration_data()

        # Get latest acceleration reading
        latest_acceleration = None
        if recent_acceleration:
            latest = recent_acceleration[-1]
            latest_acceleration = {
                "acceleration": latest.acceleration,
                "timestamp": latest.timestamp.isoformat(),
                "x": latest.x,
                "y": latest.y,
                "z": latest.z
            }

        statistics["sensor_data"] = {
            "punching_bag": {
                "latest_acceleration": latest_acceleration,
                "recent_readings_count": len(recent_acceleration),
                "data_buffer_size": len(recent_acceleration)
            }
        }

        # Get device statistics
        if ble_manager.connected_device:
            statistics["device_statistics"] = {
                "punching_bag": {
                    "device_address": ble_manager.connected_device.address,
                    "device_name": ble_manager.connected_device.name,
                    "connection_time": ble_manager.connection_time.isoformat() if ble_manager.connection_time else None,
                    "parameters": {
                        "alpha": ble_manager.current_params.alpha,
                        "threshold": ble_manager.current_params.threshold,
                        "fight_mode": ble_manager.current_params.fight_mode
                    }
                }
            }

    # Get session statistics
    if session_manager:
        session_data = session_manager.get_session_data()
        session_status = session_manager.get_session_status()

        statistics["session_statistics"] = {
            "current_session": {
                "punch_count": session_manager.punch_count,
                "session_active": session_status.get('session_active', False),
                "session_duration": session_status.get('session_duration', 0),
                "current_state": session_status.get('current_state', 'unknown'),
                "transition_timestamp": session_status.get('transition_timestamp'),
                "state_machine_status": session_manager.state_machine.get_status()
            },
            "historical_data": session_data
        }

    # Get audio statistics (if available)
    if audio_manager:
        audio_status = await audio_manager.get_status()
        if audio_status.get('current_level'):
            statistics["sensor_data"]["audio"] = {
                "current_level": audio_status['current_level'],
                "device_name": audio_status.get('device_name', 'unknown')
            }

    return Statistics(**statistics)
"""
Event system for brain components using pub/sub pattern
"""

import asyncio
import json
from datetime import datetime
from dataclasses import dataclass, asdict
from typing import Dict, Any, Optional, Callable, Set, Union, Awaitable
from enum import Enum

from shadow_warrior_brain.core.logging_config import get_logger

logger = get_logger(__name__)


class EventType(Enum):
    """Types of events that can be emitted"""
    # Audio events
    AUDIO_DEVICE_CONNECTED = "audio.device.connected"
    AUDIO_DEVICE_DISCONNECTED = "audio.device.disconnected"
    AUDIO_MONITORING_STARTED = "audio.monitoring.started"
    AUDIO_MONITORING_STOPPED = "audio.monitoring.stopped"
    AUDIO_SHOUT_DETECTED = "audio.shout.detected"
    AUDIO_LEVEL_CHANGED = "audio.level.changed"

    # BLE events
    BLE_DEVICE_CONNECTED = "ble.device.connected"
    BLE_DEVICE_DISCONNECTED = "ble.device.disconnected"
    BLE_DATA_RECEIVED = "ble.data.received"
    BLE_CONNECTION_ERROR = "ble.connection.error"

    # Session events
    SESSION_STATE_CHANGED = "session.state.changed"
    SESSION_STARTED = "session.started"
    SESSION_ENDED = "session.ended"
    SESSION_PUNCH_DETECTED = "session.punch.detected"

    # System events
    SYSTEM_STARTUP = "system.startup"
    SYSTEM_SHUTDOWN = "system.shutdown"
    SYSTEM_ERROR = "system.error"


@dataclass
class Event:
    """Base event structure"""
    type: EventType
    timestamp: datetime
    source: str
    data: Dict[str, Any]

    def __post_init__(self):
        if not isinstance(self.timestamp, datetime):
            self.timestamp = datetime.now()

    def to_dict(self) -> Dict[str, Any]:
        """Convert event to dictionary for serialization"""
        return {
            "type": self.type.value,
            "timestamp": self.timestamp.isoformat(),
            "source": self.source,
            "data": self.data
        }

    def to_json(self) -> str:
        """Convert event to JSON string"""
        return json.dumps(self.to_dict(), default=str)


class EventBus:
    """Central event bus for pub/sub pattern"""

    def __init__(self):
        self._subscribers: Dict[EventType, Set[Union[Callable[[Event], None], Callable[[Event], Awaitable[None]]]]] = {}
        self._global_subscribers: Set[Union[Callable[[Event], None], Callable[[Event], Awaitable[None]]]] = set()
        self._lock = asyncio.Lock()
        self.event_count = 0

    async def subscribe(self, event_type: EventType, callback: Union[Callable[[Event], None], Callable[[Event], Awaitable[None]]]):
        """Subscribe to specific event type"""
        async with self._lock:
            if event_type not in self._subscribers:
                self._subscribers[event_type] = set()
            self._subscribers[event_type].add(callback)
            logger.debug("Subscribed to %s events (total subscribers: %d)",
                        event_type.value, len(self._subscribers[event_type]))

    async def subscribe_all(self, callback: Union[Callable[[Event], None], Callable[[Event], Awaitable[None]]]):
        """Subscribe to all events"""
        async with self._lock:
            self._global_subscribers.add(callback)
            logger.debug("Added global subscriber (total: %d)", len(self._global_subscribers))

    async def unsubscribe(self, event_type: EventType, callback: Union[Callable[[Event], None], Callable[[Event], Awaitable[None]]]):
        """Unsubscribe from specific event type"""
        async with self._lock:
            if event_type in self._subscribers:
                self._subscribers[event_type].discard(callback)
                if not self._subscribers[event_type]:
                    del self._subscribers[event_type]
                logger.debug("Unsubscribed from %s events", event_type.value)

    async def unsubscribe_all(self, callback: Union[Callable[[Event], None], Callable[[Event], Awaitable[None]]]):
        """Unsubscribe from all events"""
        async with self._lock:
            self._global_subscribers.discard(callback)
            logger.debug("Removed global subscriber")

    async def emit(self, event: Event):
        """Emit an event to all subscribers"""
        self.event_count += 1

        # Log event emission
        logger.debug("Emitting event: %s from %s", event.type.value, event.source)

        async with self._lock:
            # Notify global subscribers
            for callback in self._global_subscribers.copy():
                try:
                    if asyncio.iscoroutinefunction(callback):
                        await callback(event)
                    else:
                        callback(event)
                except Exception as e:
                    logger.error("Error in global subscriber callback: %s", e)

            # Notify specific event type subscribers
            if event.type in self._subscribers:
                for callback in self._subscribers[event.type].copy():
                    try:
                        if asyncio.iscoroutinefunction(callback):
                            await callback(event)
                        else:
                            callback(event)
                    except Exception as e:
                        logger.error("Error in %s subscriber callback: %s", event.type.value, e)

    async def emit_event(self, event_type: EventType, source: str, data: Dict[str, Any] = None):
        """Convenience method to create and emit an event"""
        if data is None:
            data = {}

        event = Event(
            type=event_type,
            timestamp=datetime.now(),
            source=source,
            data=data
        )
        await self.emit(event)

    def get_stats(self) -> Dict[str, Any]:
        """Get event bus statistics"""
        subscriber_counts = {
            event_type.value: len(callbacks)
            for event_type, callbacks in self._subscribers.items()
        }

        return {
            "total_events_emitted": self.event_count,
            "global_subscribers": len(self._global_subscribers),
            "type_specific_subscribers": subscriber_counts,
            "total_subscription_types": len(self._subscribers)
        }


# Global event bus instance
event_bus = EventBus()


def create_audio_event(event_type: EventType, source: str = "audio_manager", **kwargs) -> Event:
    """Helper to create audio-related events"""
    return Event(
        type=event_type,
        timestamp=datetime.now(),
        source=source,
        data=kwargs
    )


def create_ble_event(event_type: EventType, source: str = "ble_manager", **kwargs) -> Event:
    """Helper to create BLE-related events"""
    return Event(
        type=event_type,
        timestamp=datetime.now(),
        source=source,
        data=kwargs
    )


def create_session_event(event_type: EventType, source: str = "session_manager", **kwargs) -> Event:
    """Helper to create session-related events"""
    return Event(
        type=event_type,
        timestamp=datetime.now(),
        source=source,
        data=kwargs
    )


def create_system_event(event_type: EventType, source: str = "system", **kwargs) -> Event:
    """Helper to create system-related events"""
    return Event(
        type=event_type,
        timestamp=datetime.now(),
        source=source,
        data=kwargs
    )
"""
Statistics and sensor data models
"""

from dataclasses import dataclass
from datetime import datetime
from typing import List, Dict, Any, Optional


@dataclass
class SensorReading:
    """Individual sensor reading with timestamp"""
    value: float
    timestamp: datetime
    sensor_type: str = "acceleration"

    def __post_init__(self):
        if not isinstance(self.timestamp, datetime):
            self.timestamp = datetime.now()


@dataclass
class SessionStatistics:
    """Statistics for a training session"""
    session_id: str
    start_time: datetime
    end_time: Optional[datetime] = None
    total_punches: int = 0
    states_visited: List[str] = None
    fight_count: int = 0
    total_fight_time: float = 0.0
    average_punches_per_fight: float = 0.0
    transitions: List[Dict[str, Any]] = None

    def __post_init__(self):
        if self.states_visited is None:
            self.states_visited = []
        if self.transitions is None:
            self.transitions = []

    @property
    def duration(self) -> Optional[float]:
        """Get session duration in seconds"""
        if self.end_time:
            return (self.end_time - self.start_time).total_seconds()
        return (datetime.now() - self.start_time).total_seconds()

    @property
    def is_active(self) -> bool:
        """Check if session is currently active"""
        return self.end_time is None


@dataclass
class DeviceStatistics:
    """Statistics for device usage and performance"""
    device_address: str
    device_name: str
    connection_count: int = 0
    total_connection_time: float = 0.0
    last_connection: Optional[datetime] = None
    sensor_readings_count: int = 0
    parameter_changes: int = 0

    @property
    def average_connection_duration(self) -> float:
        """Get average connection duration in seconds"""
        if self.connection_count > 0:
            return self.total_connection_time / self.connection_count
        return 0.0
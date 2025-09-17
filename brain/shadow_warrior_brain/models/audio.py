"""
Audio device models
"""

from dataclasses import dataclass
from datetime import datetime
from enum import Enum
from typing import Optional


class AudioStatus(Enum):
    """Status of audio device"""
    INACTIVE = "inactive"
    ACTIVE = "active"
    ERROR = "error"


@dataclass
class AudioLevel:
    """Audio level data"""
    level: float
    timestamp: datetime

    def __post_init__(self):
        if not isinstance(self.timestamp, datetime):
            self.timestamp = datetime.now()


@dataclass
class AudioDevice:
    """Represents an audio input device"""
    name: str
    device_id: Optional[str] = None
    status: AudioStatus = AudioStatus.INACTIVE
    sample_rate: int = 44100
    channels: int = 1

    def __str__(self) -> str:
        return f"AudioDevice({self.name}, {self.status.value})"
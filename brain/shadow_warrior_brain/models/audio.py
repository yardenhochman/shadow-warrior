"""
Audio device models
"""

from dataclasses import dataclass
from datetime import datetime
from enum import Enum


class AudioStatus(Enum):
    """Status of audio device"""
    INACTIVE = "inactive"
    ACTIVE = "active"
    ERROR = "error"


@dataclass
class AudioLevel:
    """Audio level data"""
    timestamp: datetime
    level_db: float
    peak_db: float
    is_clipping: bool = False

    def __post_init__(self):
        if not isinstance(self.timestamp, datetime):
            self.timestamp = datetime.now()

    def dict(self):
        """Convert to dictionary for JSON serialization"""
        return {
            "timestamp": self.timestamp.isoformat(),
            "level_db": self.level_db,
            "peak_db": self.peak_db,
            "is_clipping": self.is_clipping
        }


@dataclass
class AudioDevice:
    """Represents an audio input device"""
    device_id: int
    name: str
    channels: int
    sample_rate: int
    is_default: bool = False
    is_selected: bool = False
    status: AudioStatus = AudioStatus.INACTIVE

    def __str__(self) -> str:
        return f"AudioDevice({self.name}, {self.status.value})"
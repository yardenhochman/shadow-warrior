"""
Punching bag device models
"""

from dataclasses import dataclass
from datetime import datetime
from enum import Enum
from typing import Optional


class PunchingBagStatus(Enum):
    """Status of punching bag device"""
    DISCONNECTED = "disconnected"
    CONNECTED = "connected"
    ACTIVE = "active"
    ERROR = "error"


@dataclass
class AccelerationData:
    """Acceleration data from IMU sensor"""
    acceleration: float
    timestamp: datetime
    x: Optional[float] = None
    y: Optional[float] = None
    z: Optional[float] = None

    def __post_init__(self):
        if not isinstance(self.timestamp, datetime):
            self.timestamp = datetime.now()


@dataclass
class PunchingBagParams:
    """Configuration parameters for punching bag"""
    alpha: float = 0.8
    threshold: float = 2.0
    fight_mode: bool = False


@dataclass
class PunchingBagDevice:
    """Represents a punching bag BLE device"""
    name: str
    address: str
    rssi: Optional[int] = None
    status: PunchingBagStatus = PunchingBagStatus.DISCONNECTED

    def __str__(self) -> str:
        return f"PunchingBag({self.name}, {self.address}, {self.status.value})"
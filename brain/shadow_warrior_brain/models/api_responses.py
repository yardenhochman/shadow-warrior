"""
API Response Models using Pydantic for schema validation and documentation
"""

from datetime import datetime
from typing import Dict, List, Optional, Any
from pydantic import BaseModel, Field, ConfigDict


class SystemInfo(BaseModel):
    """System information model"""
    status: str = Field(description="System status")
    version: str = Field(description="System version")


class PunchingBagStatus(BaseModel):
    """Punching bag status model"""
    model_config = ConfigDict(extra='allow')

    connected: bool = Field(description="Whether punching bag is connected")
    device_address: Optional[str] = Field(None, description="BLE device address")
    device_name: Optional[str] = Field(None, description="Device name")
    fight_mode: bool = Field(default=False, description="Whether fight mode is enabled")
    connection_time: Optional[datetime] = Field(None, description="Connection timestamp")
    parameters: Optional[Dict[str, Any]] = Field(None, description="Device parameters")
    status: Optional[str] = Field(None, description="Status message when unavailable")


class AudioStatus(BaseModel):
    """Audio status model"""
    model_config = ConfigDict(extra='allow')

    connected: bool = Field(description="Whether audio is connected")
    device_name: Optional[str] = Field(None, description="Audio device name")
    current_level: Optional[Dict[str, Any]] = Field(None, description="Current audio level data")
    status: Optional[str] = Field(None, description="Audio status message")


class SessionStatus(BaseModel):
    """Session status model"""
    model_config = ConfigDict(extra='allow')

    current_state: str = Field(description="Current session state")
    transition_timestamp: Optional[str] = Field(None, description="ISO timestamp of last state transition")
    valid_transitions: List[str] = Field(default_factory=list, description="List of valid state transitions")
    session_active: bool = Field(default=False, description="Whether a session is active")
    session_duration: Optional[float] = Field(None, description="Session duration in seconds")
    status: Optional[str] = Field(None, description="Status message when unavailable")


class LEDStatus(BaseModel):
    """LED status model"""
    connected_controllers: int = Field(description="Number of connected LED controllers")
    status: str = Field(description="LED system status")


class BrainState(BaseModel):
    """Complete brain controller state response"""
    model_config = ConfigDict(extra='allow')

    timestamp: datetime = Field(description="Current timestamp")
    startup_timestamp: str = Field(description="Server startup timestamp")
    system: SystemInfo = Field(description="System information")
    punching_bag: PunchingBagStatus = Field(description="Punching bag status")
    audio: AudioStatus = Field(description="Audio status")
    session: SessionStatus = Field(description="Session status")
    leds: LEDStatus = Field(description="LED status")


class AccelerationReading(BaseModel):
    """Individual acceleration reading"""
    acceleration: float = Field(description="Acceleration magnitude")
    timestamp: str = Field(description="Reading timestamp")
    x: Optional[float] = Field(None, description="X-axis acceleration")
    y: Optional[float] = Field(None, description="Y-axis acceleration")
    z: Optional[float] = Field(None, description="Z-axis acceleration")


class SensorData(BaseModel):
    """Sensor data container"""
    punching_bag: Optional[Dict[str, Any]] = Field(None, description="Punching bag sensor data")
    audio: Optional[Dict[str, Any]] = Field(None, description="Audio sensor data")


class CurrentSessionStats(BaseModel):
    """Current session statistics"""
    punch_count: int = Field(description="Number of punches in current session")
    session_active: bool = Field(description="Whether session is active")
    session_duration: float = Field(description="Session duration in seconds")
    current_state: str = Field(description="Current session state")
    transition_timestamp: Optional[str] = Field(None, description="Last state transition timestamp")
    state_machine_status: Dict[str, Any] = Field(description="State machine status details")


class SessionStatistics(BaseModel):
    """Session statistics container"""
    current_session: CurrentSessionStats = Field(description="Current session statistics")
    historical_data: Dict[str, Any] = Field(description="Historical session data")


class DeviceStatistics(BaseModel):
    """Device statistics container"""
    punching_bag: Optional[Dict[str, Any]] = Field(None, description="Punching bag device statistics")


class Statistics(BaseModel):
    """Complete statistics response"""
    timestamp: str = Field(description="Response timestamp")
    uptime: str = Field(description="Server uptime")
    startup_timestamp: str = Field(description="Server startup timestamp")
    sensor_data: SensorData = Field(description="Current sensor data")
    session_statistics: SessionStatistics = Field(description="Session statistics")
    device_statistics: DeviceStatistics = Field(description="Device statistics")


class ErrorResponse(BaseModel):
    """Error response model"""
    error: str = Field(description="Error message")
    timestamp: str = Field(description="Error timestamp")
    details: Optional[Dict[str, Any]] = Field(None, description="Additional error details")
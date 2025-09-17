"""
Audio interface manager for microphone integration
"""

import asyncio
from datetime import datetime
from typing import List, Optional, Dict, Any
from collections import deque

from shadow_warrior_brain.models.audio import AudioDevice, AudioStatus, AudioLevel
from shadow_warrior_brain.core.config import settings


class AudioManager:
    """Manages audio input interface"""
    
    def __init__(self):
        self.selected_device: Optional[AudioDevice] = None
        self.monitoring = False
        self.audio_levels: deque = deque(maxlen=settings.audio_level_retention)
        self.current_level: Optional[AudioLevel] = None
        
        # Audio system status
        self._available_devices: List[AudioDevice] = []
        
    async def start_monitoring(self):
        """Start background audio monitoring"""
        self.monitoring = True
        print("Audio Manager: Started monitoring")
        
    async def cleanup(self):
        """Cleanup audio resources"""
        self.monitoring = False
        print("Audio Manager: Cleanup complete")
        
    async def get_available_devices(self) -> List[AudioDevice]:
        """Get list of available audio input devices"""
        # TODO: Implement actual audio device enumeration
        # This is a placeholder that simulates available devices
        
        placeholder_devices = [
            AudioDevice(
                device_id=0,
                name="Built-in Microphone",
                channels=1,
                sample_rate=44100,
                is_default=True,
                is_selected=self.selected_device and self.selected_device.device_id == 0
            ),
            AudioDevice(
                device_id=1,
                name="USB Audio Device",
                channels=2,
                sample_rate=48000,
                is_default=False,
                is_selected=self.selected_device and self.selected_device.device_id == 1
            )
        ]
        
        self._available_devices = placeholder_devices
        return placeholder_devices
        
    async def select_device(self, device_id: int) -> bool:
        """Select audio input device"""
        devices = await self.get_available_devices()
        device = next((d for d in devices if d.device_id == device_id), None)
        
        if device:
            # Mark previous device as not selected
            if self.selected_device:
                self.selected_device.is_selected = False
                
            self.selected_device = device
            self.selected_device.is_selected = True
            print(f"Selected audio device: {device.name}")
            return True
            
        return False
        
    async def get_status(self) -> Dict[str, Any]:
        """Get current audio system status"""
        return {
            "connected": self.selected_device is not None,
            "device_name": self.selected_device.name if self.selected_device else None,
            "device_id": self.selected_device.device_id if self.selected_device else None,
            "sample_rate": self.selected_device.sample_rate if self.selected_device else None,
            "channels": self.selected_device.channels if self.selected_device else None,
            "monitoring": self.monitoring,
            "current_level": self.current_level.dict() if self.current_level else None
        }
        
    async def get_current_level(self) -> AudioLevel:
        """Get current audio input level"""
        # TODO: Implement actual audio level monitoring
        # This is a placeholder that simulates audio levels
        
        import random
        import math
        
        # Simulate varying audio levels
        base_level = -40 + random.uniform(-10, 10)
        peak_level = base_level + random.uniform(0, 10)
        
        level = AudioLevel(
            timestamp=datetime.now(),
            level_db=base_level,
            peak_db=peak_level,
            is_clipping=peak_level > -3
        )
        
        self.current_level = level
        self.audio_levels.append(level)
        
        return level
        
    async def start_monitoring(self):
        """Start audio level monitoring"""
        if not self.monitoring:
            self.monitoring = True
            # Start background task for continuous monitoring
            asyncio.create_task(self._monitor_audio_levels())
            print("Audio monitoring started")
            
    async def stop_monitoring(self):
        """Stop audio level monitoring"""
        self.monitoring = False
        print("Audio monitoring stopped")
        
    async def _monitor_audio_levels(self):
        """Background task for continuous audio level monitoring"""
        while self.monitoring:
            if self.selected_device:
                await self.get_current_level()
            await asyncio.sleep(0.1)  # 10Hz update rate
            
    async def get_recent_levels(self) -> List[AudioLevel]:
        """Get recent audio level measurements"""
        return list(self.audio_levels)
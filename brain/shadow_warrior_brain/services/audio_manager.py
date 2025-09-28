"""
Audio interface manager for microphone integration
"""

import numpy as np
import pyaudio
import threading
import asyncio
from datetime import datetime
from typing import List, Optional, Dict, Any
from collections import deque

from shadow_warrior_brain.models.audio import AudioDevice, AudioLevel, AudioStatus
from shadow_warrior_brain.core.config import settings
from shadow_warrior_brain.core.logging_config import get_logger
from shadow_warrior_brain.core.events import event_bus, EventType

logger = get_logger(__name__)


class AudioManager:
    """Manages audio input interface"""

    def __init__(self):
        self.selected_device: Optional[AudioDevice] = None
        self.monitoring = False
        self.audio_levels: deque = deque(maxlen=settings.audio_level_retention)
        self.current_level: Optional[AudioLevel] = None

        # Audio system status
        self._available_devices: List[AudioDevice] = []
        self._pyaudio_instance: Optional[pyaudio.PyAudio] = None
        self._audio_stream: Optional[pyaudio.Stream] = None
        self._audio_thread: Optional[threading.Thread] = None
        self._stop_recording = threading.Event()

        # Shouting detection
        self.shout_score = 0.0
        self.shout_start_time: Optional[datetime] = None
        self.is_shouting = False
        self.last_shout_event: Optional[Dict[str, Any]] = None

        # Monitoring statistics
        self._samples_processed = 0
        self._last_status_log = None

        # State tracking for events
        self._last_monitoring_state = False
        self._last_device_id: Optional[int] = None
        
        
    async def cleanup(self):
        """Cleanup audio resources"""
        logger.info("Audio Manager: Starting cleanup...")
        await self.stop_monitoring()

        if self._audio_stream:
            logger.info("Audio Manager: Cleaning up remaining audio stream")
            self._audio_stream.stop_stream()
            self._audio_stream.close()

        if self._pyaudio_instance:
            logger.info("Audio Manager: Terminating PyAudio instance")
            self._pyaudio_instance.terminate()
            self._pyaudio_instance = None

        # Reset counters
        self._samples_processed = 0
        self.shout_score = 0.0
        self.is_shouting = False
        self.last_shout_event = None
        self._last_monitoring_state = False
        self._last_device_id = None

        logger.info("Audio Manager: Cleanup complete")
        
    async def get_available_devices(self) -> List[AudioDevice]:
        """Get list of available audio input devices"""
        if not self._pyaudio_instance:
            self._pyaudio_instance = pyaudio.PyAudio()
            logger.info("Audio Manager: Initialized PyAudio instance")

        devices = []
        default_device_id = self._pyaudio_instance.get_default_input_device_info()['index']
        logger.info("Audio Manager: Enumerating audio devices (default: %s)", default_device_id)

        for i in range(self._pyaudio_instance.get_device_count()):
            try:
                device_info = self._pyaudio_instance.get_device_info_by_index(i)
                max_input_channels = device_info.get('maxInputChannels', 0)
                if isinstance(max_input_channels, (int, float)) and max_input_channels > 0:
                    device_name = str(device_info.get('name', f'Device {i}'))
                    device_channels = int(min(max_input_channels, settings.audio_channels))
                    device_sample_rate = int(device_info.get('defaultSampleRate', settings.audio_sample_rate))
                    is_selected = bool(self.selected_device and self.selected_device.device_id == i)

                    device = AudioDevice(
                        device_id=i,
                        name=device_name,
                        channels=device_channels,
                        sample_rate=device_sample_rate,
                        is_default=(i == default_device_id),
                        is_selected=is_selected,
                        status=AudioStatus.INACTIVE
                    )
                    devices.append(device)
            except Exception as e:
                logger.warning("Error reading device %d: %s", i, e)

        self._available_devices = devices
        logger.info("Audio Manager: Found %d input devices", len(devices))
        return devices
        
    async def select_device(self, device_id: int) -> bool:
        """Select audio input device"""
        # Stop current monitoring if active
        if self.monitoring:
            await self.stop_monitoring()

        devices = await self.get_available_devices()
        device = next((d for d in devices if d.device_id == device_id), None)

        if device:
            # Mark previous device as not selected
            if self.selected_device:
                self.selected_device.is_selected = False

            self.selected_device = device
            self.selected_device.is_selected = True
            self.selected_device.status = AudioStatus.ACTIVE
            logger.info("Audio Manager: Selected device '%s' (ID: %d, %dch, %dHz)", device.name, device.device_id, device.channels, device.sample_rate)

            # Emit device connected event if it's a new device
            if self._last_device_id != device.device_id:
                await event_bus.emit_event(
                    EventType.AUDIO_DEVICE_CONNECTED,
                    "audio_manager",
                    {
                        "device_id": device.device_id,
                        "device_name": device.name,
                        "channels": device.channels,
                        "sample_rate": device.sample_rate
                    }
                )
                self._last_device_id = device.device_id

            return True

        return False
        
    async def get_status(self) -> Dict[str, Any]:
        """Get current audio system status"""
        # Include current audio level if available
        current_level = None
        if self.current_level:
            current_level = {
                "level_db": self.current_level.level_db,
                "peak_db": self.current_level.peak_db,
                "is_clipping": self.current_level.is_clipping,
                "timestamp": self.current_level.timestamp.isoformat()
            }

        return {
            "connected": self.selected_device is not None,
            "device_name": self.selected_device.name if self.selected_device else None,
            "device_id": self.selected_device.device_id if self.selected_device else None,
            "sample_rate": self.selected_device.sample_rate if self.selected_device else None,
            "channels": self.selected_device.channels if self.selected_device else None,
            "monitoring": self.monitoring,
            "current_level": current_level,
            "shout_score": self.shout_score,
            "is_shouting": self.is_shouting,
            "last_shout_event": self.last_shout_event
        }
        
    async def get_current_level(self) -> AudioLevel:
        """Get current audio input level"""
        if self.current_level:
            return self.current_level

        # Return a default level if no audio is being captured
        level = AudioLevel(
            timestamp=datetime.now(),
            level_db=-60.0,
            peak_db=-60.0,
            is_clipping=False
        )
        return level
        
    async def start_monitoring(self):
        """Start audio level monitoring"""
        if self.monitoring:
            return

        if not self.selected_device:
            # Auto-select device based on config or default
            logger.info("Audio Manager: Auto-selecting audio device for monitoring")
            devices = await self.get_available_devices()
            if settings.audio_device_id >= 0:
                logger.info("Audio Manager: Looking for configured device ID: %d", settings.audio_device_id)
                target_device = next((d for d in devices if d.device_id == settings.audio_device_id), None)
            else:
                logger.info("Audio Manager: Using default audio device")
                target_device = next((d for d in devices if d.is_default), None)

            if target_device:
                await self.select_device(target_device.device_id)
            else:
                logger.error("Audio Manager: No audio device available for monitoring")
                return

        if not self._pyaudio_instance or not self.selected_device:
            logger.error("No PyAudio instance or selected device available")
            return

        try:
            self._stop_recording.clear()
            self.monitoring = True

            # Create audio stream
            self._audio_stream = self._pyaudio_instance.open(
                format=pyaudio.paFloat32,
                channels=self.selected_device.channels,
                rate=self.selected_device.sample_rate,
                input=True,
                input_device_index=self.selected_device.device_id,
                frames_per_buffer=settings.audio_buffer_size
            )

            # Start audio processing thread
            self._audio_thread = threading.Thread(target=self._audio_processing_thread)
            self._audio_thread.start()

            logger.info("Audio Manager: Monitoring started on '%s' (buffer: %d, threshold: %.1fdB)", self.selected_device.name, settings.audio_buffer_size, settings.shout_threshold_db)

            # Emit monitoring started event
            if not self._last_monitoring_state:
                await event_bus.emit_event(
                    EventType.AUDIO_MONITORING_STARTED,
                    "audio_manager",
                    {
                        "device_name": self.selected_device.name,
                        "device_id": self.selected_device.device_id,
                        "buffer_size": settings.audio_buffer_size,
                        "threshold_db": settings.shout_threshold_db
                    }
                )
                self._last_monitoring_state = True

        except Exception as e:
            logger.error("Audio Manager: Failed to start monitoring: %s", e)
            self.monitoring = False
            if self.selected_device:
                self.selected_device.status = AudioStatus.ERROR
            
    async def stop_monitoring(self):
        """Stop audio level monitoring"""
        if not self.monitoring:
            logger.debug("Audio Manager: Stop monitoring called but not currently monitoring")
            return

        logger.info("Audio Manager: Stopping monitoring (processed %d samples)", self._samples_processed)
        self.monitoring = False
        self._stop_recording.set()

        if self._audio_thread and self._audio_thread.is_alive():
            logger.info("Audio Manager: Waiting for audio thread to stop...")
            self._audio_thread.join(timeout=1.0)
            if self._audio_thread.is_alive():
                logger.warning("Audio Manager: Audio thread did not stop within timeout")
            else:
                logger.info("Audio Manager: Audio thread stopped successfully")

        if self._audio_stream:
            logger.info("Audio Manager: Closing audio stream")
            self._audio_stream.stop_stream()
            self._audio_stream.close()
            self._audio_stream = None

        if self.selected_device:
            self.selected_device.status = AudioStatus.INACTIVE
            logger.info("Audio Manager: Released device '%s'", self.selected_device.name)

        # Emit monitoring stopped event
        if self._last_monitoring_state:
            await event_bus.emit_event(
                EventType.AUDIO_MONITORING_STOPPED,
                "audio_manager",
                {"samples_processed": self._samples_processed}
            )
            self._last_monitoring_state = False

        logger.info("Audio Manager: Monitoring stopped successfully")
        
    def _audio_processing_thread(self):
        """Audio processing thread for real-time monitoring"""
        logger.info("Audio Manager: Audio processing thread started")
        status_log_interval = settings.audio_status_log_interval
        self._last_status_log = datetime.now()

        while not self._stop_recording.is_set() and self.monitoring:
            try:
                if not self._audio_stream:
                    break

                # Read audio data
                audio_data = self._audio_stream.read(
                    settings.audio_buffer_size,
                    exception_on_overflow=False
                )

                # Convert to numpy array
                audio_array = np.frombuffer(audio_data, dtype=np.float32)

                # Calculate RMS level
                rms = np.sqrt(np.mean(audio_array**2))

                # Convert to dB
                if rms > 0:
                    level_db = 20 * np.log10(rms)
                    peak_db = 20 * np.log10(np.max(np.abs(audio_array)))
                else:
                    level_db = -60.0  # Silence threshold
                    peak_db = -60.0

                # Create audio level
                level = AudioLevel(
                    timestamp=datetime.now(),
                    level_db=float(level_db),
                    peak_db=float(peak_db),
                    is_clipping=(peak_db > -3.0)
                )

                self.current_level = level
                self.audio_levels.append(level)

                # Process shouting detection
                self._process_shouting_detection(level)

                # Update statistics
                self._samples_processed += 1

                # Periodic status logging
                current_time = datetime.now()
                if (current_time - self._last_status_log).total_seconds() >= status_log_interval:
                    logger.info(
                        "Audio Manager: Monitoring Status - Level: %.1fdB, Peak: %.1fdB, Score: %.2f, Samples: %d, Active: %s",
                        level.level_db, level.peak_db, self.shout_score, self._samples_processed, self.is_shouting
                    )
                    self._last_status_log = current_time

            except Exception as e:
                logger.error("Audio Manager: Audio processing error: %s", e)
                break

        logger.info("Audio Manager: Audio processing thread stopped (processed %d samples)", self._samples_processed)
            
    def _process_shouting_detection(self, level: AudioLevel):
        """Process shouting detection and scoring"""
        current_time = level.timestamp
        is_loud = level.level_db > settings.shout_threshold_db

        if is_loud:
            if not self.is_shouting:
                # Start of potential shout
                self.shout_start_time = current_time
                self.is_shouting = True
                logger.info("Audio Manager: Potential shout started (level: %.1fdB > %.1fdB)", level.level_db, settings.shout_threshold_db)

            # Calculate strength multiplier based on how much above threshold
            strength = min(2.0, (level.level_db - settings.shout_threshold_db) / 10.0 + 1.0)
            self.shout_score += settings.shout_score_boost * strength

        else:
            if self.is_shouting and self.shout_start_time:
                # End of shout - check if it was long enough
                duration = (current_time - self.shout_start_time).total_seconds()
                if duration >= settings.shout_min_duration:
                    # Generate shout event
                    strength = min(2.0, (level.peak_db - settings.shout_threshold_db) / 10.0 + 1.0)
                    shout_event = {
                        "type": "shout_detected",
                        "timestamp": current_time.isoformat(),
                        "duration": duration,
                        "peak_score": self.shout_score,
                        "strength": strength
                    }
                    self.last_shout_event = shout_event
                    logger.info("Audio Manager: SHOUT DETECTED - Duration: %.2fs, Score: %.2f, Strength: %.2f, Peak: %.1fdB", duration, self.shout_score, strength, level.peak_db)

                    # Emit shout detected event
                    asyncio.create_task(event_bus.emit_event(
                        EventType.AUDIO_SHOUT_DETECTED,
                        "audio_manager",
                        {
                            "duration": duration,
                            "peak_score": self.shout_score,
                            "strength": strength,
                            "peak_db": level.peak_db,
                            "timestamp": current_time.isoformat()
                        }
                    ))
                else:
                    logger.debug("Audio Manager: Shout too short (%.2fs < %.1fs)", duration, settings.shout_min_duration)

                self.is_shouting = False
                self.shout_start_time = None

        # Apply score decay
        self.shout_score *= settings.shout_score_decay
        self.shout_score = max(0.0, self.shout_score)


    def reset_shout_score(self):
        """Reset the shouting score to zero"""
        self.shout_score = 0.0
        self.is_shouting = False
        self.shout_start_time = None
        logger.info("Shout score reset")

    def get_and_clear_shout_event(self) -> Optional[Dict[str, Any]]:
        """Get the last shout event and clear it (consume once)"""
        event = self.last_shout_event
        self.last_shout_event = None
        return event


"""
Application configuration
"""

import yaml
from typing import List

from pydantic_settings import BaseSettings

class Settings(BaseSettings):
    """Application settings"""
    
    # Server configuration
    host: str = "0.0.0.0"
    port: int = 8000
    debug: bool = False
    
    # BLE configuration
    ble_scan_timeout: float = 5.0
    punching_bag_service_uuid: str = "6E400001-B5A3-F393-E0A9-E50E24DCCA9E"
    ble_auto_connect: bool = True
    ble_retry_interval: float = 10.0  # seconds between connection attempts
    ble_max_retry_attempts: int = 0   # 0 = infinite retries
    ble_connection_timeout: float = 10.0  # seconds to wait for connection
    
    # Audio configuration
    audio_sample_rate: int = 44100
    audio_channels: int = 1
    audio_buffer_size: int = 1024
    audio_device_id: int = -1  # -1 for default device
    audio_update_rate: float = 0.1  # seconds between audio level updates
    audio_status_log_interval: float = 30.0  # seconds between status logs

    # Shouting detection configuration
    shout_threshold_db: float = -15.0  # dB threshold for detecting shouting
    shout_min_duration: float = 0.5  # minimum duration in seconds for valid shout
    shout_score_decay: float = 0.95  # score decay factor per update
    shout_score_boost: float = 1.0  # base score increment per shout sample
    
    # LED configuration (for future use)
    led_update_rate: int = 30  # Hz
    max_led_controllers: int = 4
    
    # Data retention
    acceleration_data_retention: int = 1000  # Number of recent samples to keep
    audio_level_retention: int = 100  # Number of recent audio levels to keep

    # Arena configuration
    wled_controllers: List[str] = []
    tasmota_plugs: List[str] = []
    
    class Config:
        env_file = ".env"
        case_sensitive = False

def load_settings() -> Settings:
    """Load settings from .env and config.yaml."""
    yaml_config = {}
    try:
        with open("config.yaml", "r") as f:
            yaml_config = yaml.safe_load(f)
    except FileNotFoundError:
        pass  # It's okay if the file doesn't exist

    return Settings(**yaml_config)

settings = load_settings()

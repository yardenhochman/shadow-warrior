"""
Application configuration
"""

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
    
    # LED configuration (for future use)
    led_update_rate: int = 30  # Hz
    max_led_controllers: int = 4
    
    # Data retention
    acceleration_data_retention: int = 1000  # Number of recent samples to keep
    audio_level_retention: int = 100  # Number of recent audio levels to keep
    
    class Config:
        env_file = ".env"
        case_sensitive = False


# Global settings instance
settings = Settings()
"""
WLED Controller Manager

Controls WLED controllers via HTTP and UDP.
"""
import asyncio
import socket
from typing import List

import httpx

from shadow_warrior_brain.core.logging_config import get_logger

logger = get_logger(__name__)

class WledManager:
    """
    Manages WLED controllers.
    """
    def __init__(self, wled_ips: List[str]):
        self.wled_ips = wled_ips
        self.http_client = httpx.AsyncClient()
        self.udp_ports = {ip: 21324 for ip in wled_ips}  # Default WLED UDP port
        self.udp_sockets = {
            ip: socket.socket(socket.AF_INET, socket.SOCK_DGRAM) for ip in wled_ips
        }

    async def set_breathing_effect(self):
        """Set all WLEDs to a breathing effect."""
        logger.info("Setting breathing effect on all WLEDs.")
        for ip in self.wled_ips:
            await self._set_effect(ip, effect_name="Breathe", color_palette="*random", speed=30, intensity=200)

    async def _set_effect(self, ip: str, effect_name: str, color_palette: str, speed: int, intensity: int):
        """
        Set an effect on a WLED controller via HTTP.
        """
        try:
            url = f"http://{ip}/json/state"
            payload = {
                "on": True,
                "bri": 255,
                "transition": 7,
                "ps": -1,
                "pl": -1,
                "seg": [{
                    "id": 0,
                    "fx": self._get_effect_id(effect_name),
                    "sx": speed,
                    "ix": intensity,
                    "pal": self._get_palette_id(color_palette),
                }]
            }
            response = await self.http_client.post(url, json=payload, timeout=5.0)
            response.raise_for_status()
            logger.debug(f"Set effect '{effect_name}' on {ip}")
        except httpx.RequestError as e:
            logger.error(f"Error setting effect on {ip}: {e}")

    def send_realtime_frame(self, ip: str, frame: List[tuple[int, int, int]]):
        """
        Send a real-time frame to a WLED controller via UDP.

        Args:
            ip: The IP address of the WLED controller.
            frame: A list of (r, g, b) tuples.
        """
        # UDP packet structure for WLED:
        # Byte 0: Protocol (1 for WARLS)
        # Byte 1: Timeout in seconds (1-255)
        # Byte 2...: Pixel data (r, g, b, r, g, b, ...)
        
        num_pixels = len(frame)
        # WARLS protocol, 1 second timeout
        packet = bytearray([1, 1]) 
        for r, g, b in frame:
            packet.extend([r, g, b])
            
        try:
            self.udp_sockets[ip].sendto(packet, (ip, self.udp_ports[ip]))
        except Exception as e:
            logger.error(f"Error sending UDP frame to {ip}: {e}")

    async def cleanup(self):
        """Close the HTTP client and UDP sockets."""
        await self.http_client.aclose()
        for sock in self.udp_sockets.values():
            sock.close()

    def _get_effect_id(self, effect_name: str) -> int:
        """
        A small subset of WLED effect IDs.
        A full list can be fetched from the device at /json/effects
        """
        effects = {
            "Solid": 0,
            "Blink": 1,
            "Breathe": 2,
            "Wipe": 3,
            "Scan": 5,
            "Rainbow": 8,
            "Twinkle": 15,
            "Fireworks": 16,
        }
        return effects.get(effect_name.title(), 0)

    def _get_palette_id(self, palette_name: str) -> int:
        """
        A small subset of WLED palette IDs.
        A full list can be fetched from the device at /json/palettes
        """
        palettes = {
            "Default": 0,
            "*random": 5,
            "Rainbow": 9,
            "Party": 12,
            "Ocean": 20,
            "Lava": 22,
            "Forest": 25,
        }
        return palettes.get(palette_name, 0)

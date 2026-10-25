"""
Tasmota Smart Plug Manager

Controls Tasmota smart plugs via HTTP requests.
"""
import httpx
from typing import List

from shadow_warrior_brain.core.logging_config import get_logger

logger = get_logger(__name__)

class TasmotaManager:
    """
    Manages Tasmota smart plugs.
    """
    def __init__(self, plug_ips: List[str]):
        self.plug_ips = plug_ips
        self.client = httpx.AsyncClient()

    async def turn_on_all(self):
        """Turn on all Tasmota plugs."""
        logger.info("Turning on all Tasmota plugs.")
        for ip in self.plug_ips:
            await self._set_power_state(ip, "on")

    async def turn_off_all(self):
        """Turn off all Tasmota plugs."""
        logger.info("Turning off all Tasmota plugs.")
        for ip in self.plug_ips:
            await self._set_power_state(ip, "off")

    async def _set_power_state(self, ip: str, state: str):
        """
        Set the power state of a Tasmota plug.

        Args:
            ip: The IP address of the plug.
            state: "on" or "off".
        """
        try:
            url = f"http://{ip}/cm?cmnd=Power%20{state}"
            response = await self.client.get(url, timeout=5.0)
            response.raise_for_status()
            logger.debug(f"Set power state of {ip} to {state}")
        except httpx.RequestError as e:
            logger.error(f"Error setting power state of {ip} to {state}: {e}")

    async def cleanup(self):
        """Close the HTTP client."""
        await self.client.aclose()

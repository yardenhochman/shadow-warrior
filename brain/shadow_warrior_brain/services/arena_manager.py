"""
Arena Manager

Orchestrates the arena state, WLEDs, and Tasmota plugs.
"""
import asyncio
from shadow_warrior_brain.core.config import settings
from shadow_warrior_brain.core.logging_config import get_logger
from shadow_warrior_brain.core.state_machine import StateMachine, ArenaState
from shadow_warrior_brain.services.tasmota_manager import TasmotaManager
from shadow_warrior_brain.services.wled_manager import WledManager

logger = get_logger(__name__)

class ArenaManager:
    """
    Manages the arena state and its associated hardware.
    """
    def __init__(self):
        self.state_machine = StateMachine()
        self.tasmota_manager = TasmotaManager(settings.tasmota_plugs)
        self.wled_manager = WledManager(settings.wled_controllers)
        self._register_state_hooks()
        self.effects_task = None

    def _register_state_hooks(self):
        """Register hooks for state transitions."""
        self.state_machine.on_enter(ArenaState.IDLE, self.on_enter_idle)
        self.state_machine.on_enter(ArenaState.WARMING, self.on_enter_warming)
        self.state_machine.on_enter(ArenaState.FIGHT, self.on_enter_fight)
        self.state_machine.on_enter(ArenaState.COOLDOWN, self.on_enter_cooldown)
        self.state_machine.on_enter(ArenaState.SUSPENDED, self.on_enter_suspended)
        
        self.state_machine.on_exit(ArenaState.WARMING, self.on_exit_warming)
        self.state_machine.on_exit(ArenaState.FIGHT, self.on_exit_fight)

    async def on_enter_idle(self, new_state, from_state):
        """Handle entering the IDLE state."""
        logger.info("Arena is now IDLE.")
        await self.tasmota_manager.turn_off_all()
        await self.wled_manager.set_breathing_effect()

    async def on_enter_warming(self, new_state, from_state):
        """Handle entering the WARMING state."""
        logger.info("Arena is now WARMING.")
        await self.tasmota_manager.turn_on_all()
        # Start real-time effects
        if self.effects_task:
            self.effects_task.cancel()
        self.effects_task = asyncio.create_task(self.run_warming_effects())

    async def on_enter_fight(self, new_state, from_state):
        """Handle entering the FIGHT state."""
        logger.info("Arena is now in FIGHT.")
        await self.tasmota_manager.turn_on_all()
        # Start real-time effects
        if self.effects_task:
            self.effects_task.cancel()
        self.effects_task = asyncio.create_task(self.run_fight_effects())

    async def on_enter_cooldown(self, new_state, from_state):
        """Handle entering the COOLDOWN state."""
        logger.info("Arena is now in COOLDOWN.")
        await self.tasmota_manager.turn_off_all()
        # You might want a specific cooldown effect here
        await self.wled_manager.set_breathing_effect()

    async def on_enter_suspended(self, new_state, from_state):
        """Handle entering the SUSPENDED state."""
        logger.info("Arena is now SUSPENDED.")
        await self.tasmota_manager.turn_off_all()
        # Turn off WLEDs
        for ip in self.wled_manager.wled_ips:
             await self.wled_manager._set_effect(ip, effect_name="Solid", color_palette="*black", speed=0, intensity=0)


    async def on_exit_warming(self, state):
        """Handle exiting the WARMING state."""
        if self.effects_task:
            self.effects_task.cancel()
            self.effects_task = None
            
    async def on_exit_fight(self, state):
        """Handle exiting the FIGHT state."""
        if self.effects_task:
            self.effects_task.cancel()
            self.effects_task = None

    async def handle_punch(self, power: int):
        """Handle a punch event."""
        logger.info(f"Received punch with power: {power}")
        # Example logic: transition to FIGHT if warming and power is high
        if self.state_machine.current_state == ArenaState.WARMING and power > 80:
            await self.state_machine.transition_to(ArenaState.FIGHT)
        # During fight, punch could increase power level
        # This will be handled by the effects compositor

    async def handle_shout(self, intensity: float):
        """Handle a shout event."""
        logger.info(f"Received shout with intensity: {intensity}")
        # Example logic: transition to WARMING if IDLE and shout is loud
        if self.state_machine.current_state == ArenaState.IDLE and intensity > 0.8:
            await self.state_machine.transition_to(ArenaState.WARMING)

    async def handle_presence(self, detected: bool):
        """Handle a presence event."""
        logger.info(f"Presence detected: {detected}")
        if detected and self.state_machine.current_state == ArenaState.SUSPENDED:
            await self.state_machine.transition_to(ArenaState.IDLE)
        elif not detected and self.state_machine.current_state != ArenaState.SUSPENDED:
            await self.state_machine.force_transition_to(ArenaState.SUSPENDED)

    async def run_warming_effects(self):
        """Run real-time effects for the WARMING state."""
        # This will be implemented with effect generators and compositors
        # For now, a simple placeholder
        num_leds = 300 # example
        frame = [(0, 0, 0)] * num_leds
        while True:
            for ip in self.wled_manager.wled_ips:
                self.wled_manager.send_realtime_frame(ip, frame)
            await asyncio.sleep(1 / 30) # 30 fps

    async def run_fight_effects(self):
        """Run real-time effects for the FIGHT state."""
        # This will be implemented with effect generators and compositors
        # For now, a simple placeholder
        num_leds = 300 # example
        frame = [(255, 0, 0)] * num_leds
        while True:
            for ip in self.wled_manager.wled_ips:
                self.wled_manager.send_realtime_frame(ip, frame)
            await asyncio.sleep(1 / 30) # 30 fps
            
    async def cleanup(self):
        """Cleanup resources."""
        if self.effects_task:
            self.effects_task.cancel()
        await self.tasmota_manager.cleanup()
        await self.wled_manager.cleanup()
        # Ensure a final suspended state
        await self.on_enter_suspended(None, None)

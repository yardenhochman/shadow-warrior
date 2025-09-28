"""
Session Manager for Shadow Warrior Training Sessions

Manages the training session state machine and integrates with other services
"""

import asyncio
from datetime import datetime
from typing import Optional, Dict, Any

from shadow_warrior_brain.core.state_machine import StateMachine, SessionState, StateTransition
from shadow_warrior_brain.core.logging_config import get_logger
from shadow_warrior_brain.core.events import event_bus, EventType

logger = get_logger(__name__)


class SessionManager:
    """
    Manages training sessions using the state machine
    Coordinates between punching bag, audio, and LED systems
    """
    
    def __init__(self, ble_manager=None, audio_manager=None):
        self.state_machine = StateMachine()
        self.ble_manager = ble_manager
        self.audio_manager = audio_manager
        
        # Session tracking
        self.session_start_time: Optional[datetime] = None
        self.punch_count = 0
        self.session_data = {}
        self.last_state_transition_time: Optional[datetime] = datetime.now()  # Initialize with current time

        # State tracking for events
        self._last_state = self.state_machine.current_state
        self._last_punch_count = 0
        self._last_session_active = False

        # Register state machine hooks
        self._register_hooks()

        logger.info("Session Manager initialized")
    
    def _register_hooks(self):
        """Register state transition hooks"""
        
        # Enter state hooks
        self.state_machine.on_enter(SessionState.IDLE, self._on_enter_idle)
        self.state_machine.on_enter(SessionState.WARMING_UP, self._on_enter_warming_up)
        self.state_machine.on_enter(SessionState.FIGHT, self._on_enter_fight)
        self.state_machine.on_enter(SessionState.VICTORY, self._on_enter_victory)
        
        # Exit state hooks
        self.state_machine.on_exit(SessionState.IDLE, self._on_exit_idle)
        self.state_machine.on_exit(SessionState.WARMING_UP, self._on_exit_warming_up)
        self.state_machine.on_exit(SessionState.FIGHT, self._on_exit_fight)
        self.state_machine.on_exit(SessionState.VICTORY, self._on_exit_victory)
        
        # General transition hook
        self.state_machine.on_transition(self._on_any_transition)
    
    # State enter hooks
    async def _on_enter_idle(self, state: SessionState, from_state: SessionState):
        """Called when entering IDLE state"""
        logger.info(f"🏠 Entering IDLE state from {from_state.value}")
        
        # Turn off fight mode on punching bag
        if self.ble_manager:
            await self.ble_manager.set_fight_mode(False)
        
        # Stop audio monitoring
        if self.audio_manager:
            await self.audio_manager.stop_monitoring()
        
        # Reset session data if coming from VICTORY
        if from_state == SessionState.VICTORY:
            self._reset_session()
        
        # TODO: Set LEDs to idle/breathing effect
    
    async def _on_enter_warming_up(self, state: SessionState, from_state: SessionState):
        """Called when entering WARMING_UP state"""
        logger.info(f"🔥 Entering WARMING_UP state from {from_state.value}")
        
        # Start session if from IDLE
        if from_state == SessionState.IDLE:
            self.session_start_time = datetime.now()
            logger.info("🎯 New training session started")
        
        # Enable fight mode but with lower sensitivity
        if self.ble_manager:
            await self.ble_manager.set_parameters({
                'fight_mode': True,
                'alpha': 0.9,  # More smoothing during warm-up
                'threshold': 15.0  # Higher threshold during warm-up
            })
        
        # Start audio monitoring
        if self.audio_manager:
            await self.audio_manager.start_monitoring()
        
        # TODO: Set LEDs to warm-up effect (slow fire)
    
    async def _on_enter_fight(self, state: SessionState, from_state: SessionState):
        """Called when entering FIGHT state"""
        logger.info(f"⚡ Entering FIGHT state from {from_state.value}")
        
        # Set high-sensitivity parameters for fight
        if self.ble_manager:
            await self.ble_manager.set_parameters({
                'fight_mode': True,
                'alpha': 0.7,  # Less smoothing for responsiveness
                'threshold': 8.0  # Lower threshold for punch detection
            })
        
        # Reset punch counter for this fight round
        self.punch_count = 0
        
        # TODO: Set LEDs to intense fight effect (fast fire/flash)
    
    async def _on_enter_victory(self, state: SessionState, from_state: SessionState):
        """Called when entering VICTORY state"""
        logger.info(f"🏆 Entering VICTORY state from {from_state.value}")
        
        # Turn off fight mode
        if self.ble_manager:
            await self.ble_manager.set_fight_mode(False)
        
        # TODO: Set LEDs to victory effect (rainbow/celebration)
        
        # Record victory data
        victory_time = datetime.now()
        if self.session_start_time:
            session_duration = (victory_time - self.session_start_time).total_seconds()
            self.session_data['victory_time'] = victory_time
            self.session_data['session_duration'] = session_duration
            logger.info(f"🎉 Victory achieved! Session duration: {session_duration:.1f}s")
    
    # State exit hooks
    async def _on_exit_idle(self, state: SessionState):
        """Called when exiting IDLE state"""
        logger.debug("⬅️ Exiting IDLE state")
    
    async def _on_exit_warming_up(self, state: SessionState):
        """Called when exiting WARMING_UP state"""
        logger.debug("⬅️ Exiting WARMING_UP state")
    
    async def _on_exit_fight(self, state: SessionState):
        """Called when exiting FIGHT state"""
        logger.debug("⬅️ Exiting FIGHT state")
        
        # Record fight statistics
        fight_duration = self.state_machine.get_state_duration()
        self.session_data.setdefault('fights', []).append({
            'duration': fight_duration,
            'punches': self.punch_count,
            'punches_per_second': self.punch_count / max(fight_duration, 1)
        })
    
    async def _on_exit_victory(self, state: SessionState):
        """Called when exiting VICTORY state"""
        logger.debug("⬅️ Exiting VICTORY state")
    
    # General transition hook
    async def _on_any_transition(self, transition: StateTransition):
        """Called on any state transition"""
        logger.info(f"🔄 Transition: {transition.from_state.value} → {transition.to_state.value}")

        # Update last transition timestamp
        self.last_state_transition_time = transition.timestamp

        # Log transition with timestamp
        transition_data = {
            'from_state': transition.from_state.value,
            'to_state': transition.to_state.value,
            'timestamp': transition.timestamp,
            'metadata': transition.metadata
        }

        self.session_data.setdefault('transitions', []).append(transition_data)

        # Emit state change event if state actually changed
        if self._last_state != transition.to_state:
            await event_bus.emit_event(
                EventType.SESSION_STATE_CHANGED,
                "session_manager",
                {
                    "from_state": transition.from_state.value,
                    "to_state": transition.to_state.value,
                    "timestamp": transition.timestamp.isoformat(),
                    "metadata": transition.metadata
                }
            )
            self._last_state = transition.to_state

            # Emit specific session events
            if transition.to_state == SessionState.WARMING_UP and transition.from_state == SessionState.IDLE:
                await event_bus.emit_event(
                    EventType.SESSION_STARTED,
                    "session_manager",
                    {"session_type": "warming_up", "timestamp": transition.timestamp.isoformat()}
                )
            elif transition.to_state == SessionState.IDLE and transition.from_state in [SessionState.FIGHT, SessionState.VICTORY, SessionState.WARMING_UP]:
                await event_bus.emit_event(
                    EventType.SESSION_ENDED,
                    "session_manager",
                    {
                        "final_punch_count": self.punch_count,
                        "session_duration": (transition.timestamp - self.session_start_time).total_seconds() if self.session_start_time else 0,
                        "timestamp": transition.timestamp.isoformat()
                    }
                )
    
    # Public interface methods
    async def start_warming_up(self) -> bool:
        """Start a warming up session"""
        return await self.state_machine.transition_to(SessionState.WARMING_UP)
    
    async def start_fight(self) -> bool:
        """Start a fight round"""
        return await self.state_machine.transition_to(SessionState.FIGHT)
    
    async def achieve_victory(self) -> bool:
        """Transition to victory state"""
        return await self.state_machine.transition_to(SessionState.VICTORY)
    
    async def return_to_idle(self) -> bool:
        """Return to idle state"""
        return await self.state_machine.transition_to(SessionState.IDLE)
    
    async def return_to_warming_up(self) -> bool:
        """Return to warming up (from fight)"""
        return await self.state_machine.transition_to(SessionState.WARMING_UP)
    
    async def emergency_stop(self) -> bool:
        """Force transition to idle (emergency stop)"""
        return await self.state_machine.force_transition_to(
            SessionState.IDLE, 
            {'reason': 'emergency_stop'}
        )
    
    def register_punch(self, acceleration: float):
        """Register a punch during fight state"""
        if self.state_machine.current_state == SessionState.FIGHT:
            self.punch_count += 1
            logger.info(f"👊 Punch #{self.punch_count} detected! (acceleration: {acceleration:.2f})")

            # Emit punch detected event if count changed
            if self.punch_count != self._last_punch_count:
                asyncio.create_task(event_bus.emit_event(
                    EventType.SESSION_PUNCH_DETECTED,
                    "session_manager",
                    {
                        "punch_number": self.punch_count,
                        "acceleration": acceleration,
                        "timestamp": datetime.now().isoformat()
                    }
                ))
                self._last_punch_count = self.punch_count
    
    def _reset_session(self):
        """Reset session data for new session"""
        self.session_start_time = None
        self.punch_count = 0
        self.session_data = {}
        self._last_punch_count = 0
        logger.debug("🔄 Session data reset")
    
    # Status and query methods
    def get_current_state(self) -> SessionState:
        """Get current session state"""
        return self.state_machine.get_current_state()
    
    def get_valid_transitions(self) -> list:
        """Get valid state transitions from current state"""
        return [state.value for state in self.state_machine.get_valid_transitions()]
    
    def get_session_status(self) -> Dict[str, Any]:
        """Get comprehensive session status"""
        current_state = self.state_machine.get_current_state()

        status = {
            'current_state': current_state.value,
            'transition_timestamp': self.last_state_transition_time.isoformat() if self.last_state_transition_time else None,
            'valid_transitions': self.get_valid_transitions(),
            'session_active': self.session_start_time is not None
        }
        
        if self.session_start_time:
            status['session_duration'] = (datetime.now() - self.session_start_time).total_seconds()
        
        return status
    
    def get_session_data(self) -> Dict[str, Any]:
        """Get complete session data"""
        return self.session_data.copy()
    
    async def cleanup(self):
        """Cleanup session manager"""
        await self.emergency_stop()
        logger.info("Session Manager cleanup complete")
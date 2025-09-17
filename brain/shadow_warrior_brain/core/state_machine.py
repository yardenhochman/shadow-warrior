"""
Shadow Warrior State Machine

Manages the training session states and transitions with hooks for custom behavior.
"""

import asyncio
from datetime import datetime
from enum import Enum
from typing import Dict, List, Callable, Optional, Any
from dataclasses import dataclass

from shadow_warrior_brain.core.logging_config import get_logger

logger = get_logger(__name__)


class SessionState(Enum):
    """Training session states"""
    IDLE = "idle"
    WARMING_UP = "warming_up"
    FIGHT = "fight"
    VICTORY = "victory"


@dataclass
class StateTransition:
    """Represents a state transition"""
    from_state: SessionState
    to_state: SessionState
    timestamp: datetime
    metadata: Optional[Dict[str, Any]] = None


class StateMachine:
    """
    State machine for Shadow Warrior training sessions
    
    State transitions:
    - IDLE -> WARMING_UP
    - WARMING_UP -> {IDLE, FIGHT}
    - FIGHT -> {VICTORY, WARMING_UP}
    - VICTORY -> IDLE
    """
    
    # Valid state transitions
    VALID_TRANSITIONS: Dict[SessionState, List[SessionState]] = {
        SessionState.IDLE: [SessionState.WARMING_UP],
        SessionState.WARMING_UP: [SessionState.IDLE, SessionState.FIGHT],
        SessionState.FIGHT: [SessionState.VICTORY, SessionState.WARMING_UP],
        SessionState.VICTORY: [SessionState.IDLE]
    }
    
    def __init__(self, initial_state: SessionState = SessionState.IDLE):
        self.current_state = initial_state
        self.state_start_time = datetime.now()
        self.transition_history: List[StateTransition] = []
        
        # Transition hooks - functions called on state changes
        self._on_enter_hooks: Dict[SessionState, List[Callable]] = {
            state: [] for state in SessionState
        }
        self._on_exit_hooks: Dict[SessionState, List[Callable]] = {
            state: [] for state in SessionState
        }
        self._on_transition_hooks: List[Callable] = []
        
        # State duration tracking
        self._state_durations: Dict[SessionState, float] = {}
        
        logger.info(f"State machine initialized in {self.current_state.value} state")
    
    def can_transition_to(self, target_state: SessionState) -> bool:
        """Check if transition to target state is valid"""
        return target_state in self.VALID_TRANSITIONS.get(self.current_state, [])
    
    async def transition_to(self, target_state: SessionState, metadata: Optional[Dict[str, Any]] = None) -> bool:
        """
        Transition to a new state if valid
        
        Args:
            target_state: The state to transition to
            metadata: Optional metadata about the transition
            
        Returns:
            True if transition was successful, False otherwise
        """
        if not self.can_transition_to(target_state):
            logger.warning(f"Invalid transition: {self.current_state.value} -> {target_state.value}")
            return False
        
        # Calculate duration in current state
        now = datetime.now()
        duration = (now - self.state_start_time).total_seconds()
        self._state_durations[self.current_state] = duration
        
        # Create transition record
        transition = StateTransition(
            from_state=self.current_state,
            to_state=target_state,
            timestamp=now,
            metadata=metadata
        )
        
        logger.info(f"State transition: {self.current_state.value} -> {target_state.value} "
              f"(duration: {duration:.1f}s)")
        
        # Call exit hooks for current state
        await self._call_exit_hooks(self.current_state)
        
        # Call transition hooks
        await self._call_transition_hooks(transition)
        
        # Update state
        previous_state = self.current_state
        self.current_state = target_state
        self.state_start_time = now
        self.transition_history.append(transition)
        
        # Call enter hooks for new state
        await self._call_enter_hooks(target_state, previous_state)
        
        return True
    
    async def force_transition_to(self, target_state: SessionState, metadata: Optional[Dict[str, Any]] = None) -> bool:
        """
        Force transition to any state (bypasses validation)
        Use with caution - mainly for emergency stops or admin overrides
        """
        logger.warning(f"FORCED transition: {self.current_state.value} -> {target_state.value}")
        
        # Calculate duration in current state
        now = datetime.now()
        duration = (now - self.state_start_time).total_seconds()
        self._state_durations[self.current_state] = duration
        
        # Create transition record with force flag
        if metadata is None:
            metadata = {}
        metadata['forced'] = True
        
        transition = StateTransition(
            from_state=self.current_state,
            to_state=target_state,
            timestamp=now,
            metadata=metadata
        )
        
        # Call exit hooks for current state
        await self._call_exit_hooks(self.current_state)
        
        # Call transition hooks
        await self._call_transition_hooks(transition)
        
        # Update state
        previous_state = self.current_state
        self.current_state = target_state
        self.state_start_time = now
        self.transition_history.append(transition)
        
        # Call enter hooks for new state
        await self._call_enter_hooks(target_state, previous_state)
        
        return True
    
    # Hook registration methods
    def on_enter(self, state: SessionState, hook: Callable):
        """Register a hook to be called when entering a specific state"""
        self._on_enter_hooks[state].append(hook)
        logger.debug(f"Registered enter hook for {state.value} state")
    
    def on_exit(self, state: SessionState, hook: Callable):
        """Register a hook to be called when exiting a specific state"""
        self._on_exit_hooks[state].append(hook)
        logger.debug(f"Registered exit hook for {state.value} state")
    
    def on_transition(self, hook: Callable):
        """Register a hook to be called on any state transition"""
        self._on_transition_hooks.append(hook)
        logger.debug("Registered transition hook")
    
    # Hook execution methods
    async def _call_enter_hooks(self, state: SessionState, from_state: SessionState):
        """Call all enter hooks for a state"""
        for hook in self._on_enter_hooks[state]:
            try:
                if asyncio.iscoroutinefunction(hook):
                    await hook(state, from_state)
                else:
                    hook(state, from_state)
            except Exception as e:
                logger.error(f"Error in enter hook for {state.value}: {e}")
    
    async def _call_exit_hooks(self, state: SessionState):
        """Call all exit hooks for a state"""
        for hook in self._on_exit_hooks[state]:
            try:
                if asyncio.iscoroutinefunction(hook):
                    await hook(state)
                else:
                    hook(state)
            except Exception as e:
                logger.error(f"Error in exit hook for {state.value}: {e}")
    
    async def _call_transition_hooks(self, transition: StateTransition):
        """Call all transition hooks"""
        for hook in self._on_transition_hooks:
            try:
                if asyncio.iscoroutinefunction(hook):
                    await hook(transition)
                else:
                    hook(transition)
            except Exception as e:
                logger.error(f"Error in transition hook: {e}")
    
    # Status and query methods
    def get_current_state(self) -> SessionState:
        """Get the current state"""
        return self.current_state
    
    def get_state_duration(self) -> float:
        """Get duration in current state (seconds)"""
        return (datetime.now() - self.state_start_time).total_seconds()
    
    def get_valid_transitions(self) -> List[SessionState]:
        """Get list of valid states we can transition to from current state"""
        return self.VALID_TRANSITIONS.get(self.current_state, [])
    
    def get_transition_history(self, limit: Optional[int] = None) -> List[StateTransition]:
        """Get transition history, optionally limited to recent N transitions"""
        if limit:
            return self.transition_history[-limit:]
        return self.transition_history.copy()
    
    def get_state_statistics(self) -> Dict[SessionState, Dict[str, Any]]:
        """Get statistics about state usage"""
        stats = {}
        
        for state in SessionState:
            # Count transitions into this state
            transitions_in = sum(1 for t in self.transition_history if t.to_state == state)
            
            # Get total time spent in this state
            total_duration = sum(
                duration for s, duration in self._state_durations.items() if s == state
            )
            
            # Add current state duration if applicable
            if self.current_state == state:
                total_duration += self.get_state_duration()
            
            stats[state] = {
                'transitions_in': transitions_in,
                'total_duration': total_duration,
                'average_duration': total_duration / max(transitions_in, 1)
            }
        
        return stats
    
    def get_status(self) -> Dict[str, Any]:
        """Get complete state machine status"""
        return {
            'current_state': self.current_state.value,
            'state_duration': self.get_state_duration(),
            'valid_transitions': [state.value for state in self.get_valid_transitions()],
            'total_transitions': len(self.transition_history),
            'statistics': {
                state.value: stats for state, stats in self.get_state_statistics().items()
            }
        }
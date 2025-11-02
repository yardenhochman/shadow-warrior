// Composable for integrating state machine with event bus
import { onMounted, onUnmounted } from 'vue';
import { useStateMachineStore } from 'src/stores/state-machine';
import { eventBus, Events } from 'src/services/event-bus';
import type { ArenaState } from 'src/types/state-machine';

export function useStateMachine() {
  const store = useStateMachineStore();

  // Event handlers
  const handlePunch = (payload: { force: number }) => {
    store.onPunchDetected(payload.force);
  };

  const handleShout = (payload: { amplitude: number }) => {
    store.onShoutDetected(payload.amplitude);
  };

  const handleTransitionRequest = (payload: { state: string }) => {
    // Manual transition request from UI or external source
    const targetState = payload.state as ArenaState;
    store.transition(targetState);
  };

  // Set up event listeners
  onMounted(() => {
    eventBus.on(Events.PUNCH_DETECTED, handlePunch);
    eventBus.on(Events.SHOUT_DETECTED, handleShout);
    eventBus.on(Events.STATE_TRANSITION_REQUESTED, handleTransitionRequest);
  });

  // Clean up event listeners
  onUnmounted(() => {
    eventBus.off(Events.PUNCH_DETECTED, handlePunch);
    eventBus.off(Events.SHOUT_DETECTED, handleShout);
    eventBus.off(Events.STATE_TRANSITION_REQUESTED, handleTransitionRequest);
  });

  return {
    store,
    eventBus,
  };
}

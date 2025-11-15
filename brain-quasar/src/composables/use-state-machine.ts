// Composable for integrating state machine with event bus
import { onMounted, onUnmounted } from 'vue';
import { useStateMachineStore } from 'src/stores/state-machine';
import { eventBus, Events } from 'src/services/event-bus';
import type { ArenaState } from 'src/types/state-machine';
import { musicPlayerService } from 'src/services/music-player';
import { ArenaState as ArenaStateEnum } from 'src/types/state-machine';

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

  const handleStateChange = (payload: { from: ArenaState; to: ArenaState }) => {
    console.log('State change detected:', payload.from, '->', payload.to);

    // Handle music playback based on state transitions
    if (payload.to === ArenaStateEnum.FIGHT) {
      // Entering fight mode - play random song
      const playlist = musicPlayerService.getPlaylist();
      console.log('Fight mode - playlist has', playlist.length, 'tracks');

      if (playlist.length > 0) {
        // Pick random track
        const randomIndex = Math.floor(Math.random() * playlist.length);
        const randomTrack = playlist[randomIndex];
        if (randomTrack) {
          console.log('Fight mode started - playing random track:', randomTrack.name, 'ID:', randomTrack.id);
          void musicPlayerService.play(randomTrack.id);
        }
      } else {
        console.warn('Fight mode started but no music in playlist');
      }
    } else if (payload.from === ArenaStateEnum.FIGHT) {
      // Exiting fight mode - stop music
      console.log('Fight mode ended - stopping music');
      void musicPlayerService.stop();
    }
  };

  // Set up event listeners
  onMounted(() => {
    eventBus.on(Events.PUNCH_DETECTED, handlePunch);
    eventBus.on(Events.SHOUT_DETECTED, handleShout);
    eventBus.on(Events.STATE_TRANSITION_REQUESTED, handleTransitionRequest);
    eventBus.on(Events.STATE_CHANGED, handleStateChange);
  });

  // Clean up event listeners
  onUnmounted(() => {
    eventBus.off(Events.PUNCH_DETECTED, handlePunch);
    eventBus.off(Events.SHOUT_DETECTED, handleShout);
    eventBus.off(Events.STATE_TRANSITION_REQUESTED, handleTransitionRequest);
    eventBus.off(Events.STATE_CHANGED, handleStateChange);
  });

  return {
    store,
    eventBus,
  };
}

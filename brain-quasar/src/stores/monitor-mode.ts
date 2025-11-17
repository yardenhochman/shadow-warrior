import { defineStore } from 'pinia';
import { ref } from 'vue';

interface ArenaState {
  currentState: string;
  metrics: {
    shoutAmplitude: number;
    punchForce: number;
    warmingPower: number;
    fightPower: number;
  };
  progress: {
    warmingProgress: number;
    fightProgress: number;
  };
  timers: {
    fightElapsed: number;
    warmingElapsed: number;
    cooldownRemaining: number;
  };
  timestamp: number;
}

export const useMonitorModeStore = defineStore('monitorMode', () => {
  const isMonitorMode = ref(false);
  const connectedArenaId = ref<string | null>(null);
  const connectedArenaName = ref<string | null>(null);
  const connectionStatus = ref<'disconnected' | 'scanning' | 'connecting' | 'connected'>(
    'disconnected'
  );
  const lastUpdate = ref<number | null>(null);
  const remoteArenaState = ref<ArenaState | null>(null);

  function enableMonitorMode() {
    isMonitorMode.value = true;
  }

  function disableMonitorMode() {
    isMonitorMode.value = false;
    connectedArenaId.value = null;
    connectedArenaName.value = null;
    connectionStatus.value = 'disconnected';
    remoteArenaState.value = null;
  }

  function setConnectionStatus(
    status: 'disconnected' | 'scanning' | 'connecting' | 'connected'
  ) {
    connectionStatus.value = status;
  }

  function setConnectedArena(id: string, name: string) {
    connectedArenaId.value = id;
    connectedArenaName.value = name;
  }

  function updateRemoteArenaState(state: ArenaState) {
    remoteArenaState.value = state;
    lastUpdate.value = Date.now();
  }

  return {
    isMonitorMode,
    connectedArenaId,
    connectedArenaName,
    connectionStatus,
    lastUpdate,
    remoteArenaState,
    enableMonitorMode,
    disableMonitorMode,
    setConnectionStatus,
    setConnectedArena,
    updateRemoteArenaState,
  };
});

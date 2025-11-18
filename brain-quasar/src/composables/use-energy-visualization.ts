// Composable for real-time energy visualization
import { ref, onMounted, onUnmounted } from 'vue';
import { eventBus, Events } from 'src/services/event-bus';

export function useEnergyVisualization() {
  const shoutAmplitude = ref(0);
  const punchForce = ref(0);
  const punchMagnitude = ref(0);
  const lastPunchTime = ref(0);

  // Punch force decay for visual effect
  const PUNCH_DECAY_MS = 500; // How long the punch bar stays visible
  let punchDecayInterval: number | null = null;

  const handleShout = (payload: { amplitude: number }) => {
    shoutAmplitude.value = payload.amplitude;
  };

  const handlePunch = (payload: { force: number; magnitude: number; timestamp: number }) => {
    punchForce.value = payload.force;
    punchMagnitude.value = payload.magnitude;
    lastPunchTime.value = payload.timestamp;

    // Clear any existing decay interval
    if (punchDecayInterval !== null) {
      clearInterval(punchDecayInterval);
    }

    // Start decay animation
    const startForce = payload.force;
    const startTime = Date.now();

    punchDecayInterval = window.setInterval(() => {
      const elapsed = Date.now() - startTime;
      const progress = Math.min(1, elapsed / PUNCH_DECAY_MS);

      // Exponential decay
      punchForce.value = startForce * Math.exp(-progress * 3);

      if (progress >= 1) {
        punchForce.value = 0;
        if (punchDecayInterval !== null) {
          clearInterval(punchDecayInterval);
          punchDecayInterval = null;
        }
      }
    }, 16); // ~60fps
  };

  const handleMicrophoneAmplitude = (payload: { amplitude: number }) => {
    // Continuous amplitude updates from microphone service
    shoutAmplitude.value = payload.amplitude;
  };

  onMounted(() => {
    // Listen to shout events (above threshold)
    eventBus.on(Events.SHOUT_DETECTED, handleShout);

    // Listen to continuous microphone amplitude for visualization
    eventBus.on('microphone:amplitude', handleMicrophoneAmplitude);

    // Listen to punch events
    eventBus.on(Events.PUNCH_DETECTED, handlePunch);
  });

  onUnmounted(() => {
    eventBus.off(Events.SHOUT_DETECTED, handleShout);
    eventBus.off('microphone:amplitude', handleMicrophoneAmplitude);
    eventBus.off(Events.PUNCH_DETECTED, handlePunch);

    if (punchDecayInterval !== null) {
      clearInterval(punchDecayInterval);
    }
  });

  return {
    shoutAmplitude,
    punchForce,
    punchMagnitude,
    lastPunchTime,
  };
}

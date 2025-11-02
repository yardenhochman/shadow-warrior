<template>
  <q-page class="flex flex-center column q-pa-md">
    <div class="full-width" style="max-width: 800px">
      <!-- Current State Display -->
      <q-card class="q-mb-md">
        <q-card-section>
          <div class="text-h4 text-center">Shadow Warrior Arena</div>
          <div class="text-h2 text-center q-mt-md" :class="stateColorClass">
            {{ currentStateDisplay }}
          </div>
        </q-card-section>
      </q-card>

      <!-- Real-time Energy Visualization -->
      <q-card class="q-mb-md">
        <q-card-section>
          <div class="text-h6 q-mb-md">Live Sensor Input</div>

          <div class="row q-col-gutter-md">
            <div class="col-12 col-sm-6">
              <EnergyBar label="Microphone (Shout)" :value="energyViz.shoutAmplitude.value" />
            </div>
            <div class="col-12 col-sm-6">
              <EnergyBar label="Accelerometer (Punch)" :value="energyViz.punchForce.value" />
            </div>
          </div>
        </q-card-section>
      </q-card>

      <!-- Metrics Display -->
      <q-card class="q-mb-md">
        <q-card-section>
          <div class="text-h6">Metrics</div>

          <!-- Warming Progress -->
          <div v-if="stateMachine.isWarming" class="q-mt-md">
            <div class="text-subtitle2">Warming Progress</div>
            <q-linear-progress
              :value="stateMachine.warmingProgress / 100"
              size="25px"
              color="orange"
              class="q-mt-sm"
            >
              <div class="absolute-full flex flex-center">
                <q-badge
                  color="white"
                  text-color="black"
                  :label="`${Math.round(stateMachine.warmingProgress)}%`"
                />
              </div>
            </q-linear-progress>
          </div>

          <!-- Fight Progress -->
          <div v-if="stateMachine.isFight" class="q-mt-md">
            <div class="text-subtitle2">Fight Progress</div>
            <q-linear-progress
              :value="stateMachine.fightProgress / 100"
              size="25px"
              color="red"
              class="q-mt-sm"
            >
              <div class="absolute-full flex flex-center">
                <q-badge
                  color="white"
                  text-color="black"
                  :label="`${Math.round(stateMachine.fightProgress)}%`"
                />
              </div>
            </q-linear-progress>
          </div>

          <!-- Cooldown Timer -->
          <div v-if="stateMachine.isCooldown" class="q-mt-md">
            <div class="text-subtitle2">Cooldown Remaining</div>
            <div class="text-h5 text-center q-mt-sm">
              {{ cooldownTimeDisplay }}
            </div>
          </div>

          <!-- Real-time Metrics -->
          <div class="row q-mt-md q-col-gutter-md">
            <div class="col-6">
              <q-card flat bordered>
                <q-card-section>
                  <div class="text-caption">Punch Force</div>
                  <div class="text-h6">{{ (metrics.punchForce * 100).toFixed(0) }}%</div>
                </q-card-section>
              </q-card>
            </div>
            <div class="col-6">
              <q-card flat bordered>
                <q-card-section>
                  <div class="text-caption">Shout Amplitude</div>
                  <div class="text-h6">{{ (metrics.shoutAmplitude * 100).toFixed(0) }}%</div>
                </q-card-section>
              </q-card>
            </div>
          </div>
        </q-card-section>
      </q-card>

      <!-- Manual Controls -->
      <q-card class="q-mb-md">
        <q-card-section>
          <div class="text-h6">Manual Controls</div>

          <div class="row q-mt-md q-col-gutter-sm">
            <div class="col-12 col-sm-6">
              <q-btn
                label="Start Sensors"
                color="primary"
                icon="sensors"
                :disabled="sensorsRunning"
                @click="startSensors"
                class="full-width"
              />
            </div>
            <div class="col-12 col-sm-6">
              <q-btn
                label="Stop Sensors"
                color="negative"
                icon="sensors_off"
                :disabled="!sensorsRunning"
                @click="stopSensors"
                class="full-width"
              />
            </div>
          </div>

          <div class="row q-mt-sm q-col-gutter-sm">
            <div class="col-4">
              <q-btn
                label="Idle"
                color="grey"
                :outline="!stateMachine.isIdle"
                @click="forceState('idle')"
                class="full-width"
                size="sm"
              />
            </div>
            <div class="col-4">
              <q-btn
                label="Warming"
                color="orange"
                :outline="!stateMachine.isWarming"
                @click="forceState('warming')"
                class="full-width"
                size="sm"
              />
            </div>
            <div class="col-4">
              <q-btn
                label="Fight"
                color="red"
                :outline="!stateMachine.isFight"
                @click="forceState('fight')"
                class="full-width"
                size="sm"
              />
            </div>
          </div>
        </q-card-section>
      </q-card>

      <!-- Device Status -->
      <q-card>
        <q-card-section>
          <div class="text-h6">Device Status</div>

          <q-list>
            <q-item>
              <q-item-section avatar>
                <q-icon
                  :name="ledConnected ? 'check_circle' : 'cancel'"
                  :color="ledConnected ? 'positive' : 'negative'"
                />
              </q-item-section>
              <q-item-section>
                <q-item-label>LED Controller</q-item-label>
                <q-item-label caption>
                  {{ ledConnected ? 'Connected' : 'Disconnected' }}
                </q-item-label>
              </q-item-section>
              <q-item-section side v-if="!ledConnected">
                <q-btn label="Connect" color="primary" size="sm" @click="connectLED" />
              </q-item-section>
            </q-item>

            <q-separator />

            <q-item>
              <q-item-section avatar>
                <q-icon
                  :name="microphoneService.isEnabled() ? 'check_circle' : 'cancel'"
                  :color="microphoneService.isEnabled() ? 'positive' : 'negative'"
                />
              </q-item-section>
              <q-item-section>
                <q-item-label>Microphone</q-item-label>
                <q-item-label caption>
                  {{ microphoneService.isEnabled() ? 'Active' : 'Inactive' }}
                </q-item-label>
              </q-item-section>
            </q-item>
          </q-list>
        </q-card-section>
      </q-card>
    </div>
  </q-page>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue';
import { useStateMachineStore } from 'src/stores/state-machine';
import { ArenaState } from 'src/types/state-machine';
import { accelerometerService } from 'src/services/accelerometer';
import { microphoneService } from 'src/services/microphone';
import { ledControllerService } from 'src/services/led-controller';
import { speakerService } from 'src/services/speaker';
import { useStateMachine } from 'src/composables/use-state-machine';
import { useEnergyVisualization } from 'src/composables/use-energy-visualization';
import EnergyBar from 'src/components/EnergyBar.vue';

const stateMachine = useStateMachineStore();
useStateMachine();
const energyViz = useEnergyVisualization();

const sensorsRunning = ref(false);
const ledConnected = ref(false);
const cooldownInterval = ref<number | null>(null);

const metrics = computed(() => stateMachine.metrics);

const currentStateDisplay = computed(() => {
  return stateMachine.currentState.toUpperCase();
});

const stateColorClass = computed(() => {
  const colors: Record<ArenaState, string> = {
    [ArenaState.IDLE]: 'text-grey',
    [ArenaState.WARMING]: 'text-orange',
    [ArenaState.FIGHT]: 'text-red',
    [ArenaState.VICTORY]: 'text-green',
    [ArenaState.COOLDOWN]: 'text-blue',
  };
  return colors[stateMachine.currentState];
});

const cooldownTimeDisplay = computed(() => {
  const remaining = stateMachine.cooldownTimeRemaining;
  const minutes = Math.floor(remaining / 60000);
  const seconds = Math.floor((remaining % 60000) / 1000);
  return `${minutes}:${seconds.toString().padStart(2, '0')}`;
});

async function startSensors() {
  let microphoneStarted = false;
  let accelerometerStarted = false;
  let microphoneError: string | null = null;
  let accelerometerError: string | null = null;

  try {
    // Ensure audio context is resumed on mobile platforms
    await microphoneService.ensureAudioContextResumed();

    // Start both sensors in parallel
    await Promise.allSettled([
      microphoneService.start(),
      accelerometerService.start(),
    ]).then(([micro, accel]) => {
      if (micro.status === 'fulfilled') {
        microphoneStarted = true;
      } else {
        microphoneError = micro.reason instanceof Error ? micro.reason.message : String(micro.reason);
        console.warn('Microphone failed to start:', microphoneError);
      }

      if (accel.status === 'fulfilled') {
        accelerometerStarted = true;
      } else {
        accelerometerError = accel.reason instanceof Error ? accel.reason.message : String(accel.reason);
        console.warn('Accelerometer failed to start:', accelerometerError);
      }
    });

    // Mark as running if at least one sensor started
    if (microphoneStarted || accelerometerStarted) {
      sensorsRunning.value = true;

      // Show warnings if one sensor failed
      if (!microphoneStarted && accelerometerStarted) {
        alert('Microphone access failed. Accelerometer started successfully, but shout detection will not work. Please check microphone permissions.');
      } else if (microphoneStarted && !accelerometerStarted) {
        alert('Accelerometer access failed. Microphone started successfully, but punch detection will not work. Please check motion/accelerometer permissions.');
      }
    } else {
      // Both failed
      const errorMsg = microphoneError || accelerometerError || 'Unknown error';
      alert(`Failed to start sensors: ${errorMsg}`);
      throw new Error(errorMsg);
    }
  } catch (error) {
    console.error('Error starting sensors:', error);
    sensorsRunning.value = false;
  }
}

async function stopSensors() {
  await accelerometerService.stop();
  await microphoneService.stop();
  sensorsRunning.value = false;
}

async function connectLED() {
  try {
    await ledControllerService.initialize();
    const devices = await ledControllerService.scan();

    if (devices.length > 0 && devices[0]) {
      await ledControllerService.connect(devices[0].deviceId);
      ledConnected.value = true;
    }
  } catch (error) {
    console.error('Failed to connect LED controller:', error);
  }
}

function forceState(state: string) {
  stateMachine.forceTransition(state as ArenaState);
}

onMounted(async () => {
  // Initialize services
  await ledControllerService.initialize();
  ledConnected.value = ledControllerService.isConnected();

  // Preload audio
  await speakerService.preloadAll();

  // Update cooldown timer
  cooldownInterval.value = window.setInterval(() => {
    if (stateMachine.isCooldown) {
      // Force reactivity update
      stateMachine.$patch({});
    }
  }, 1000);
});

onUnmounted(() => {
  if (cooldownInterval.value !== null) {
    clearInterval(cooldownInterval.value);
  }
});
</script>

<style scoped>
.q-card {
  border-radius: 12px;
}
</style>

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
                <q-btn label="Connect" color="primary" size="sm" @click="scanAndShowDevices" />
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

    <!-- Device Selection Dialog -->
    <q-dialog v-model="deviceDialog.show" persistent>
      <q-card style="min-width: 400px">
        <q-card-section class="row items-center">
          <div class="text-h6">Select LED Controller</div>
          <q-space />
          <q-btn icon="close" flat round dense v-close-popup />
        </q-card-section>

        <q-card-section v-if="deviceDialog.scanning">
          <div class="text-center q-pa-md">
            <q-spinner color="primary" size="3em" />
            <div class="text-h6 q-mt-md">Scanning for devices...</div>
          </div>
        </q-card-section>

        <q-card-section v-else-if="deviceDialog.devices.length === 0">
          <div class="text-center q-pa-md">
            <q-icon name="device_unknown" size="4em" color="grey" />
            <div class="text-h6 q-mt-md text-grey">No devices found</div>
            <div class="text-body2 text-grey q-mt-sm">
              Make sure your ShadowLED device is powered on and in range
            </div>
          </div>
        </q-card-section>

        <q-list v-else>
          <q-item
            v-for="device in deviceDialog.devices"
            :key="device.deviceId"
            clickable
            @click="connectToDevice(device)"
          >
            <q-item-section avatar>
              <q-icon name="lightbulb" color="primary" />
            </q-item-section>
            <q-item-section>
              <q-item-label>{{ device.name || 'Unknown Device' }}</q-item-label>
              <q-item-label caption>{{ device.deviceId }}</q-item-label>
            </q-item-section>
            <q-item-section side>
              <q-icon name="chevron_right" color="grey" />
            </q-item-section>
          </q-item>
        </q-list>

        <q-card-actions align="right">
          <q-btn flat label="Cancel" v-close-popup />
          <q-btn
            flat
            label="Scan Again"
            color="primary"
            @click="scanDevices"
            :loading="deviceDialog.scanning"
          />
        </q-card-actions>
      </q-card>
    </q-dialog>
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
import type { BleDevice } from '@capacitor-community/bluetooth-le';

const stateMachine = useStateMachineStore();
useStateMachine();
const energyViz = useEnergyVisualization();

const sensorsRunning = ref(false);
const ledConnected = ref(false);
const cooldownInterval = ref<number | null>(null);

// Device selection dialog state
const deviceDialog = ref({
  show: false,
  scanning: false,
  devices: [] as BleDevice[],
});

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
    await Promise.allSettled([microphoneService.start(), accelerometerService.start()]).then(
      ([micro, accel]) => {
        if (micro.status === 'fulfilled') {
          microphoneStarted = true;
        } else {
          microphoneError =
            micro.reason instanceof Error ? micro.reason.message : String(micro.reason);
          console.warn('Microphone failed to start:', microphoneError);
        }

        if (accel.status === 'fulfilled') {
          accelerometerStarted = true;
        } else {
          accelerometerError =
            accel.reason instanceof Error ? accel.reason.message : String(accel.reason);
          console.warn('Accelerometer failed to start:', accelerometerError);
        }
      },
    );

    // Mark as running if at least one sensor started
    if (microphoneStarted || accelerometerStarted) {
      sensorsRunning.value = true;

      // Show warnings if one sensor failed
      if (!microphoneStarted && accelerometerStarted) {
        alert(
          'Microphone access failed. Accelerometer started successfully, but shout detection will not work. Please check microphone permissions.',
        );
      } else if (microphoneStarted && !accelerometerStarted) {
        alert(
          'Accelerometer access failed. Microphone started successfully, but punch detection will not work. Please check motion/accelerometer permissions.',
        );
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

async function scanAndShowDevices() {
  deviceDialog.value.show = true;
  await scanDevices();
}

async function scanDevices() {
  deviceDialog.value.scanning = true;
  deviceDialog.value.devices = [];

  try {
    await ledControllerService.initialize();
    const allDevices = await ledControllerService.scan();

    // Filter for devices named "ShadowLED"
    deviceDialog.value.devices = allDevices.filter(device =>
      device.name && device.name.includes('ShadowLED')
    );
  } catch (error) {
    console.error('Failed to scan for devices:', error);
    deviceDialog.value.devices = [];
  } finally {
    deviceDialog.value.scanning = false;
  }
}

async function connectToDevice(device: BleDevice) {
  try {
    await ledControllerService.connect(device.deviceId);
    ledConnected.value = true;
    deviceDialog.value.show = false;
  } catch (error) {
    console.error('Failed to connect to device:', error);
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

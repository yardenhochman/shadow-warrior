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
          <div class="text-center q-mt-md">
            <q-btn
              :label="suspendResumeButtonLabel"
              :color="suspendResumeButtonColor"
              :icon="suspendResumeButtonIcon"
              @click="toggleSuspendResume"
              size="lg"
            />
          </div>
        </q-card-section>
      </q-card>

      <!-- Real-time Energy Visualization -->
      <q-card class="q-mb-md">
        <q-card-section>
          <div class="text-h6 q-mb-md">Live Sensor Input</div>

          <div class="row q-col-gutter-md">
            <div class="col-12 col-sm-6">
              <div class="text-subtitle2 q-mb-sm">Microphone (Shout)</div>
              <q-linear-progress
                :value="energyViz.shoutAmplitude.value"
                size="32px"
                :color="getIntensityColor(energyViz.shoutAmplitude.value)"
                class="q-mt-sm"
                :instant-feedback="true"
                rounded
              >
                <div class="absolute-full flex flex-center">
                  <q-badge
                    color="white"
                    text-color="black"
                    :label="`${Math.round(energyViz.shoutAmplitude.value * 100)}%`"
                  />
                </div>
              </q-linear-progress>
            </div>
            <div class="col-12 col-sm-6">
              <div class="text-subtitle2 q-mb-sm">Accelerometer (Punch)</div>
              <q-linear-progress
                :value="energyViz.punchForce.value"
                size="32px"
                :color="getIntensityColor(energyViz.punchForce.value)"
                class="q-mt-sm"
                :instant-feedback="true"
                rounded
              >
                <div class="absolute-full flex flex-center">
                  <q-badge
                    color="white"
                    text-color="black"
                    :label="`${energyViz.punchMagnitude.value.toFixed(1)}G`"
                  />
                </div>
              </q-linear-progress>
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
              :instant-feedback="true"
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
              :instant-feedback="true"
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
                  <div class="text-h6">{{ metrics.punchMagnitude.toFixed(1) }}G</div>
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
                  :name="allControllers.length > 0 ? 'check_circle' : 'cancel'"
                  :color="allControllers.length > 0 ? 'positive' : 'negative'"
                />
              </q-item-section>
              <q-item-section>
                <q-item-label>LED Controller</q-item-label>
                <q-item-label caption>
                  {{ allControllers.length > 0
                    ? `${connectedCount}/${allControllers.length} connected`
                    : 'No controllers configured'
                  }}
                </q-item-label>
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

      <!-- Controllers -->
      <q-card class="q-mt-md">
        <q-card-section>
          <div class="text-h6 q-mb-md">LED Controllers</div>

          <q-list v-if="allControllers.length > 0">
            <q-item
              v-for="controller in allControllers"
              :key="controller.id"
              class="q-pa-sm"
            >
              <q-item-section avatar>
                <q-icon
                  :name="controller.type === ControllerType.WLED ? 'wifi' : 'bluetooth'"
                  :color="ledControllerService.isControllerConnected(controller.id) ? 'positive' : 'grey'"
                  size="sm"
                />
              </q-item-section>
              <q-item-section>
                <q-item-label class="text-body2">
                  {{ controller.type === ControllerType.BLE
                    ? ((controller.device as BleDevice).name || 'BLE Device')
                    : ((controller.device as WLEDController).name || `WLED Controller`)
                  }}
                </q-item-label>
                <q-item-label caption class="text-caption">
                  {{ controller.type === ControllerType.BLE
                    ? (controller.device as BleDevice).deviceId.substring(0, 8) + '...'
                    : (controller.device as WLEDController).ip
                  }}
                </q-item-label>
              </q-item-section>
              <q-item-section side>
                <div class="row q-gutter-xs">
                  <q-btn
                    :icon="ledControllerService.isControllerConnected(controller.id) ? 'power_off' : 'power'"
                    :color="ledControllerService.isControllerConnected(controller.id) ? 'negative' : 'positive'"
                    size="sm"
                    round
                    flat
                    @click="ledControllerService.isControllerConnected(controller.id) ? disconnectController(controller.id) : connectController(controller.id)"
                    :loading="connectingController === controller.id || disconnectingController === controller.id"
                  >
                    <q-tooltip>{{ ledControllerService.isControllerConnected(controller.id) ? 'Disconnect' : 'Connect' }}</q-tooltip>
                  </q-btn>
                  <q-btn
                    icon="delete"
                    color="negative"
                    size="sm"
                    round
                    flat
                    @click="removeController(controller.id)"
                  >
                    <q-tooltip>Remove Controller</q-tooltip>
                  </q-btn>
                </div>
              </q-item-section>
            </q-item>
          </q-list>

          <div v-else class="text-center q-pa-md q-mb-md">
            <q-icon name="lightbulb_outline" size="3em" color="grey" />
            <div class="text-subtitle1 q-mt-sm text-grey">No controllers configured</div>
          </div>

          <!-- Add Controller Section -->
          <q-separator class="q-my-md" />
          <div class="text-subtitle2 q-mb-sm">Add Controller</div>
          <div class="row q-gutter-sm">
            <div class="col">
              <q-input
                v-model="newControllerAddress"
                label="Controller Address (IP or BLE ID)"
                placeholder="192.168.1.100 or BLE:device-id"
                outlined
                dense
              />
            </div>
            <div class="col-auto">
              <q-btn
                label="Add"
                color="primary"
                @click="addControllerByAddress"
                :loading="addingController"
                :disable="!newControllerAddress || addingController"
              />
            </div>
            <div class="col-auto">
              <q-btn
                label="Scan"
                color="secondary"
                @click="scanForControllers"
                :disable="true"
                icon="search"
              >
                <q-tooltip>BLE scan feature coming soon</q-tooltip>
              </q-btn>
            </div>
          </div>
        </q-card-section>
      </q-card>

      <!-- Music Player -->
      <MusicPlayer class="q-mt-md" />
    </div>
  </q-page>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue';
import { useStateMachineStore } from 'src/stores/state-machine';
import { ArenaState } from 'src/types/state-machine';
import { accelerometerService } from 'src/services/accelerometer';
import { microphoneService } from 'src/services/microphone';
import { ledControllerService, ControllerType, type WLEDController } from 'src/services/led-controller';
import { speakerService } from 'src/services/speaker';
import { useStateMachine } from 'src/composables/use-state-machine';
import { useEnergyVisualization } from 'src/composables/use-energy-visualization';
import MusicPlayer from 'src/components/MusicPlayer.vue';
import type { BleDevice } from '@capacitor-community/bluetooth-le';
import { eventBus, Events } from 'src/services/event-bus';

const stateMachine = useStateMachineStore();
useStateMachine();
const energyViz = useEnergyVisualization();

const sensorsRunning = ref(false);
const ledConnected = ref(false);
const cooldownInterval = ref<number | null>(null);
const disconnectingController = ref<string | null>(null);
const connectingController = ref<string | null>(null);
const connectionCheckInterval = ref<number | null>(null);
const newControllerAddress = ref('');
const addingController = ref(false);
const controllerListVersion = ref(0); // Force reactivity trigger

// Event handlers for cleanup
const controllerConnectedHandler = () => {
  ledConnected.value = ledControllerService.getConnectedControllers().length > 0;
  controllerListVersion.value++; // Force UI update
};

const controllerDisconnectedHandler = () => {
  ledConnected.value = ledControllerService.getConnectedControllers().length > 0;
  controllerListVersion.value++; // Force UI update
};

const controllerAddedHandler = () => {
  console.log('Controller added event received, forcing UI update');
  controllerListVersion.value++; // Trigger reactivity
};

const controllerRemovedHandler = () => {
  console.log('Controller removed event received, forcing UI update');
  controllerListVersion.value++; // Trigger reactivity
};

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
    [ArenaState.SUSPENDED]: 'text-purple',
  };
  return colors[stateMachine.currentState];
});

const cooldownTimeDisplay = computed(() => {
  const remaining = stateMachine.cooldownTimeRemaining;
  const minutes = Math.floor(remaining / 60000);
  const seconds = Math.floor((remaining % 60000) / 1000);
  return `${minutes}:${seconds.toString().padStart(2, '0')}`;
});

const allControllers = computed(() => {
  // Use controllerListVersion to force reactivity when controllers are added/removed
  void controllerListVersion.value;
  const controllers = ledControllerService.getControllers();
  console.log('allControllers computed:', controllers.length, 'controllers');
  return controllers;
});

const connectedCount = computed(() => {
  // Use controllerListVersion to force reactivity
  void controllerListVersion.value;
  const count = ledControllerService.getConnectedControllers().length;
  console.log('connectedCount computed:', count);
  return count;
});

const suspendResumeButtonLabel = computed(() => {
  return stateMachine.currentState === ArenaState.SUSPENDED ? 'Resume Arena' : 'Suspend Arena';
});

const suspendResumeButtonColor = computed(() => {
  return stateMachine.currentState === ArenaState.SUSPENDED ? 'primary' : 'warning';
});

const suspendResumeButtonIcon = computed(() => {
  return stateMachine.currentState === ArenaState.SUSPENDED ? 'play_arrow' : 'pause';
});

function getIntensityColor(value: number): string {
  if (value < 0.3) return 'green';
  if (value < 0.6) return 'orange';
  return 'red';
}

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

function addControllerByAddress() {
  if (!newControllerAddress.value) return;

  addingController.value = true;
  try {
    // Add controller by address (IP for WLED)
    const controllerId = ledControllerService.addWledController(newControllerAddress.value);
    console.log('Added controller at', newControllerAddress.value);

    // Clear the input
    newControllerAddress.value = '';

    // Automatically try to connect to the newly added controller
    connectController(controllerId).catch(error => {
      console.error('Failed to auto-connect to new controller:', error);
    });
  } catch (error) {
    console.error('Failed to add controller:', error);

    // Check if it's an "already added" error
    if (error instanceof Error && error.message.includes('already added')) {
      // Extract IP from the input to find the existing controller
      const cleanIp = newControllerAddress.value.replace(/^https?:\/\//, '');
      const existingControllerId = `wled-${cleanIp}`;

      // Check if it's connected
      const isConnected = ledControllerService.isControllerConnected(existingControllerId);

      if (isConnected) {
        alert(`Controller at ${cleanIp} is already added and connected!`);
      } else {
        // Try to connect to the existing controller
        alert(`Controller at ${cleanIp} is already configured. Attempting to connect...`);
        connectController(existingControllerId).catch(connectError => {
          console.error('Failed to connect to existing controller:', connectError);
        });
      }

      // Clear the input
      newControllerAddress.value = '';
    } else {
      alert(`Failed to add controller: ${error instanceof Error ? error.message : String(error)}`);
    }
  } finally {
    addingController.value = false;
  }
}

function scanForControllers() {
  // Placeholder for future BLE scan functionality
  console.log('Scan functionality will be implemented later');
}

function forceState(state: string) {
  stateMachine.forceTransition(state as ArenaState);
}

function toggleSuspendResume() {
  if (stateMachine.currentState === ArenaState.SUSPENDED) {
    stateMachine.resumeFromSuspension();
  } else {
    stateMachine.suspend();
  }
}

async function disconnectController(controllerId: string) {
  disconnectingController.value = controllerId;
  try {
    await ledControllerService.disconnect(controllerId);
    ledConnected.value = ledControllerService.getConnectedControllers().length > 0;
  } catch (error) {
    console.error('Failed to disconnect controller:', error);
    alert(`Failed to disconnect controller: ${error instanceof Error ? error.message : String(error)}`);
  } finally {
    disconnectingController.value = null;
  }
}

async function connectController(controllerId: string) {
  connectingController.value = controllerId;
  try {
    await ledControllerService.connect(controllerId);
    ledConnected.value = ledControllerService.getConnectedControllers().length > 0;
  } catch (error) {
    console.error('Failed to connect controller:', error);
    alert(`Failed to connect controller: ${error instanceof Error ? error.message : String(error)}`);
  } finally {
    connectingController.value = null;
  }
}

async function removeController(controllerId: string) {
  try {
    // Disconnect if connected
    if (ledControllerService.isControllerConnected(controllerId)) {
      await ledControllerService.disconnect(controllerId);
    }
    // Remove the controller
    await ledControllerService.removeController(controllerId);
    ledConnected.value = ledControllerService.getConnectedControllers().length > 0;
  } catch (error) {
    console.error('Failed to remove controller:', error);
    alert(`Failed to remove controller: ${error instanceof Error ? error.message : String(error)}`);
  }
}

onMounted(async () => {
  console.log('DashboardPage onMounted - initializing services');

  // Listen for controller connection events BEFORE initializing
  // (initialize will load saved controllers and emit CONTROLLER_ADDED events)
  eventBus.on(Events.CONTROLLER_CONNECTED, controllerConnectedHandler);
  eventBus.on(Events.CONTROLLER_DISCONNECTED, controllerDisconnectedHandler);
  eventBus.on(Events.CONTROLLER_ADDED, controllerAddedHandler);
  eventBus.on(Events.CONTROLLER_REMOVED, controllerRemovedHandler);

  // Initialize services (this will load saved controllers and trigger CONTROLLER_ADDED events)
  await ledControllerService.initialize();
  console.log('LED controller service initialized');

  ledConnected.value = ledControllerService.getConnectedControllers().length > 0;
  console.log('Initial connected controllers:', ledConnected.value);

  // Automatically connect to all pre-configured controllers
  const allControllers = ledControllerService.getControllers();
  console.log('Pre-configured controllers found:', allControllers.length);
  if (allControllers.length > 0) {
    console.log('Attempting to connect to', allControllers.length, 'pre-configured controllers...');
    // Connect to all controllers in parallel
    await Promise.allSettled(
      allControllers.map(controller => ledControllerService.connect(controller.id))
    );
    ledConnected.value = ledControllerService.getConnectedControllers().length > 0;
    console.log('After auto-connect, connected controllers:', ledConnected.value);
  } else {
    console.log('No pre-configured controllers found, skipping auto-connect');
  }

  // Preload audio
  await speakerService.preloadAll();

  // Update cooldown timer
  cooldownInterval.value = window.setInterval(() => {
    if (stateMachine.isCooldown) {
      // Force reactivity update
      stateMachine.$patch({});
    }
  }, 1000);

  // Update connection status periodically
  connectionCheckInterval.value = window.setInterval(() => {
    ledConnected.value = ledControllerService.getConnectedControllers().length > 0;
  }, 2000);
});

onUnmounted(() => {
  if (cooldownInterval.value !== null) {
    clearInterval(cooldownInterval.value);
  }
  if (connectionCheckInterval.value !== null) {
    clearInterval(connectionCheckInterval.value);
  }

  // Clean up event listeners
  eventBus.off(Events.CONTROLLER_CONNECTED, controllerConnectedHandler);
  eventBus.off(Events.CONTROLLER_DISCONNECTED, controllerDisconnectedHandler);
  eventBus.off(Events.CONTROLLER_ADDED, controllerAddedHandler);
  eventBus.off(Events.CONTROLLER_REMOVED, controllerRemovedHandler);
});
</script>

<style scoped>
.q-card {
  border-radius: 12px;
}
</style>

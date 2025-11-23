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
                  :name="controller.device.isConnected ? 'wifi' : 'wifi_off'"
                  :color="controller.device.isConnected ? 'positive' : 'warning'"
                  size="sm"
                />
              </q-item-section>
              <q-item-section>
                <q-item-label class="text-body2">
                  {{ controller.device.name || 'WLED Controller' }}
                </q-item-label>
                <q-item-label caption class="text-caption">
                  {{ controller.device.ip }}
                  <span
                    v-if="controller.device.lastConnectivityCheck"
                    class="q-ml-sm text-grey-7"
                  >
                    • checked {{ formatTimeAgo(controller.device.lastConnectivityCheck) }}
                  </span>
                </q-item-label>
              </q-item-section>
              <q-item-section side>
                <div class="row q-gutter-xs">
                  <q-btn
                    v-if="!controller.device.isConnected"
                    icon="refresh"
                    color="info"
                    size="sm"
                    round
                    flat
                    :loading="reconnectingControllers.has(controller.id)"
                    @click="reconnectController(controller.id)"
                  >
                    <q-tooltip>Reconnect Controller</q-tooltip>
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

      <!-- UV Light Hosts -->
      <q-card class="q-mt-md">
        <q-card-section>
          <div class="text-h6 q-mb-md">UV Light Controllers</div>

          <q-list v-if="uvLightHosts.length > 0">
            <q-item
              v-for="(host, index) in uvLightHosts"
              :key="index"
              class="q-pa-sm"
            >
              <q-item-section avatar>
                <q-icon
                  name="lightbulb"
                  color="purple"
                  size="sm"
                />
              </q-item-section>
              <q-item-section>
                <q-item-label class="text-body2">
                  UV Smart Plug
                </q-item-label>
                <q-item-label caption class="text-caption">
                  {{ host }}
                </q-item-label>
              </q-item-section>
              <q-item-section side>
                <q-btn
                  icon="delete"
                  color="negative"
                  size="sm"
                  round
                  flat
                  @click="removeUVHost(host)"
                >
                  <q-tooltip>Remove UV Host</q-tooltip>
                </q-btn>
              </q-item-section>
            </q-item>
          </q-list>

          <div v-else class="text-center q-pa-md q-mb-md">
            <q-icon name="lightbulb" size="3em" color="grey" />
            <div class="text-subtitle1 q-mt-sm text-grey">No UV controllers configured</div>
          </div>

          <!-- Add UV Host Section -->
          <q-separator class="q-my-md" />
          <div class="text-subtitle2 q-mb-sm">Add UV Controller</div>
          <div class="row q-gutter-sm">
            <div class="col">
              <q-input
                v-model="newUVHost"
                label="UV Controller Address (IP)"
                placeholder="192.168.1.200"
                outlined
                dense
              />
            </div>
            <div class="col-auto">
              <q-btn
                label="Add"
                color="primary"
                @click="addUVHost"
                :loading="addingUVHost"
                :disable="!newUVHost || addingUVHost"
              />
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
import { ledControllerService } from 'src/services/led-controller';
import { speakerService } from 'src/services/speaker';
import { uvLightService } from 'src/services/uv-light';
import { useStateMachine } from 'src/composables/use-state-machine';
import { useEnergyVisualization } from 'src/composables/use-energy-visualization';
import MusicPlayer from 'src/components/MusicPlayer.vue';
import { eventBus, Events } from 'src/services/event-bus';

const stateMachine = useStateMachineStore();
useStateMachine();
const energyViz = useEnergyVisualization();

const sensorsRunning = ref(false);
const cooldownInterval = ref<number | null>(null);
const newControllerAddress = ref('');
const addingController = ref(false);
const controllerListVersion = ref(0); // Force reactivity trigger
const newUVHost = ref('');
const addingUVHost = ref(false);
const uvHostListVersion = ref(0); // Force reactivity trigger
const reconnectingControllers = ref(new Set<string>()); // Track reconnecting controllers

// Event handlers for cleanup
const controllerAddedHandler = () => {
  console.log('Controller added event received, forcing UI update');
  controllerListVersion.value++; // Trigger reactivity
};

const controllerRemovedHandler = () => {
  console.log('Controller removed event received, forcing UI update');
  controllerListVersion.value++; // Trigger reactivity
};

const uvHostAddedHandler = () => {
  console.log('UV host added event received, forcing UI update');
  uvHostListVersion.value++; // Trigger reactivity
};

const uvHostRemovedHandler = () => {
  console.log('UV host removed event received, forcing UI update');
  uvHostListVersion.value++; // Trigger reactivity
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
  const count = allControllers.value.length;
  console.log('connectedCount computed:', count);
  return count;
});

const uvLightHosts = computed(() => {
  // Use uvHostListVersion to force reactivity when hosts are added/removed
  void uvHostListVersion.value;
  const hosts = uvLightService.getHosts();
  console.log('uvLightHosts computed:', hosts.length, 'hosts');
  return hosts;
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

function formatTimeAgo(timestamp: number): string {
  const now = Date.now();
  const diffMs = now - timestamp;

  // Less than a minute
  if (diffMs < 60000) {
    return 'just now';
  }

  // Less than an hour
  if (diffMs < 3600000) {
    const minutes = Math.floor(diffMs / 60000);
    return `${minutes}m ago`;
  }

  // Less than a day
  if (diffMs < 86400000) {
    const hours = Math.floor(diffMs / 3600000);
    return `${hours}h ago`;
  }

  // Otherwise show date
  return 'long ago';
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
    console.log('Added controller at', newControllerAddress.value, 'with ID:', controllerId);

    // Clear the input
    newControllerAddress.value = '';
  } catch (error) {
    console.error('Failed to add controller:', error);

    // Check if it's an "already added" error
    if (error instanceof Error && error.message.includes('already added')) {
      // Extract IP from the input
      const cleanIp = newControllerAddress.value.replace(/^https?:\/\//, '');
      alert(`Controller at ${cleanIp} is already added!`);

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

function removeController(controllerId: string) {
  try {
    // Remove the controller
    ledControllerService.removeController(controllerId);
  } catch (error) {
    console.error('Failed to remove controller:', error);
    alert(`Failed to remove controller: ${error instanceof Error ? error.message : String(error)}`);
  }
}

async function reconnectController(controllerId: string) {
  try {
    // Mark as reconnecting
    reconnectingControllers.value.add(controllerId);

    // Test connectivity
    const isConnected = await ledControllerService.reconnectController(controllerId);

    // Show result
    const controller = allControllers.value.find((c) => c.id === controllerId);
    if (controller) {
      if (isConnected) {
        console.log(`✓ Successfully reconnected to ${controller.device.name}`);
        // The UI will update automatically through the event emitted by testControllerConnectivity
      } else {
        console.error(`Failed to reconnect to ${controller.device.name}`);
      }
    }
  } catch (error) {
    console.error('Error reconnecting to controller:', error);
  } finally {
    // Remove from reconnecting set
    reconnectingControllers.value.delete(controllerId);
  }
}

function addUVHost() {
  if (!newUVHost.value) return;

  addingUVHost.value = true;
  try {
    uvLightService.addHost(newUVHost.value);
    console.log('Added UV host:', newUVHost.value);
    newUVHost.value = '';
  } catch (error) {
    console.error('Failed to add UV host:', error);
    
    if (error instanceof Error && error.message.includes('already added')) {
      const cleanIp = newUVHost.value.replace(/^https?:\/\//, '');
      alert(`UV host ${cleanIp} is already added!`);
      newUVHost.value = '';
    } else {
      alert(`Failed to add UV host: ${error instanceof Error ? error.message : String(error)}`);
    }
  } finally {
    addingUVHost.value = false;
  }
}

function removeUVHost(host: string) {
  try {
    uvLightService.removeHost(host);
  } catch (error) {
    console.error('Failed to remove UV host:', error);
    alert(`Failed to remove UV host: ${error instanceof Error ? error.message : String(error)}`);
  }
}

onMounted(async () => {
  console.log('DashboardPage onMounted - initializing services');

  // Listen for controller events
  eventBus.on(Events.CONTROLLER_ADDED, controllerAddedHandler);
  eventBus.on(Events.CONTROLLER_REMOVED, controllerRemovedHandler);
  eventBus.on('uv-light:host-added', uvHostAddedHandler);
  eventBus.on('uv-light:host-removed', uvHostRemovedHandler);

  // Initialize services (this will load saved controllers)
  ledControllerService.initialize();
  console.log('LED controller service initialized');

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

  // Clean up event listeners
  eventBus.off(Events.CONTROLLER_ADDED, controllerAddedHandler);
  eventBus.off(Events.CONTROLLER_REMOVED, controllerRemovedHandler);
  eventBus.off('uv-light:host-added', uvHostAddedHandler);
  eventBus.off('uv-light:host-removed', uvHostRemovedHandler);
});
</script>

<style scoped>
.q-card {
  border-radius: 12px;
}
</style>

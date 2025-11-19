<template>
  <q-page class="q-pa-md">
    <div class="full-width" style="max-width: 800px; margin: 0 auto">
      <div class="text-h4 q-mb-md">Settings</div>

      <!-- State Machine Configuration -->
      <q-card class="q-mb-md">
        <q-card-section>
          <div class="text-h6">State Machine Configuration</div>

          <div class="q-mt-md">
            <div class="text-subtitle2">Warming Threshold</div>
            <q-slider
              v-model="config.warmingThreshold"
              :min="0"
              :max="100"
              :step="5"
              label
              label-always
              color="orange"
              @update:model-value="updateConfig"
            />
          </div>

          <div class="q-mt-md">
            <div class="text-subtitle2">Fight Threshold</div>
            <q-slider
              v-model="config.fightThreshold"
              :min="0"
              :max="100"
              :step="5"
              label
              label-always
              color="red"
              @update:model-value="updateConfig"
            />
          </div>

          <div class="q-mt-md">
            <div class="text-subtitle2">Cooldown Duration (minutes)</div>
            <q-slider
              v-model="cooldownMinutes"
              :min="1"
              :max="15"
              :step="1"
              label
              label-always
              color="blue"
              @update:model-value="updateCooldownDuration"
            />
          </div>

          <div class="q-mt-md">
            <div class="text-subtitle2">Warming Timeout (seconds)</div>
            <q-slider
              v-model="warmingSeconds"
              :min="10"
              :max="120"
              :step="10"
              label
              label-always
              color="orange"
              @update:model-value="updateWarmingTimeout"
            />
          </div>

          <div class="q-mt-md">
            <div class="text-subtitle2">Fight Timeout (minutes)</div>
            <q-slider
              v-model="fightMinutes"
              :min="1"
              :max="10"
              :step="1"
              label
              label-always
              color="red"
              @update:model-value="updateFightTimeout"
            />
          </div>

          <div class="q-mt-md">
            <div class="text-subtitle2">Fight Inactivity Timeout (seconds)</div>
            <q-slider
              v-model="fightInactivitySeconds"
              :min="10"
              :max="120"
              :step="10"
              label
              label-always
              color="red"
              @update:model-value="updateFightInactivityTimeout"
            />
          </div>

          <div class="q-mt-md">
            <div class="text-subtitle2">Warming Decay Rate (power/second)</div>
            <q-slider
              v-model="config.warmingDecayRate"
              :min="0"
              :max="20"
              :step="1"
              label
              label-always
              color="orange"
              @update:model-value="updateConfig"
            />
          </div>

          <div class="q-mt-md">
            <div class="text-subtitle2">Fight Decay Rate (power/second)</div>
            <q-slider
              v-model="config.fightDecayRate"
              :min="0"
              :max="10"
              :step="1"
              label
              label-always
              color="red"
              @update:model-value="updateConfig"
            />
          </div>

          <div class="q-mt-md">
            <div class="text-subtitle2">Warming Shout Scale Factor</div>
            <q-slider
              v-model="config.warmingShoutScale"
              :min="0"
              :max="30"
              :step="1"
              label
              label-always
              color="orange"
              @update:model-value="updateConfig"
            />
          </div>

          <div class="q-mt-md">
            <div class="text-subtitle2">Fight Punch Scale Factor</div>
            <q-slider
              v-model="config.fightPunchScale"
              :min="0"
              :max="30"
              :step="1"
              label
              label-always
              color="red"
              @update:model-value="updateConfig"
            />
          </div>

          <div class="q-mt-md">
            <div class="text-subtitle2">Fight Shout Scale Factor</div>
            <q-slider
              v-model="config.fightShoutScale"
              :min="0"
              :max="15"
              :step="0.5"
              label
              label-always
              color="orange"
              @update:model-value="updateConfig"
            />
          </div>

          <div class="q-mt-md">
            <div class="text-subtitle2">Presence Detection Threshold</div>
            <q-slider
              v-model="config.presenceDetectionThreshold"
              :min="0.1"
              :max="1.0"
              :step="0.05"
              label
              label-always
              color="primary"
              @update:model-value="updateConfig"
            />
          </div>
        </q-card-section>
      </q-card>

      <!-- Accelerometer Configuration -->
      <q-card class="q-mb-md">
        <q-card-section>
          <div class="text-h6">Accelerometer Configuration</div>

          <div class="q-mt-md">
            <div class="text-subtitle2">Punch Threshold (G-force)</div>
            <q-slider
              v-model="accelConfig.threshold"
              :min="0.5"
              :max="5.0"
              :step="0.1"
              label
              label-always
              color="primary"
              @update:model-value="updateAccelConfig"
            />
          </div>

          <div class="q-mt-md">
            <div class="text-subtitle2">Detection Cooldown (ms)</div>
            <q-slider
              v-model="accelConfig.cooldownMs"
              :min="50"
              :max="500"
              :step="50"
              label
              label-always
              color="primary"
              @update:model-value="updateAccelConfig"
            />
          </div>

          <div class="q-mt-md">
            <div class="text-subtitle2">Baseline Smoothing Alpha</div>
            <q-slider
              v-model="accelConfig.baselineAlpha"
              :min="0.001"
              :max="0.1"
              :step="0.001"
              label
              label-always
              color="primary"
              @update:model-value="updateAccelConfig"
            />
            <div class="text-caption text-grey q-mt-sm">
              EWMA factor for baseline drift correction. Very slow adaptation (0.005) prevents gradual drift from device movement while maintaining long-term stability.
            </div>
          </div>

          <div class="q-mt-md">
            <div class="text-subtitle2">Accelerometer Smoothing Alpha</div>
            <q-slider
              v-model="accelConfig.accelAlpha"
              :min="0.1"
              :max="0.9"
              :step="0.05"
              label
              label-always
              color="primary"
              @update:model-value="updateAccelConfig"
            />
            <div class="text-caption text-grey q-mt-sm">
              EWMA factor for accelerometer trend detection. Moderate smoothing (0.4) reduces noise while maintaining responsiveness for punch detection.
            </div>
          </div>

          <div class="q-mt-md">
            <div class="text-subtitle2">Baseline X (m/s²)</div>
            <q-slider
              v-model="accelConfig.baselineX"
              :min="0"
              :max="2"
              :step="0.1"
              label
              label-always
              color="primary"
              @update:model-value="updateAccelConfig"
            />
            <div class="text-caption text-grey q-mt-sm">
              Expected X-axis acceleration when device is at rest (typically 0).
            </div>
          </div>

          <div class="q-mt-md">
            <div class="text-subtitle2">Baseline Y (m/s²)</div>
            <q-slider
              v-model="accelConfig.baselineY"
              :min="0"
              :max="2"
              :step="0.1"
              label
              label-always
              color="primary"
              @update:model-value="updateAccelConfig"
            />
            <div class="text-caption text-grey q-mt-sm">
              Expected Y-axis acceleration when device is at rest (typically 0).
            </div>
          </div>

          <div class="q-mt-md">
            <div class="text-subtitle2">Baseline Z (m/s²)</div>
            <q-slider
              v-model="accelConfig.baselineZ"
              :min="0"
              :max="12"
              :step="0.1"
              label
              label-always
              color="primary"
              @update:model-value="updateAccelConfig"
            />
            <div class="text-caption text-grey q-mt-sm">
              Expected Z-axis acceleration when device is at rest (typically 9.81 for gravity).
            </div>
          </div>
        </q-card-section>
      </q-card>

      <!-- Microphone Configuration -->
      <q-card class="q-mb-md">
        <q-card-section>
          <div class="text-h6">Microphone Configuration</div>

          <div class="q-mt-md">
            <div class="text-subtitle2">Shout Threshold</div>
            <q-slider
              v-model="micConfig.threshold"
              :min="0.1"
              :max="1.0"
              :step="0.05"
              label
              label-always
              color="primary"
              @update:model-value="updateMicConfig"
            />
          </div>

          <div class="q-mt-md">
            <div class="text-subtitle2">Smoothing Factor</div>
            <q-slider
              v-model="micConfig.smoothingFactor"
              :min="0"
              :max="1.0"
              :step="0.1"
              label
              label-always
              color="primary"
              @update:model-value="updateMicConfig"
            />
          </div>

          <div class="q-mt-md">
            <div class="text-subtitle2">Update Interval (ms)</div>
            <q-slider
              v-model="micConfig.updateIntervalMs"
              :min="20"
              :max="200"
              :step="10"
              label
              label-always
              color="primary"
              @update:model-value="updateMicConfig"
            />
          </div>

          <div class="q-mt-md">
            <div class="text-subtitle2">Gain Multiplier</div>
            <q-slider
              v-model="micConfig.gain"
              :min="0.1"
              :max="10.0"
              :step="0.1"
              label
              label-always
              color="primary"
              @update:model-value="updateMicConfig"
            />
            <div class="text-caption text-grey q-mt-sm">
              Amplify microphone input. Default 1.5x. Lower for loud environments, higher for quiet voices.
            </div>
          </div>

          <div class="q-mt-md">
            <div class="text-subtitle2">Audio Processing</div>
            <div class="text-caption text-grey q-mb-sm">
              Changes require microphone restart to apply
            </div>

            <q-toggle
              v-model="micConfig.echoCancellation"
              label="Echo Cancellation"
              color="primary"
              @update:model-value="updateMicConfig"
            />
            <div class="text-caption text-grey q-ml-md q-mb-sm">
              Remove echo feedback (disable for raw input)
            </div>

            <q-toggle
              v-model="micConfig.noiseSuppression"
              label="Noise Suppression"
              color="primary"
              @update:model-value="updateMicConfig"
            />
            <div class="text-caption text-grey q-ml-md q-mb-sm">
              Reduce background noise (may affect shout detection)
            </div>

            <q-toggle
              v-model="micConfig.autoGainControl"
              label="Auto Gain Control"
              color="primary"
              @update:model-value="updateMicConfig"
            />
            <div class="text-caption text-grey q-ml-md">
              Automatic volume leveling (disable for manual gain control)
            </div>
          </div>

          <div class="q-mt-md">
            <div class="row items-center">
              <div class="text-subtitle2">Enable Band-pass Filter (150-800 Hz)</div>
              <q-icon name="help_outline" class="q-ml-sm cursor-help">
                <q-tooltip anchor="top middle">
                  Higher low-cut removes more wind rumble but may attenuate low-pitched voices.
                  Increase low cutoff to reduce wind; lower it to preserve deeper shouts.
                </q-tooltip>
              </q-icon>
            </div>

            <q-toggle
              v-model="micConfig.filterEnabled"
              label="Enable 150-800 Hz filter"
              color="primary"
              @update:model-value="updateMicConfig"
            />
            <div class="text-caption text-grey q-mt-sm">
              Higher low-cut removes more wind rumble but may attenuate low-pitched voices. Increase
              low cutoff to reduce wind; lower it to preserve deeper shouts.
            </div>
          </div>

          <div v-if="micConfig.filterEnabled" class="q-mt-md">
            <div class="text-subtitle2">Filter Low Cutoff (Hz)</div>
            <q-slider
              v-model="micConfig.filterLowHz"
              :min="20"
              :max="400"
              :step="1"
              label
              label-always
              color="primary"
              @update:model-value="updateMicConfig"
            />

            <div class="q-mt-md">
              <div class="text-subtitle2">Filter High Cutoff (Hz)</div>
              <q-slider
                v-model="micConfig.filterHighHz"
                :min="300"
                :max="2000"
                :step="5"
                label
                label-always
                color="primary"
                @update:model-value="updateMicConfig"
              />
            </div>
          </div>
        </q-card-section>
      </q-card>

      <!-- UV Light Configuration -->
      <q-card class="q-mb-md">
        <q-card-section>
          <div class="text-h6">UV Light Configuration</div>

          <q-toggle
            v-model="uvConfig.enabled"
            label="Enable UV Light Control"
            color="primary"
            @update:model-value="updateUVConfig"
          />

          <div v-if="uvConfig.enabled" class="q-mt-md">
            <q-input
              v-model="uvConfig.relayUrl"
              label="Relay URL"
              hint="e.g., http://192.168.1.100"
              outlined
              @update:model-value="updateUVConfig"
            />

            <q-input
              v-model="uvConfig.onEndpoint"
              label="ON Endpoint"
              hint="e.g., /relay/on"
              outlined
              class="q-mt-md"
              @update:model-value="updateUVConfig"
            />

            <q-input
              v-model="uvConfig.offEndpoint"
              label="OFF Endpoint"
              hint="e.g., /relay/off"
              outlined
              class="q-mt-md"
              @update:model-value="updateUVConfig"
            />
          </div>
        </q-card-section>
      </q-card>

      <!-- Schedule Configuration -->
      <q-card class="q-mb-md">
        <q-card-section>
          <div class="text-h6">Schedule Configuration</div>
          <div class="text-caption text-grey q-mb-md">
            Configure daily active hours. Arena will suspend outside these times.
          </div>

          <q-toggle
            v-model="scheduleConfig.enabled"
            label="Enable Schedule"
            color="primary"
            @update:model-value="updateScheduleConfig"
          />

          <div v-if="scheduleConfig.enabled" class="q-mt-md">
            <div class="q-mt-md">
              <div class="text-subtitle2">Daily Active Start Time</div>
              <q-input
                v-model="scheduleConfig.dailyActiveStart"
                type="time"
                outlined
                hint="Arena becomes active at this time"
                @update:model-value="updateScheduleConfig"
              />
            </div>

            <div class="q-mt-md">
              <div class="text-subtitle2">Daily Active End Time</div>
              <q-input
                v-model="scheduleConfig.dailyActiveEnd"
                type="time"
                outlined
                hint="Arena suspends at this time"
                @update:model-value="updateScheduleConfig"
              />
            </div>
          </div>
        </q-card-section>
      </q-card>

      <!-- BLE Arena Broadcasting Configuration -->
      <q-card class="q-mb-md">
        <q-card-section>
          <div class="text-h6">Arena Broadcasting (BLE)</div>
          <div class="text-caption text-grey q-mb-md">
            Enable Bluetooth broadcasting to allow monitor devices to remotely view arena state.
          </div>

          <q-toggle
            v-model="bleConfig.enabled"
            label="Enable Arena Broadcasting"
            color="primary"
            @update:model-value="updateBleConfig"
          />

          <div v-if="bleConfig.enabled" class="q-mt-md">
            <q-input
              v-model="bleConfig.arenaName"
              label="Arena Name (Broadcast Name)"
              hint="Name shown to scanning devices"
              outlined
              maxlength="30"
              counter
              @update:model-value="updateBleConfig"
            />

            <div v-if="bleStatus" class="q-mt-md">
              <div class="text-subtitle2">Status</div>
              <q-chip
                :color="bleStatus.isAdvertising ? 'positive' : 'warning'"
                text-color="white"
                icon="bluetooth"
              >
                {{ bleStatus.isAdvertising ? 'Broadcasting' : 'Idle' }}
              </q-chip>
              <q-chip v-if="bleStatus.connectedDevices > 0" color="info" text-color="white" icon="devices">
                {{ bleStatus.connectedDevices }} device{{ bleStatus.connectedDevices !== 1 ? 's' : '' }}
                connected
              </q-chip>
            </div>
          </div>
        </q-card-section>
      </q-card>

      <!-- HTTP Server Configuration -->
      <q-card class="q-mb-md">
        <q-card-section>
          <div class="text-h6">HTTP Server Configuration</div>
          <div class="text-caption text-grey q-mb-md">
            Enable HTTP API to expose arena state to monitoring devices on the local network.
          </div>

          <q-toggle
            v-model="httpServerConfig.enabled"
            label="Enable HTTP Server (port 8080)"
            color="primary"
            @update:model-value="updateHttpServerConfig"
          />

          <div v-if="httpServerConfig.enabled" class="q-mt-md">
            <div class="q-mt-md">
              <div class="text-subtitle2">API Endpoints</div>
              <div class="text-caption q-mt-sm">
                <ul class="q-pl-md q-my-sm">
                  <li>/api/health - Server health check</li>
                  <li>/api/arena-state - Current arena state and metrics</li>
                  <li>/api/arena-config - Current configuration</li>
                  <li>/api/arena-history - State transition history</li>
                  <li>/api/info - Server information</li>
                </ul>
              </div>
              <p class="text-caption text-warning q-mt-md">
                Note: HTTP server settings take effect after app restart. Find your device's IP address in device settings or use a network scanner to access: http://&lt;device-ip&gt;:8080/api/arena-state
              </p>
            </div>
          </div>
        </q-card-section>
      </q-card>

      <!-- Actions -->
      <div class="row q-col-gutter-sm">
        <div class="col-6">
          <q-btn
            label="Save to Storage"
            color="primary"
            icon="save"
            @click="saveSettings"
            class="full-width"
          />
        </div>
        <div class="col-6">
          <q-btn
            label="Reset to Defaults"
            color="negative"
            icon="restore"
            outline
            @click="() => void resetSettings()"
            class="full-width"
          />
        </div>
      </div>
    </div>
  </q-page>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { useStateMachineStore } from 'src/stores/state-machine';
import { accelerometerService } from 'src/services/accelerometer';
import { microphoneService } from 'src/services/microphone';
import { uvLightService } from 'src/services/uv-light';
import { scheduleService } from 'src/services/schedule';
import { useQuasar } from 'quasar';
import { bleArenaPeripheral } from 'src/services/ble-arena-peripheral';

const $q = useQuasar();
const stateMachine = useStateMachineStore();

const config = ref({
  warmingThreshold: 80,
  fightThreshold: 100,
  cooldownDuration: 300000,
  warmingTimeout: 60000,
  fightTimeout: 180000,
  fightInactivityTimeout: 60000,
  warmingDecayRate: 5,
  fightDecayRate: 3,
  warmingShoutScale: 10,
  fightPunchScale: 10,
  fightShoutScale: 2,
  presenceDetectionThreshold: 0.3,
});

const accelConfig = ref({
  threshold: 2.0,
  cooldownMs: 200,
  baselineAlpha: 0.01,
  baselineX: 0,
  baselineY: 0,
  baselineZ: 9.81,
  accelAlpha: 0.3,
});

const micConfig = ref({
  threshold: 0.3,
  smoothingFactor: 0.8,
  updateIntervalMs: 50,
  gain: 1.5,
  echoCancellation: false,
  noiseSuppression: false,
  autoGainControl: false,
  filterEnabled: true,
  filterLowHz: 150,
  filterHighHz: 800,
});

const uvConfig = ref({
  enabled: false,
  relayUrl: 'http://192.168.1.100',
  onEndpoint: '/relay/on',
  offEndpoint: '/relay/off',
});

const scheduleConfig = ref({
  enabled: false,
  dailyActiveStart: '09:00',
  dailyActiveEnd: '22:00',
});

const httpServerConfig = ref({
  enabled: false,
});

const bleConfig = ref({
  enabled: false,
  arenaName: 'Shadow Warrior Arena',
});

const bleStatus = ref<{ isAdvertising: boolean; connectedDevices: number } | null>(null);

const cooldownMinutes = ref(5);
const warmingSeconds = ref(60);
const fightMinutes = ref(3);
const fightInactivitySeconds = ref(60);

function updateConfig() {
  stateMachine.updateConfig({
    warmingThreshold: config.value.warmingThreshold,
    fightThreshold: config.value.fightThreshold,
    warmingDecayRate: config.value.warmingDecayRate,
    fightDecayRate: config.value.fightDecayRate,
    warmingShoutScale: config.value.warmingShoutScale,
    fightPunchScale: config.value.fightPunchScale,
    fightShoutScale: config.value.fightShoutScale,
    presenceDetectionThreshold: config.value.presenceDetectionThreshold,
  });
}

function updateCooldownDuration() {
  config.value.cooldownDuration = cooldownMinutes.value * 60000;
  stateMachine.updateConfig({
    cooldownDuration: config.value.cooldownDuration,
  });
}

function updateWarmingTimeout() {
  config.value.warmingTimeout = warmingSeconds.value * 1000;
  stateMachine.updateConfig({
    warmingTimeout: config.value.warmingTimeout,
  });
}

function updateFightTimeout() {
  config.value.fightTimeout = fightMinutes.value * 60000;
  stateMachine.updateConfig({
    fightTimeout: config.value.fightTimeout,
  });
}

function updateFightInactivityTimeout() {
  config.value.fightInactivityTimeout = fightInactivitySeconds.value * 1000;
  stateMachine.updateConfig({
    fightInactivityTimeout: config.value.fightInactivityTimeout,
  });
}

function updateAccelConfig() {
  void accelerometerService.updateConfig(accelConfig.value);
}

function updateMicConfig() {
  microphoneService.updateConfig(micConfig.value);
}

function updateUVConfig() {
  uvLightService.updateConfig(uvConfig.value);
}

async function updateScheduleConfig() {
  stateMachine.updateScheduleConfig(scheduleConfig.value);
  await scheduleService.updateSchedule(scheduleConfig.value);
}

function updateHttpServerConfig() {
  localStorage.setItem(
    'shadow-warrior-http-server-enabled',
    httpServerConfig.value.enabled.toString()
  );
  $q.notify({
    type: 'info',
    message: httpServerConfig.value.enabled
      ? 'HTTP server will be enabled on next app restart'
      : 'HTTP server will be disabled on next app restart',
    position: 'top',
  });
}

async function updateBleConfig() {
  localStorage.setItem(
    'shadow-warrior-ble-peripheral-enabled',
    bleConfig.value.enabled.toString()
  );
  localStorage.setItem(
    'shadow-warrior-ble-arena-name',
    bleConfig.value.arenaName
  );

  bleArenaPeripheral.setArenaName(bleConfig.value.arenaName);
  await bleArenaPeripheral.toggle(bleConfig.value.enabled);

  bleStatus.value = bleArenaPeripheral.getStatus();

  $q.notify({
    type: 'positive',
    message: bleConfig.value.enabled ? 'Arena broadcasting enabled' : 'Arena broadcasting disabled',
    position: 'top',
  });
}

function saveSettings() {
  try {
    const settings = {
      stateMachine: config.value,
      accelerometer: accelConfig.value,
      microphone: micConfig.value,
      uvLight: uvConfig.value,
      schedule: scheduleConfig.value,
      httpServer: httpServerConfig.value,
      ble: bleConfig.value,
    };

    console.log('Attempting to save settings...');
    console.log('Settings object:', settings);

    const settingsJson = JSON.stringify(settings);
    console.log('Settings JSON length:', settingsJson.length, 'bytes');
    console.log('Settings JSON preview:', settingsJson.substring(0, 200));

    localStorage.setItem('shadow-warrior-settings', settingsJson);
    console.log('Settings saved successfully to localStorage');

    try {
      $q.notify({
        type: 'positive',
        message: 'Settings saved successfully',
        position: 'top',
      });
    } catch (notifyError) {
      console.warn('Failed to show notification:', notifyError);
    }
  } catch (error) {
    console.error('Failed to save settings - Error type:', typeof error);
    console.error('Failed to save settings - Error:', error);
    console.error('Failed to save settings - Error string:', String(error));
    console.error('Failed to save settings - Error JSON:', JSON.stringify(error, Object.getOwnPropertyNames(error)));

    let errorMessage = 'Unknown error';
    if (error instanceof Error) {
      errorMessage = error.message;
    } else if (error && typeof error === 'object') {
      errorMessage = JSON.stringify(error, Object.getOwnPropertyNames(error));
    } else {
      errorMessage = String(error);
    }

    try {
      $q.notify({
        type: 'negative',
        message: `Failed to save settings: ${errorMessage}`,
        position: 'top',
        timeout: 5000,
      });
    } catch (notifyError) {
      console.error('Failed to show error notification:', notifyError);
    }
  }
}

async function resetSettings() {
  // Reset to defaults
  config.value = {
    warmingThreshold: 80,
    fightThreshold: 100,
    cooldownDuration: 300000,
    warmingTimeout: 60000,
    fightTimeout: 180000,
    fightInactivityTimeout: 60000,
    warmingDecayRate: 5,
    fightDecayRate: 5,
    warmingShoutScale: 4,
    fightPunchScale: 4,
    fightShoutScale: 0.5,
    presenceDetectionThreshold: 0.3,
  };

  accelConfig.value = {
    threshold: 2.0,
    cooldownMs: 200,
    baselineAlpha: 0.005,
    baselineX: 0,
    baselineY: 0,
    baselineZ: 9.81,
    accelAlpha: 0.4,
  };

  micConfig.value = {
    threshold: 0.3,
    smoothingFactor: 0.8,
    updateIntervalMs: 50,
    gain: 1.5,
    echoCancellation: false,
    noiseSuppression: false,
    autoGainControl: false,
    filterEnabled: true,
    filterLowHz: 150,
    filterHighHz: 800,
  };

  uvConfig.value = {
    enabled: false,
    relayUrl: 'http://192.168.1.100',
    onEndpoint: '/relay/on',
    offEndpoint: '/relay/off',
  };

  scheduleConfig.value = {
    enabled: false,
    dailyActiveStart: '09:00',
    dailyActiveEnd: '22:00',
  };

  httpServerConfig.value = {
    enabled: false,
  };

  bleConfig.value = {
    enabled: false,
    arenaName: 'Shadow Warrior Arena',
  };

  cooldownMinutes.value = 5;
  warmingSeconds.value = 60;
  fightMinutes.value = 3;

  updateConfig();
  updateAccelConfig();
  updateMicConfig();
  updateUVConfig();
  await updateScheduleConfig();
  updateHttpServerConfig();

  $q.notify({
    type: 'info',
    message: 'Settings reset to defaults',
    position: 'top',
  });
}

async function loadSettings() {
  const saved = localStorage.getItem('shadow-warrior-settings');
  if (saved) {
    try {
      const settings = JSON.parse(saved);

      if (settings.stateMachine) {
        config.value = settings.stateMachine;
        cooldownMinutes.value = settings.stateMachine.cooldownDuration / 60000;
        warmingSeconds.value = settings.stateMachine.warmingTimeout / 1000;
        fightMinutes.value = settings.stateMachine.fightTimeout / 60000;
        updateConfig();
      }

      if (settings.accelerometer) {
        accelConfig.value = settings.accelerometer;
        updateAccelConfig();
      }

      if (settings.microphone) {
        micConfig.value = settings.microphone;
        updateMicConfig();
      }

      if (settings.uvLight) {
        uvConfig.value = settings.uvLight;
        updateUVConfig();
      }

      if (settings.schedule) {
        scheduleConfig.value = settings.schedule;
        await updateScheduleConfig();
      }

      if (settings.httpServer) {
        httpServerConfig.value = settings.httpServer;
      }

      if (settings.ble) {
        bleConfig.value = settings.ble;
      }
    } catch (error) {
      console.error('Failed to load settings:', error);
    }
  } else {
    // Load from state machine store if no saved settings
    config.value.warmingThreshold = stateMachine.config.warmingThreshold;
    config.value.fightThreshold = stateMachine.config.fightThreshold;
    config.value.cooldownDuration = stateMachine.config.cooldownDuration;
    config.value.warmingTimeout = stateMachine.config.warmingTimeout;
    config.value.fightTimeout = stateMachine.config.fightTimeout;
    config.value.fightInactivityTimeout = stateMachine.config.fightInactivityTimeout;
    config.value.warmingDecayRate = stateMachine.config.warmingDecayRate;
    config.value.fightDecayRate = stateMachine.config.fightDecayRate;
    config.value.warmingShoutScale = stateMachine.config.warmingShoutScale;
    config.value.fightPunchScale = stateMachine.config.fightPunchScale;
    config.value.fightShoutScale = stateMachine.config.fightShoutScale;
    config.value.presenceDetectionThreshold = stateMachine.config.presenceDetectionThreshold;
    cooldownMinutes.value = stateMachine.config.cooldownDuration / 60000;
    warmingSeconds.value = stateMachine.config.warmingTimeout / 1000;
    fightMinutes.value = stateMachine.config.fightTimeout / 60000;
    fightInactivitySeconds.value = stateMachine.config.fightInactivityTimeout / 1000;
  }
}

onMounted(async () => {
  await loadSettings();
  // Load HTTP server enabled flag from localStorage
  const httpServerEnabled = localStorage.getItem('shadow-warrior-http-server-enabled');
  if (httpServerEnabled !== null) {
    httpServerConfig.value.enabled = httpServerEnabled === 'true';
  }

  // Load BLE config from localStorage
  const blePeripheralEnabled = localStorage.getItem('shadow-warrior-ble-peripheral-enabled');
  const bleArenaName = localStorage.getItem('shadow-warrior-ble-arena-name');
  if (blePeripheralEnabled !== null) {
    bleConfig.value.enabled = blePeripheralEnabled === 'true';
  }
  if (bleArenaName !== null) {
    bleConfig.value.arenaName = bleArenaName;
  }

  // Update BLE status
  bleStatus.value = bleArenaPeripheral.getStatus();
});
</script>

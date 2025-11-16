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
});

const micConfig = ref({
  threshold: 0.3,
  smoothingFactor: 0.8,
  updateIntervalMs: 50,
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
  accelerometerService.updateConfig(accelConfig.value);
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

function saveSettings() {
  const settings = {
    stateMachine: config.value,
    accelerometer: accelConfig.value,
    microphone: micConfig.value,
    uvLight: uvConfig.value,
    schedule: scheduleConfig.value,
  };

  localStorage.setItem('shadow-warrior-settings', JSON.stringify(settings));

  $q.notify({
    type: 'positive',
    message: 'Settings saved successfully',
    position: 'top',
  });
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
  };

  micConfig.value = {
    threshold: 0.3,
    smoothingFactor: 0.8,
    updateIntervalMs: 50,
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

  cooldownMinutes.value = 5;
  warmingSeconds.value = 60;
  fightMinutes.value = 3;

  updateConfig();
  updateAccelConfig();
  updateMicConfig();
  updateUVConfig();
  await updateScheduleConfig();

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

onMounted(() => {
  void loadSettings();
});
</script>

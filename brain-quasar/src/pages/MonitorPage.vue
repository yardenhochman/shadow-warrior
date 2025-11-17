<template>
  <q-page class="q-pa-md">
    <!-- Connection Status Banner -->
    <q-banner v-if="!isConnected" class="bg-warning text-white q-mb-md">
      <template v-slot:avatar>
        <q-icon
          :name="connectionStatusIcon"
          :animation="connectionStatus !== 'disconnected' ? 'spin' : undefined"
        />
      </template>
      <div>
        <div>{{ connectionStatusText }}</div>
        <div v-if="connectionStatus !== 'disconnected'" class="text-caption">
          {{ connectionStatusDetail }}
        </div>
      </div>
      <template v-slot:action>
        <q-btn
          :label="connectionStatus === 'disconnected' ? 'Scan & Connect' : 'Cancel'"
          flat
          @click="handleConnectionAction"
          :loading="connectionStatus === 'scanning' || connectionStatus === 'connecting'"
          :disable="connectionStatus !== 'disconnected'"
        />
      </template>
    </q-banner>

    <q-banner v-else class="bg-positive text-white q-mb-md">
      <template v-slot:avatar>
        <q-icon name="bluetooth_connected" />
      </template>
      Connected: {{ connectedArenaName }}
      <template v-slot:action>
        <q-btn label="Disconnect" flat @click="disconnect" />
      </template>
    </q-banner>

    <!-- Arena State Display (reuse existing components) -->
    <div v-if="remoteArenaState" class="full-width" style="max-width: 1200px; margin: 0 auto">
      <!-- Current State -->
      <q-card class="q-mb-md">
        <q-card-section>
          <div class="text-h6">Current State: {{ remoteArenaState.currentState }}</div>
          <div class="text-caption text-grey">Last update: {{ lastUpdateTime }}</div>
        </q-card-section>
      </q-card>

      <!-- Metrics -->
      <q-card class="q-mb-md">
        <q-card-section>
          <div class="text-h6">Metrics</div>

          <div class="row q-col-gutter-md q-mt-sm">
            <div class="col-xs-12 col-sm-6">
              <div class="text-subtitle2">Shout Amplitude</div>
              <q-linear-progress
                :value="remoteArenaState.metrics.shoutAmplitude"
                color="orange"
                class="q-mt-sm"
              />
              <div class="text-caption q-mt-sm">
                {{ (remoteArenaState.metrics.shoutAmplitude * 100).toFixed(1) }}%
              </div>
            </div>

            <div class="col-xs-12 col-sm-6">
              <div class="text-subtitle2">Punch Force</div>
              <q-linear-progress
                :value="Math.min(remoteArenaState.metrics.punchForce / 500, 1)"
                color="red"
                class="q-mt-sm"
              />
              <div class="text-caption q-mt-sm">
                {{ remoteArenaState.metrics.punchForce.toFixed(1) }}G
              </div>
            </div>

            <div class="col-xs-12 col-sm-6">
              <div class="text-subtitle2">Warming Power</div>
              <q-linear-progress
                :value="remoteArenaState.metrics.warmingPower / 100"
                color="orange"
                class="q-mt-sm"
              />
              <div class="text-caption q-mt-sm">
                {{ Math.round(remoteArenaState.metrics.warmingPower) }}%
              </div>
            </div>

            <div class="col-xs-12 col-sm-6">
              <div class="text-subtitle2">Fight Power</div>
              <q-linear-progress
                :value="remoteArenaState.metrics.fightPower / 100"
                color="red"
                class="q-mt-sm"
              />
              <div class="text-caption q-mt-sm">
                {{ Math.round(remoteArenaState.metrics.fightPower) }}%
              </div>
            </div>
          </div>
        </q-card-section>
      </q-card>

      <!-- Progress -->
      <q-card class="q-mb-md">
        <q-card-section>
          <div class="text-h6">Progress</div>

          <div class="row q-col-gutter-md q-mt-sm">
            <div class="col-xs-12 col-sm-6">
              <div class="text-subtitle2">Warming Progress</div>
              <q-linear-progress
                :value="remoteArenaState.progress.warmingProgress / 100"
                color="orange"
                class="q-mt-sm"
              />
              <div class="text-caption q-mt-sm">
                {{ remoteArenaState.progress.warmingProgress }}%
              </div>
            </div>

            <div class="col-xs-12 col-sm-6">
              <div class="text-subtitle2">Fight Progress</div>
              <q-linear-progress
                :value="remoteArenaState.progress.fightProgress / 100"
                color="red"
                class="q-mt-sm"
              />
              <div class="text-caption q-mt-sm">
                {{ remoteArenaState.progress.fightProgress }}%
              </div>
            </div>
          </div>
        </q-card-section>
      </q-card>

      <!-- Timers -->
      <q-card class="q-mb-md">
        <q-card-section>
          <div class="text-h6">Timers</div>

          <q-list separator>
            <q-item>
              <q-item-section>
                <q-item-label>Fight Elapsed</q-item-label>
                <q-item-label caption>{{ formatTime(remoteArenaState.timers.fightElapsed) }}</q-item-label>
              </q-item-section>
              <q-item-section side>
                <div class="text-subtitle2">{{ (remoteArenaState.timers.fightElapsed / 1000).toFixed(1) }}s</div>
              </q-item-section>
            </q-item>

            <q-item>
              <q-item-section>
                <q-item-label>Warming Elapsed</q-item-label>
                <q-item-label caption>{{ formatTime(remoteArenaState.timers.warmingElapsed) }}</q-item-label>
              </q-item-section>
              <q-item-section side>
                <div class="text-subtitle2">{{ (remoteArenaState.timers.warmingElapsed / 1000).toFixed(1) }}s</div>
              </q-item-section>
            </q-item>

            <q-item>
              <q-item-section>
                <q-item-label>Cooldown Remaining</q-item-label>
                <q-item-label caption>{{ formatTime(remoteArenaState.timers.cooldownRemaining) }}</q-item-label>
              </q-item-section>
              <q-item-section side>
                <div class="text-subtitle2">{{ (remoteArenaState.timers.cooldownRemaining / 1000).toFixed(1) }}s</div>
              </q-item-section>
            </q-item>
          </q-list>
        </q-card-section>
      </q-card>
    </div>

    <!-- No Connection State -->
    <div v-else class="text-center q-mt-xl">
      <q-icon name="bluetooth_disabled" size="64px" color="grey-7" class="q-mb-md" />
      <div class="text-h6">No Arena Connected</div>
      <div class="text-body2 text-grey q-mt-md">
        Connect to an arena to view real-time monitoring data
      </div>
    </div>
  </q-page>
</template>

<script setup lang="ts">
import { computed, onMounted } from 'vue';
import { useMonitorModeStore } from 'src/stores/monitor-mode';
import { bleArenaMonitor } from 'src/services/ble-arena-monitor';

const store = useMonitorModeStore();

const isConnected = computed(() => store.connectionStatus === 'connected');
const connectionStatus = computed(() => store.connectionStatus);
const connectedArenaName = computed(() => store.connectedArenaName);
const remoteArenaState = computed(() => store.remoteArenaState);
const lastUpdate = computed(() => store.lastUpdate);

const connectionStatusIcon = computed(() => {
  switch (connectionStatus.value) {
    case 'scanning':
      return 'bluetooth_searching';
    case 'connecting':
      return 'bluetooth';
    default:
      return 'bluetooth_disabled';
  }
});

const connectionStatusText = computed(() => {
  switch (connectionStatus.value) {
    case 'scanning':
      return 'Scanning for arenas...';
    case 'connecting':
      return 'Connecting to arena...';
    default:
      return 'Not connected to arena';
  }
});

const connectionStatusDetail = computed(() => {
  switch (connectionStatus.value) {
    case 'scanning':
      return 'Looking for Shadow Warrior Arena devices';
    case 'connecting':
      return 'Establishing BLE connection...';
    default:
      return '';
  }
});

const lastUpdateTime = computed(() => {
  if (!lastUpdate.value) return 'Never';
  const elapsed = Date.now() - lastUpdate.value;
  if (elapsed < 1000) return 'Just now';
  if (elapsed < 60000) return `${Math.floor(elapsed / 1000)}s ago`;
  return `${Math.floor(elapsed / 60000)}m ago`;
});

async function scanAndConnect(): Promise<void> {
  try {
    store.enableMonitorMode();
    await bleArenaMonitor.scanAndConnect();
  } catch (error) {
    console.error('Failed to scan and connect:', error);
    const errorMsg = error instanceof Error ? error.message : String(error);
    console.error('Detailed error:', errorMsg);
    store.setConnectionStatus('disconnected');
  }
}

async function handleConnectionAction(): Promise<void> {
  if (connectionStatus.value === 'disconnected') {
    await scanAndConnect();
  } else {
    // Cancel ongoing connection attempt
    await bleArenaMonitor.disconnect();
    store.disableMonitorMode();
  }
}

async function disconnect(): Promise<void> {
  await bleArenaMonitor.disconnect();
  store.disableMonitorMode();
}

function formatTime(ms: number): string {
  const seconds = Math.floor(ms / 1000);
  const minutes = Math.floor(seconds / 60);
  const secs = seconds % 60;
  return `${minutes}:${secs.toString().padStart(2, '0')}`;
}

onMounted(() => {
  // Check if already connected and restore UI state
  const status = bleArenaMonitor.getStatus();
  if (status.isConnected) {
    store.enableMonitorMode();
    store.setConnectedArena('', status.deviceName || 'Unknown Arena');
  }
});
</script>

<template>
  <q-layout view="lHh Lpr lFf">
    <q-header elevated class="bg-primary text-white" style="padding-top: env(safe-area-inset-top); padding-left: env(safe-area-inset-left); padding-right: env(safe-area-inset-right);">
      <q-toolbar>
        <q-btn flat dense round icon="menu" aria-label="Menu" @click="toggleLeftDrawer" />

        <q-toolbar-title>
          Shadow Warrior Brain
          <div class="text-caption">ver {{ version }}-{{ buildNumber }}</div>
        </q-toolbar-title>

        <q-btn flat dense round icon="settings" aria-label="Settings" to="/settings" />
      </q-toolbar>
    </q-header>

    <q-drawer v-model="leftDrawerOpen" show-if-above bordered>
      <q-list>
        <q-item-label header> Navigation </q-item-label>

        <q-item clickable to="/" exact>
          <q-item-section avatar>
            <q-icon name="dashboard" />
          </q-item-section>
          <q-item-section>
            <q-item-label>Dashboard</q-item-label>
          </q-item-section>
        </q-item>

        <q-item clickable to="/monitor">
          <q-item-section avatar>
            <q-icon name="sensors" />
          </q-item-section>
          <q-item-section>
            <q-item-label>Monitor Arena</q-item-label>
          </q-item-section>
          <q-item-section side v-if="isMonitorMode">
            <q-chip size="sm" color="blue" text-color="white" icon="sensors">
              {{ monitorConnectionStatus }}
            </q-chip>
          </q-item-section>
        </q-item>

        <q-item clickable to="/settings">
          <q-item-section avatar>
            <q-icon name="settings" />
          </q-item-section>
          <q-item-section>
            <q-item-label>Settings</q-item-label>
          </q-item-section>
        </q-item>
      </q-list>
    </q-drawer>

    <div class="layout-wrapper">
      <q-page-container style="padding-left: env(safe-area-inset-left); padding-right: env(safe-area-inset-right);">
        <router-view />
      </q-page-container>
    </div>

    <ConsolePanel />
  </q-layout>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue';
import { useMonitorModeStore } from 'src/stores/monitor-mode';
import ConsolePanel from 'src/components/ConsolePanel.vue';
import packageJson from '../../package.json';

const leftDrawerOpen = ref(false);
const version = packageJson.version;
const buildNumber = packageJson.buildNumber || 0;

const monitorStore = useMonitorModeStore();
const isMonitorMode = computed(() => monitorStore.isMonitorMode);
const monitorConnectionStatus = computed(() => {
  const status = monitorStore.connectionStatus;
  if (status === 'connected') return 'Connected';
  if (status === 'connecting') return 'Connecting...';
  if (status === 'scanning') return 'Scanning...';
  return 'Disconnected';
});

function toggleLeftDrawer() {
  leftDrawerOpen.value = !leftDrawerOpen.value;
}
</script>

<style scoped>
.layout-wrapper {
  display: flex;
  flex-direction: column;
  height: 100%;
}

:deep(.q-page-container) {
  flex: 1;
  overflow-y: auto;
}
</style>

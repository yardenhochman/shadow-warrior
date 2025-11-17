// Boot file to initialize all Shadow Warrior services
import { boot } from 'quasar/wrappers';
import { ledControllerService } from 'src/services/led-controller';
import { speakerService } from 'src/services/speaker';
import { uvLightService } from 'src/services/uv-light';
import { scheduleService } from 'src/services/schedule';
import { arenaWebSocketServer } from 'src/services/websocket-server';
import { bleArenaPeripheral } from 'src/services/ble-arena-peripheral';

export default boot(async () => {
  console.log('Initializing Shadow Warrior services...');

  try {
    // Initialize LED controller
    await ledControllerService.initialize();
    console.log('LED controller service initialized');

    // Preload audio files
    await speakerService.preloadAll();
    console.log('Speaker service initialized');

    // Load UV light configuration from localStorage
    const savedSettings = localStorage.getItem('shadow-warrior-settings');
    if (savedSettings) {
      try {
        const settings = JSON.parse(savedSettings);
        if (settings.uvLight) {
          await uvLightService.initialize(settings.uvLight);
          console.log('UV light service initialized with saved settings');
        }
      } catch (error) {
        console.error('Failed to load saved settings:', error);
      }
    }

    // Initialize schedule service
    await scheduleService.initialize();
    console.log('Schedule service initialized');

    // Initialize WebSocket server for arena state monitoring
    try {
      const wsServerEnabled =
        localStorage.getItem('shadow-warrior-websocket-server-enabled') !== 'false';
      console.log('[WebSocket Server] Enabled flag:', wsServerEnabled);
      if (wsServerEnabled) {
        arenaWebSocketServer.initialize();
        console.log('[WebSocket Server] Starting server...');
        await arenaWebSocketServer.start();
        console.log('[WebSocket Server] Initialized and started successfully');
      } else {
        arenaWebSocketServer.initialize();
        console.log('[WebSocket Server] Initialized but not started');
      }
    } catch (wsError) {
      console.error('[WebSocket Server] Initialization error:', wsError);
    }

    // Initialize BLE Arena Peripheral for broadcasting arena state
    try {
      const blePeripheralEnabled =
        localStorage.getItem('shadow-warrior-ble-peripheral-enabled') === 'true';
      const arenaName =
        localStorage.getItem('shadow-warrior-ble-arena-name') || 'Shadow Arena';

      console.log('[BLE Peripheral] Enabled flag:', blePeripheralEnabled);
      console.log('[BLE Peripheral] Arena name:', arenaName);

      await bleArenaPeripheral.initialize(arenaName, blePeripheralEnabled);
      console.log('[BLE Peripheral] Initialized successfully');

      if (blePeripheralEnabled) {
        await bleArenaPeripheral.startAdvertising();
        console.log('[BLE Peripheral] Started advertising');
        bleArenaPeripheral.startStreaming();
        console.log('[BLE Peripheral] Started state streaming');
      }
    } catch (bleError) {
      console.error('[BLE Peripheral] Initialization error:', bleError);
    }

    console.log('All services initialized successfully');
  } catch (error) {
    console.error('Error initializing services:', error);
    // Don't throw - let the app continue even if some services fail to initialize
  }
});

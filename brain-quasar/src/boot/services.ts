// Boot file to initialize all Shadow Warrior services
import { boot } from 'quasar/wrappers';
import { ledControllerService } from 'src/services/led-controller';
import { speakerService } from 'src/services/speaker';
import { uvLightService } from 'src/services/uv-light';
import { scheduleService } from 'src/services/schedule';
import { arenaWebSocketServer } from 'src/services/websocket-server';
import { bleArenaPeripheral } from 'src/services/ble-arena-peripheral';

// Helper function to add timeout to promises
const withTimeout = <T>(promise: Promise<T>, timeoutMs: number, taskName: string): Promise<T> => {
  return Promise.race([
    promise,
    new Promise<T>((_, reject) =>
      setTimeout(() => reject(new Error(`${taskName} timed out after ${timeoutMs}ms`)), timeoutMs)
    ),
  ]);
};

export default boot(async () => {
  console.log('Initializing Shadow Warrior services...');

  // Note: Foreground service is started automatically in MainActivity.onCreate()

  try {
    console.log('[Boot] Starting parallel service initialization...');

    // Initialize all services in parallel with timeouts
    const initResults = await Promise.allSettled([
      // LED controller with 3 second timeout (network discovery can be slow)
      withTimeout(
        (async () => {
          console.log('[Boot] Initializing LED controller...');
          await ledControllerService.initialize();
          console.log('LED controller service initialized');
        })(),
        3000,
        'LED controller'
      ),

      // Speaker service (should be fast)
      (async () => {
        console.log('[Boot] Preloading audio files...');
        await speakerService.preloadAll();
        console.log('Speaker service initialized');
      })(),

      // UV light service
      (async () => {
        const savedSettings = localStorage.getItem('shadow-warrior-settings');
        if (savedSettings) {
          const settings = JSON.parse(savedSettings);
          if (settings.uvLight) {
            await uvLightService.initialize(settings.uvLight);
            console.log('UV light service initialized with saved settings');
          }
        }
      })(),

      // Schedule service
      (async () => {
        console.log('[Boot] Initializing schedule service...');
        await scheduleService.initialize();
        console.log('Schedule service initialized');
      })(),

      // WebSocket server with 2 second timeout
      withTimeout(
        (async () => {
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
        })(),
        2000,
        'WebSocket server'
      ),

      // BLE Arena Peripheral (critical - no timeout)
      (async () => {
        const storedValue = localStorage.getItem('shadow-warrior-ble-peripheral-enabled');
        const blePeripheralEnabled = storedValue === null || storedValue === 'true';
        const arenaName =
          localStorage.getItem('shadow-warrior-ble-arena-name') || 'Shadow Warrior Arena';

        console.log('[BLE Peripheral] Enabled flag:', blePeripheralEnabled);
        console.log('[BLE Peripheral] Arena name:', arenaName);

        await bleArenaPeripheral.initialize(arenaName, blePeripheralEnabled);
        console.log('[BLE Peripheral] Initialized successfully');
      })(),
    ]);

    // Log results
    initResults.forEach((result, index) => {
      const serviceName = [
        'LED controller',
        'Speaker',
        'UV light',
        'Schedule',
        'WebSocket server',
        'BLE Peripheral',
      ][index];

      if (result.status === 'rejected') {
        console.error(`[Boot] ${serviceName} failed:`, result.reason);
      }
    });

    console.log('[Boot] All services initialization completed');
  } catch (error) {
    console.error('Error initializing services:', error);
    // Don't throw - let the app continue even if some services fail to initialize
  }
});

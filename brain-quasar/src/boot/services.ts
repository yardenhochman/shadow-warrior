// Boot file to initialize all Shadow Warrior services
import { boot } from 'quasar/wrappers';
import { ledControllerService } from 'src/services/led-controller';
import { speakerService } from 'src/services/speaker';
import { uvLightService } from 'src/services/uv-light';

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

    console.log('All services initialized successfully');
  } catch (error) {
    console.error('Error initializing services:', error);
    // Don't throw - let the app continue even if some services fail to initialize
  }
});

import type { CapacitorConfig } from '@capacitor/cli';

const config: CapacitorConfig = {
  appId: 'org.shadowwarrior.brain',
  appName: 'Shadow Warrior Brain',
  webDir: 'dist/capacitor',
  server: {
    cleartext: true,
    allowNavigation: ['*'],
    androidScheme: 'http',
  },
};

export default config;

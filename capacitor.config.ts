import { CapacitorConfig } from '@capacitor/cli';

const config: CapacitorConfig = {
  appId: 'cc.snowify.app',
  appName: 'Snowify',
  webDir: 'src/renderer',
  android: {
    path: 'android',
  },
  plugins: {
    CapacitorHttp: {
      // Keep Firebase/Auth/Firestore on the normal browser networking stack.
      // Specific mobile endpoints that need native HTTP use CapacitorHttp directly.
      enabled: false,
    },
  },
};

export default config;

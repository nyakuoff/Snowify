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
      // Patch the global fetch() to route through native OkHttp.
      // This bypasses CORS for music.youtube.com and all API requests.
      enabled: true,
    },
  },
};

export default config;

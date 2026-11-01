// Prints JSON.stringify of the AxiosError ytmusic-api's constructRequest rejects with on a
// 404 (adapter stubbed), to pin the shape the log panel shows.
(async () => {
  const axios = (await import('axios')).default;
  axios.defaults.adapter = async (config) => {
    const res = { data: '{"error":{"code":404}}', status: 404, statusText: 'Not Found', headers: { 'content-type': 'application/json' }, config, request: {} };
    const { default: settle } = await import('axios/unsafe/core/settle.js');
    return new Promise((resolve, reject) => settle(resolve, reject, res));
  };
  const YTMusic = (await import('ytmusic-api')).default;
  const y = new YTMusic();
  y.config = { INNERTUBE_API_KEY: 'K', INNERTUBE_API_VERSION: 'v1', INNERTUBE_CLIENT_NAME: 'WEB_REMIX', INNERTUBE_CLIENT_VERSION: '1.0', GL: 'US', HL: 'en' };
  try { await y.constructRequest('browse', { browseId: 'x' }); } catch (e) { process.stdout.write(JSON.stringify(e, null, 1)); }
})();

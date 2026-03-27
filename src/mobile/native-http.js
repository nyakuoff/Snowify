import { CapacitorHttp } from '@capacitor/core';

function normalizeHeaders(headers) {
  return headers && typeof headers === 'object' ? headers : {};
}

function normalizeData(data, headers) {
  if (data == null) return undefined;
  const contentType = String(headers['Content-Type'] || headers['content-type'] || '').toLowerCase();
  if (typeof data === 'string') {
    if (contentType.includes('application/json')) {
      try {
        return JSON.parse(data);
      } catch {
        return data;
      }
    }
    return data;
  }
  return data;
}

async function request(url, options = {}) {
  const headers = normalizeHeaders(options.headers || {});
  return CapacitorHttp.request({
    url,
    method: options.method || 'GET',
    headers,
    data: normalizeData(options.body, headers),
    responseType: options.responseType || 'text',
    connectTimeout: options.connectTimeout || 30000,
    readTimeout: options.readTimeout || 30000,
    shouldEncodeUrlParams: options.shouldEncodeUrlParams,
  });
}

export async function nativeGetText(url, options = {}) {
  const response = await request(url, { ...options, method: 'GET', responseType: 'text' });
  return typeof response.data === 'string' ? response.data : String(response.data ?? '');
}

export async function nativeGetJson(url, options = {}) {
  const response = await request(url, { ...options, method: 'GET', responseType: 'json' });
  if (typeof response.data === 'string') {
    const text = response.data.trim();
    if (!text) throw new Error(`Empty JSON response from ${url}`);
    return JSON.parse(text);
  }
  return response.data;
}

export async function nativeRequestJson(url, options = {}) {
  const response = await request(url, { ...options, responseType: 'json' });
  if (typeof response.data === 'string') {
    const text = response.data.trim();
    if (!text) throw new Error(`Empty JSON response from ${url}`);
    return JSON.parse(text);
  }
  return response.data;
}
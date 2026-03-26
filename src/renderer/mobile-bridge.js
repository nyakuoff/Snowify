var __SnowifyMobile = (() => {
  var __defProp = Object.defineProperty;
  var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
  var __getOwnPropNames = Object.getOwnPropertyNames;
  var __hasOwnProp = Object.prototype.hasOwnProperty;
  var __esm = (fn, res) => function __init() {
    return fn && (res = (0, fn[__getOwnPropNames(fn)[0]])(fn = 0)), res;
  };
  var __export = (target, all) => {
    for (var name in all)
      __defProp(target, name, { get: all[name], enumerable: true });
  };
  var __copyProps = (to, from, except, desc) => {
    if (from && typeof from === "object" || typeof from === "function") {
      for (let key of __getOwnPropNames(from))
        if (!__hasOwnProp.call(to, key) && key !== except)
          __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
    }
    return to;
  };
  var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);

  // node_modules/@capacitor/core/dist/index.js
  var ExceptionCode, CapacitorException, getPlatformId, createCapacitor, initCapacitorGlobal, Capacitor, registerPlugin, WebPlugin, encode, decode, CapacitorCookiesPluginWeb, CapacitorCookies, readBlobAsBase64, normalizeHttpHeaders, buildUrlParams, buildRequestInit, CapacitorHttpPluginWeb, CapacitorHttp, SystemBarsStyle, SystemBarType, SystemBarsPluginWeb, SystemBars;
  var init_dist = __esm({
    "node_modules/@capacitor/core/dist/index.js"() {
      (function(ExceptionCode2) {
        ExceptionCode2["Unimplemented"] = "UNIMPLEMENTED";
        ExceptionCode2["Unavailable"] = "UNAVAILABLE";
      })(ExceptionCode || (ExceptionCode = {}));
      CapacitorException = class extends Error {
        constructor(message, code, data) {
          super(message);
          this.message = message;
          this.code = code;
          this.data = data;
        }
      };
      getPlatformId = (win) => {
        var _a, _b;
        if (win === null || win === void 0 ? void 0 : win.androidBridge) {
          return "android";
        } else if ((_b = (_a = win === null || win === void 0 ? void 0 : win.webkit) === null || _a === void 0 ? void 0 : _a.messageHandlers) === null || _b === void 0 ? void 0 : _b.bridge) {
          return "ios";
        } else {
          return "web";
        }
      };
      createCapacitor = (win) => {
        const capCustomPlatform = win.CapacitorCustomPlatform || null;
        const cap = win.Capacitor || {};
        const Plugins = cap.Plugins = cap.Plugins || {};
        const getPlatform = () => {
          return capCustomPlatform !== null ? capCustomPlatform.name : getPlatformId(win);
        };
        const isNativePlatform = () => getPlatform() !== "web";
        const isPluginAvailable = (pluginName) => {
          const plugin = registeredPlugins.get(pluginName);
          if (plugin === null || plugin === void 0 ? void 0 : plugin.platforms.has(getPlatform())) {
            return true;
          }
          if (getPluginHeader(pluginName)) {
            return true;
          }
          return false;
        };
        const getPluginHeader = (pluginName) => {
          var _a;
          return (_a = cap.PluginHeaders) === null || _a === void 0 ? void 0 : _a.find((h) => h.name === pluginName);
        };
        const handleError = (err) => win.console.error(err);
        const registeredPlugins = /* @__PURE__ */ new Map();
        const registerPlugin2 = (pluginName, jsImplementations = {}) => {
          const registeredPlugin = registeredPlugins.get(pluginName);
          if (registeredPlugin) {
            console.warn(`Capacitor plugin "${pluginName}" already registered. Cannot register plugins twice.`);
            return registeredPlugin.proxy;
          }
          const platform = getPlatform();
          const pluginHeader = getPluginHeader(pluginName);
          let jsImplementation;
          const loadPluginImplementation = async () => {
            if (!jsImplementation && platform in jsImplementations) {
              jsImplementation = typeof jsImplementations[platform] === "function" ? jsImplementation = await jsImplementations[platform]() : jsImplementation = jsImplementations[platform];
            } else if (capCustomPlatform !== null && !jsImplementation && "web" in jsImplementations) {
              jsImplementation = typeof jsImplementations["web"] === "function" ? jsImplementation = await jsImplementations["web"]() : jsImplementation = jsImplementations["web"];
            }
            return jsImplementation;
          };
          const createPluginMethod = (impl, prop) => {
            var _a, _b;
            if (pluginHeader) {
              const methodHeader = pluginHeader === null || pluginHeader === void 0 ? void 0 : pluginHeader.methods.find((m) => prop === m.name);
              if (methodHeader) {
                if (methodHeader.rtype === "promise") {
                  return (options) => cap.nativePromise(pluginName, prop.toString(), options);
                } else {
                  return (options, callback) => cap.nativeCallback(pluginName, prop.toString(), options, callback);
                }
              } else if (impl) {
                return (_a = impl[prop]) === null || _a === void 0 ? void 0 : _a.bind(impl);
              }
            } else if (impl) {
              return (_b = impl[prop]) === null || _b === void 0 ? void 0 : _b.bind(impl);
            } else {
              throw new CapacitorException(`"${pluginName}" plugin is not implemented on ${platform}`, ExceptionCode.Unimplemented);
            }
          };
          const createPluginMethodWrapper = (prop) => {
            let remove;
            const wrapper = (...args) => {
              const p = loadPluginImplementation().then((impl) => {
                const fn = createPluginMethod(impl, prop);
                if (fn) {
                  const p2 = fn(...args);
                  remove = p2 === null || p2 === void 0 ? void 0 : p2.remove;
                  return p2;
                } else {
                  throw new CapacitorException(`"${pluginName}.${prop}()" is not implemented on ${platform}`, ExceptionCode.Unimplemented);
                }
              });
              if (prop === "addListener") {
                p.remove = async () => remove();
              }
              return p;
            };
            wrapper.toString = () => `${prop.toString()}() { [capacitor code] }`;
            Object.defineProperty(wrapper, "name", {
              value: prop,
              writable: false,
              configurable: false
            });
            return wrapper;
          };
          const addListener = createPluginMethodWrapper("addListener");
          const removeListener = createPluginMethodWrapper("removeListener");
          const addListenerNative = (eventName, callback) => {
            const call = addListener({ eventName }, callback);
            const remove = async () => {
              const callbackId = await call;
              removeListener({
                eventName,
                callbackId
              }, callback);
            };
            const p = new Promise((resolve2) => call.then(() => resolve2({ remove })));
            p.remove = async () => {
              console.warn(`Using addListener() without 'await' is deprecated.`);
              await remove();
            };
            return p;
          };
          const proxy = new Proxy({}, {
            get(_, prop) {
              switch (prop) {
                // https://github.com/facebook/react/issues/20030
                case "$$typeof":
                  return void 0;
                case "toJSON":
                  return () => ({});
                case "addListener":
                  return pluginHeader ? addListenerNative : addListener;
                case "removeListener":
                  return removeListener;
                default:
                  return createPluginMethodWrapper(prop);
              }
            }
          });
          Plugins[pluginName] = proxy;
          registeredPlugins.set(pluginName, {
            name: pluginName,
            proxy,
            platforms: /* @__PURE__ */ new Set([...Object.keys(jsImplementations), ...pluginHeader ? [platform] : []])
          });
          return proxy;
        };
        if (!cap.convertFileSrc) {
          cap.convertFileSrc = (filePath) => filePath;
        }
        cap.getPlatform = getPlatform;
        cap.handleError = handleError;
        cap.isNativePlatform = isNativePlatform;
        cap.isPluginAvailable = isPluginAvailable;
        cap.registerPlugin = registerPlugin2;
        cap.Exception = CapacitorException;
        cap.DEBUG = !!cap.DEBUG;
        cap.isLoggingEnabled = !!cap.isLoggingEnabled;
        return cap;
      };
      initCapacitorGlobal = (win) => win.Capacitor = createCapacitor(win);
      Capacitor = /* @__PURE__ */ initCapacitorGlobal(typeof globalThis !== "undefined" ? globalThis : typeof self !== "undefined" ? self : typeof window !== "undefined" ? window : typeof global !== "undefined" ? global : {});
      registerPlugin = Capacitor.registerPlugin;
      WebPlugin = class {
        constructor() {
          this.listeners = {};
          this.retainedEventArguments = {};
          this.windowListeners = {};
        }
        addListener(eventName, listenerFunc) {
          let firstListener = false;
          const listeners = this.listeners[eventName];
          if (!listeners) {
            this.listeners[eventName] = [];
            firstListener = true;
          }
          this.listeners[eventName].push(listenerFunc);
          const windowListener = this.windowListeners[eventName];
          if (windowListener && !windowListener.registered) {
            this.addWindowListener(windowListener);
          }
          if (firstListener) {
            this.sendRetainedArgumentsForEvent(eventName);
          }
          const remove = async () => this.removeListener(eventName, listenerFunc);
          const p = Promise.resolve({ remove });
          return p;
        }
        async removeAllListeners() {
          this.listeners = {};
          for (const listener in this.windowListeners) {
            this.removeWindowListener(this.windowListeners[listener]);
          }
          this.windowListeners = {};
        }
        notifyListeners(eventName, data, retainUntilConsumed) {
          const listeners = this.listeners[eventName];
          if (!listeners) {
            if (retainUntilConsumed) {
              let args = this.retainedEventArguments[eventName];
              if (!args) {
                args = [];
              }
              args.push(data);
              this.retainedEventArguments[eventName] = args;
            }
            return;
          }
          listeners.forEach((listener) => listener(data));
        }
        hasListeners(eventName) {
          var _a;
          return !!((_a = this.listeners[eventName]) === null || _a === void 0 ? void 0 : _a.length);
        }
        registerWindowListener(windowEventName, pluginEventName) {
          this.windowListeners[pluginEventName] = {
            registered: false,
            windowEventName,
            pluginEventName,
            handler: (event) => {
              this.notifyListeners(pluginEventName, event);
            }
          };
        }
        unimplemented(msg = "not implemented") {
          return new Capacitor.Exception(msg, ExceptionCode.Unimplemented);
        }
        unavailable(msg = "not available") {
          return new Capacitor.Exception(msg, ExceptionCode.Unavailable);
        }
        async removeListener(eventName, listenerFunc) {
          const listeners = this.listeners[eventName];
          if (!listeners) {
            return;
          }
          const index = listeners.indexOf(listenerFunc);
          this.listeners[eventName].splice(index, 1);
          if (!this.listeners[eventName].length) {
            this.removeWindowListener(this.windowListeners[eventName]);
          }
        }
        addWindowListener(handle) {
          window.addEventListener(handle.windowEventName, handle.handler);
          handle.registered = true;
        }
        removeWindowListener(handle) {
          if (!handle) {
            return;
          }
          window.removeEventListener(handle.windowEventName, handle.handler);
          handle.registered = false;
        }
        sendRetainedArgumentsForEvent(eventName) {
          const args = this.retainedEventArguments[eventName];
          if (!args) {
            return;
          }
          delete this.retainedEventArguments[eventName];
          args.forEach((arg) => {
            this.notifyListeners(eventName, arg);
          });
        }
      };
      encode = (str) => encodeURIComponent(str).replace(/%(2[346B]|5E|60|7C)/g, decodeURIComponent).replace(/[()]/g, escape);
      decode = (str) => str.replace(/(%[\dA-F]{2})+/gi, decodeURIComponent);
      CapacitorCookiesPluginWeb = class extends WebPlugin {
        async getCookies() {
          const cookies = document.cookie;
          const cookieMap = {};
          cookies.split(";").forEach((cookie) => {
            if (cookie.length <= 0)
              return;
            let [key, value] = cookie.replace(/=/, "CAP_COOKIE").split("CAP_COOKIE");
            key = decode(key).trim();
            value = decode(value).trim();
            cookieMap[key] = value;
          });
          return cookieMap;
        }
        async setCookie(options) {
          try {
            const encodedKey = encode(options.key);
            const encodedValue = encode(options.value);
            const expires = options.expires ? `; expires=${options.expires.replace("expires=", "")}` : "";
            const path = (options.path || "/").replace("path=", "");
            const domain = options.url != null && options.url.length > 0 ? `domain=${options.url}` : "";
            document.cookie = `${encodedKey}=${encodedValue || ""}${expires}; path=${path}; ${domain};`;
          } catch (error) {
            return Promise.reject(error);
          }
        }
        async deleteCookie(options) {
          try {
            document.cookie = `${options.key}=; Max-Age=0`;
          } catch (error) {
            return Promise.reject(error);
          }
        }
        async clearCookies() {
          try {
            const cookies = document.cookie.split(";") || [];
            for (const cookie of cookies) {
              document.cookie = cookie.replace(/^ +/, "").replace(/=.*/, `=;expires=${(/* @__PURE__ */ new Date()).toUTCString()};path=/`);
            }
          } catch (error) {
            return Promise.reject(error);
          }
        }
        async clearAllCookies() {
          try {
            await this.clearCookies();
          } catch (error) {
            return Promise.reject(error);
          }
        }
      };
      CapacitorCookies = registerPlugin("CapacitorCookies", {
        web: () => new CapacitorCookiesPluginWeb()
      });
      readBlobAsBase64 = async (blob) => new Promise((resolve2, reject) => {
        const reader = new FileReader();
        reader.onload = () => {
          const base64String = reader.result;
          resolve2(base64String.indexOf(",") >= 0 ? base64String.split(",")[1] : base64String);
        };
        reader.onerror = (error) => reject(error);
        reader.readAsDataURL(blob);
      });
      normalizeHttpHeaders = (headers = {}) => {
        const originalKeys = Object.keys(headers);
        const loweredKeys = Object.keys(headers).map((k) => k.toLocaleLowerCase());
        const normalized = loweredKeys.reduce((acc, key, index) => {
          acc[key] = headers[originalKeys[index]];
          return acc;
        }, {});
        return normalized;
      };
      buildUrlParams = (params, shouldEncode = true) => {
        if (!params)
          return null;
        const output = Object.entries(params).reduce((accumulator, entry) => {
          const [key, value] = entry;
          let encodedValue;
          let item;
          if (Array.isArray(value)) {
            item = "";
            value.forEach((str) => {
              encodedValue = shouldEncode ? encodeURIComponent(str) : str;
              item += `${key}=${encodedValue}&`;
            });
            item.slice(0, -1);
          } else {
            encodedValue = shouldEncode ? encodeURIComponent(value) : value;
            item = `${key}=${encodedValue}`;
          }
          return `${accumulator}&${item}`;
        }, "");
        return output.substr(1);
      };
      buildRequestInit = (options, extra = {}) => {
        const output = Object.assign({ method: options.method || "GET", headers: options.headers }, extra);
        const headers = normalizeHttpHeaders(options.headers);
        const type = headers["content-type"] || "";
        if (typeof options.data === "string") {
          output.body = options.data;
        } else if (type.includes("application/x-www-form-urlencoded")) {
          const params = new URLSearchParams();
          for (const [key, value] of Object.entries(options.data || {})) {
            params.set(key, value);
          }
          output.body = params.toString();
        } else if (type.includes("multipart/form-data") || options.data instanceof FormData) {
          const form = new FormData();
          if (options.data instanceof FormData) {
            options.data.forEach((value, key) => {
              form.append(key, value);
            });
          } else {
            for (const key of Object.keys(options.data)) {
              form.append(key, options.data[key]);
            }
          }
          output.body = form;
          const headers2 = new Headers(output.headers);
          headers2.delete("content-type");
          output.headers = headers2;
        } else if (type.includes("application/json") || typeof options.data === "object") {
          output.body = JSON.stringify(options.data);
        }
        return output;
      };
      CapacitorHttpPluginWeb = class extends WebPlugin {
        /**
         * Perform an Http request given a set of options
         * @param options Options to build the HTTP request
         */
        async request(options) {
          const requestInit = buildRequestInit(options, options.webFetchExtra);
          const urlParams = buildUrlParams(options.params, options.shouldEncodeUrlParams);
          const url = urlParams ? `${options.url}?${urlParams}` : options.url;
          const response = await fetch(url, requestInit);
          const contentType = response.headers.get("content-type") || "";
          let { responseType = "text" } = response.ok ? options : {};
          if (contentType.includes("application/json")) {
            responseType = "json";
          }
          let data;
          let blob;
          switch (responseType) {
            case "arraybuffer":
            case "blob":
              blob = await response.blob();
              data = await readBlobAsBase64(blob);
              break;
            case "json":
              data = await response.json();
              break;
            case "document":
            case "text":
            default:
              data = await response.text();
          }
          const headers = {};
          response.headers.forEach((value, key) => {
            headers[key] = value;
          });
          return {
            data,
            headers,
            status: response.status,
            url: response.url
          };
        }
        /**
         * Perform an Http GET request given a set of options
         * @param options Options to build the HTTP request
         */
        async get(options) {
          return this.request(Object.assign(Object.assign({}, options), { method: "GET" }));
        }
        /**
         * Perform an Http POST request given a set of options
         * @param options Options to build the HTTP request
         */
        async post(options) {
          return this.request(Object.assign(Object.assign({}, options), { method: "POST" }));
        }
        /**
         * Perform an Http PUT request given a set of options
         * @param options Options to build the HTTP request
         */
        async put(options) {
          return this.request(Object.assign(Object.assign({}, options), { method: "PUT" }));
        }
        /**
         * Perform an Http PATCH request given a set of options
         * @param options Options to build the HTTP request
         */
        async patch(options) {
          return this.request(Object.assign(Object.assign({}, options), { method: "PATCH" }));
        }
        /**
         * Perform an Http DELETE request given a set of options
         * @param options Options to build the HTTP request
         */
        async delete(options) {
          return this.request(Object.assign(Object.assign({}, options), { method: "DELETE" }));
        }
      };
      CapacitorHttp = registerPlugin("CapacitorHttp", {
        web: () => new CapacitorHttpPluginWeb()
      });
      (function(SystemBarsStyle2) {
        SystemBarsStyle2["Dark"] = "DARK";
        SystemBarsStyle2["Light"] = "LIGHT";
        SystemBarsStyle2["Default"] = "DEFAULT";
      })(SystemBarsStyle || (SystemBarsStyle = {}));
      (function(SystemBarType2) {
        SystemBarType2["StatusBar"] = "StatusBar";
        SystemBarType2["NavigationBar"] = "NavigationBar";
      })(SystemBarType || (SystemBarType = {}));
      SystemBarsPluginWeb = class extends WebPlugin {
        async setStyle() {
          this.unavailable("not available for web");
        }
        async setAnimation() {
          this.unavailable("not available for web");
        }
        async show() {
          this.unavailable("not available for web");
        }
        async hide() {
          this.unavailable("not available for web");
        }
      };
      SystemBars = registerPlugin("SystemBars", {
        web: () => new SystemBarsPluginWeb()
      });
    }
  });

  // node_modules/@capacitor/filesystem/dist/esm/definitions.js
  var Directory, Encoding;
  var init_definitions = __esm({
    "node_modules/@capacitor/filesystem/dist/esm/definitions.js"() {
      (function(Directory2) {
        Directory2["Documents"] = "DOCUMENTS";
        Directory2["Data"] = "DATA";
        Directory2["Library"] = "LIBRARY";
        Directory2["Cache"] = "CACHE";
        Directory2["External"] = "EXTERNAL";
        Directory2["ExternalStorage"] = "EXTERNAL_STORAGE";
        Directory2["ExternalCache"] = "EXTERNAL_CACHE";
        Directory2["LibraryNoCloud"] = "LIBRARY_NO_CLOUD";
        Directory2["Temporary"] = "TEMPORARY";
      })(Directory || (Directory = {}));
      (function(Encoding2) {
        Encoding2["UTF8"] = "utf8";
        Encoding2["ASCII"] = "ascii";
        Encoding2["UTF16"] = "utf16";
      })(Encoding || (Encoding = {}));
    }
  });

  // node_modules/@capacitor/filesystem/dist/esm/web.js
  var web_exports = {};
  __export(web_exports, {
    FilesystemWeb: () => FilesystemWeb
  });
  function resolve(path) {
    const posix = path.split("/").filter((item) => item !== ".");
    const newPosix = [];
    posix.forEach((item) => {
      if (item === ".." && newPosix.length > 0 && newPosix[newPosix.length - 1] !== "..") {
        newPosix.pop();
      } else {
        newPosix.push(item);
      }
    });
    return newPosix.join("/");
  }
  function isPathParent(parent, children) {
    parent = resolve(parent);
    children = resolve(children);
    const pathsA = parent.split("/");
    const pathsB = children.split("/");
    return parent !== children && pathsA.every((value, index) => value === pathsB[index]);
  }
  var FilesystemWeb;
  var init_web = __esm({
    "node_modules/@capacitor/filesystem/dist/esm/web.js"() {
      init_dist();
      init_definitions();
      FilesystemWeb = class _FilesystemWeb extends WebPlugin {
        constructor() {
          super(...arguments);
          this.DB_VERSION = 1;
          this.DB_NAME = "Disc";
          this._writeCmds = ["add", "put", "delete"];
          this.downloadFile = async (options) => {
            var _a, _b;
            const requestInit = buildRequestInit(options, options.webFetchExtra);
            const response = await fetch(options.url, requestInit);
            let blob;
            if (!options.progress)
              blob = await response.blob();
            else if (!(response === null || response === void 0 ? void 0 : response.body))
              blob = new Blob();
            else {
              const reader = response.body.getReader();
              let bytes = 0;
              const chunks = [];
              const contentType = response.headers.get("content-type");
              const contentLength = parseInt(response.headers.get("content-length") || "0", 10);
              while (true) {
                const { done, value } = await reader.read();
                if (done)
                  break;
                chunks.push(value);
                bytes += (value === null || value === void 0 ? void 0 : value.length) || 0;
                const status = {
                  url: options.url,
                  bytes,
                  contentLength
                };
                this.notifyListeners("progress", status);
              }
              const allChunks = new Uint8Array(bytes);
              let position = 0;
              for (const chunk of chunks) {
                if (typeof chunk === "undefined")
                  continue;
                allChunks.set(chunk, position);
                position += chunk.length;
              }
              blob = new Blob([allChunks.buffer], { type: contentType || void 0 });
            }
            const result = await this.writeFile({
              path: options.path,
              directory: (_a = options.directory) !== null && _a !== void 0 ? _a : void 0,
              recursive: (_b = options.recursive) !== null && _b !== void 0 ? _b : false,
              data: blob
            });
            return { path: result.uri, blob };
          };
        }
        readFileInChunks(_options, _callback) {
          throw this.unavailable("Method not implemented.");
        }
        async initDb() {
          if (this._db !== void 0) {
            return this._db;
          }
          if (!("indexedDB" in window)) {
            throw this.unavailable("This browser doesn't support IndexedDB");
          }
          return new Promise((resolve2, reject) => {
            const request = indexedDB.open(this.DB_NAME, this.DB_VERSION);
            request.onupgradeneeded = _FilesystemWeb.doUpgrade;
            request.onsuccess = () => {
              this._db = request.result;
              resolve2(request.result);
            };
            request.onerror = () => reject(request.error);
            request.onblocked = () => {
              console.warn("db blocked");
            };
          });
        }
        static doUpgrade(event) {
          const eventTarget = event.target;
          const db = eventTarget.result;
          switch (event.oldVersion) {
            case 0:
            case 1:
            default: {
              if (db.objectStoreNames.contains("FileStorage")) {
                db.deleteObjectStore("FileStorage");
              }
              const store = db.createObjectStore("FileStorage", { keyPath: "path" });
              store.createIndex("by_folder", "folder");
            }
          }
        }
        async dbRequest(cmd, args) {
          const readFlag = this._writeCmds.indexOf(cmd) !== -1 ? "readwrite" : "readonly";
          return this.initDb().then((conn) => {
            return new Promise((resolve2, reject) => {
              const tx = conn.transaction(["FileStorage"], readFlag);
              const store = tx.objectStore("FileStorage");
              const req = store[cmd](...args);
              req.onsuccess = () => resolve2(req.result);
              req.onerror = () => reject(req.error);
            });
          });
        }
        async dbIndexRequest(indexName, cmd, args) {
          const readFlag = this._writeCmds.indexOf(cmd) !== -1 ? "readwrite" : "readonly";
          return this.initDb().then((conn) => {
            return new Promise((resolve2, reject) => {
              const tx = conn.transaction(["FileStorage"], readFlag);
              const store = tx.objectStore("FileStorage");
              const index = store.index(indexName);
              const req = index[cmd](...args);
              req.onsuccess = () => resolve2(req.result);
              req.onerror = () => reject(req.error);
            });
          });
        }
        getPath(directory, uriPath) {
          const cleanedUriPath = uriPath !== void 0 ? uriPath.replace(/^[/]+|[/]+$/g, "") : "";
          let fsPath = "";
          if (directory !== void 0)
            fsPath += "/" + directory;
          if (uriPath !== "")
            fsPath += "/" + cleanedUriPath;
          return fsPath;
        }
        async clear() {
          const conn = await this.initDb();
          const tx = conn.transaction(["FileStorage"], "readwrite");
          const store = tx.objectStore("FileStorage");
          store.clear();
        }
        /**
         * Read a file from disk
         * @param options options for the file read
         * @return a promise that resolves with the read file data result
         */
        async readFile(options) {
          const path = this.getPath(options.directory, options.path);
          const entry = await this.dbRequest("get", [path]);
          if (entry === void 0)
            throw Error("File does not exist.");
          return { data: entry.content ? entry.content : "" };
        }
        /**
         * Write a file to disk in the specified location on device
         * @param options options for the file write
         * @return a promise that resolves with the file write result
         */
        async writeFile(options) {
          const path = this.getPath(options.directory, options.path);
          let data = options.data;
          const encoding = options.encoding;
          const doRecursive = options.recursive;
          const occupiedEntry = await this.dbRequest("get", [path]);
          if (occupiedEntry && occupiedEntry.type === "directory")
            throw Error("The supplied path is a directory.");
          const parentPath = path.substr(0, path.lastIndexOf("/"));
          const parentEntry = await this.dbRequest("get", [parentPath]);
          if (parentEntry === void 0) {
            const subDirIndex = parentPath.indexOf("/", 1);
            if (subDirIndex !== -1) {
              const parentArgPath = parentPath.substr(subDirIndex);
              await this.mkdir({
                path: parentArgPath,
                directory: options.directory,
                recursive: doRecursive
              });
            }
          }
          if (!encoding && !(data instanceof Blob)) {
            data = data.indexOf(",") >= 0 ? data.split(",")[1] : data;
            if (!this.isBase64String(data))
              throw Error("The supplied data is not valid base64 content.");
          }
          const now = Date.now();
          const pathObj = {
            path,
            folder: parentPath,
            type: "file",
            size: data instanceof Blob ? data.size : data.length,
            ctime: now,
            mtime: now,
            content: data
          };
          await this.dbRequest("put", [pathObj]);
          return {
            uri: pathObj.path
          };
        }
        /**
         * Append to a file on disk in the specified location on device
         * @param options options for the file append
         * @return a promise that resolves with the file write result
         */
        async appendFile(options) {
          const path = this.getPath(options.directory, options.path);
          let data = options.data;
          const encoding = options.encoding;
          const parentPath = path.substr(0, path.lastIndexOf("/"));
          const now = Date.now();
          let ctime = now;
          const occupiedEntry = await this.dbRequest("get", [path]);
          if (occupiedEntry && occupiedEntry.type === "directory")
            throw Error("The supplied path is a directory.");
          const parentEntry = await this.dbRequest("get", [parentPath]);
          if (parentEntry === void 0) {
            const subDirIndex = parentPath.indexOf("/", 1);
            if (subDirIndex !== -1) {
              const parentArgPath = parentPath.substr(subDirIndex);
              await this.mkdir({
                path: parentArgPath,
                directory: options.directory,
                recursive: true
              });
            }
          }
          if (!encoding && !this.isBase64String(data))
            throw Error("The supplied data is not valid base64 content.");
          if (occupiedEntry !== void 0) {
            if (occupiedEntry.content instanceof Blob) {
              throw Error("The occupied entry contains a Blob object which cannot be appended to.");
            }
            if (occupiedEntry.content !== void 0 && !encoding) {
              data = btoa(atob(occupiedEntry.content) + atob(data));
            } else {
              data = occupiedEntry.content + data;
            }
            ctime = occupiedEntry.ctime;
          }
          const pathObj = {
            path,
            folder: parentPath,
            type: "file",
            size: data.length,
            ctime,
            mtime: now,
            content: data
          };
          await this.dbRequest("put", [pathObj]);
        }
        /**
         * Delete a file from disk
         * @param options options for the file delete
         * @return a promise that resolves with the deleted file data result
         */
        async deleteFile(options) {
          const path = this.getPath(options.directory, options.path);
          const entry = await this.dbRequest("get", [path]);
          if (entry === void 0)
            throw Error("File does not exist.");
          const entries = await this.dbIndexRequest("by_folder", "getAllKeys", [IDBKeyRange.only(path)]);
          if (entries.length !== 0)
            throw Error("Folder is not empty.");
          await this.dbRequest("delete", [path]);
        }
        /**
         * Create a directory.
         * @param options options for the mkdir
         * @return a promise that resolves with the mkdir result
         */
        async mkdir(options) {
          const path = this.getPath(options.directory, options.path);
          const doRecursive = options.recursive;
          const parentPath = path.substr(0, path.lastIndexOf("/"));
          const depth = (path.match(/\//g) || []).length;
          const parentEntry = await this.dbRequest("get", [parentPath]);
          const occupiedEntry = await this.dbRequest("get", [path]);
          if (depth === 1)
            throw Error("Cannot create Root directory");
          if (occupiedEntry !== void 0)
            throw Error("Current directory does already exist.");
          if (!doRecursive && depth !== 2 && parentEntry === void 0)
            throw Error("Parent directory must exist");
          if (doRecursive && depth !== 2 && parentEntry === void 0) {
            const parentArgPath = parentPath.substr(parentPath.indexOf("/", 1));
            await this.mkdir({
              path: parentArgPath,
              directory: options.directory,
              recursive: doRecursive
            });
          }
          const now = Date.now();
          const pathObj = {
            path,
            folder: parentPath,
            type: "directory",
            size: 0,
            ctime: now,
            mtime: now
          };
          await this.dbRequest("put", [pathObj]);
        }
        /**
         * Remove a directory
         * @param options the options for the directory remove
         */
        async rmdir(options) {
          const { path, directory, recursive } = options;
          const fullPath = this.getPath(directory, path);
          const entry = await this.dbRequest("get", [fullPath]);
          if (entry === void 0)
            throw Error("Folder does not exist.");
          if (entry.type !== "directory")
            throw Error("Requested path is not a directory");
          const readDirResult = await this.readdir({ path, directory });
          if (readDirResult.files.length !== 0 && !recursive)
            throw Error("Folder is not empty");
          for (const entry2 of readDirResult.files) {
            const entryPath = `${path}/${entry2.name}`;
            const entryObj = await this.stat({ path: entryPath, directory });
            if (entryObj.type === "file") {
              await this.deleteFile({ path: entryPath, directory });
            } else {
              await this.rmdir({ path: entryPath, directory, recursive });
            }
          }
          await this.dbRequest("delete", [fullPath]);
        }
        /**
         * Return a list of files from the directory (not recursive)
         * @param options the options for the readdir operation
         * @return a promise that resolves with the readdir directory listing result
         */
        async readdir(options) {
          const path = this.getPath(options.directory, options.path);
          const entry = await this.dbRequest("get", [path]);
          if (options.path !== "" && entry === void 0)
            throw Error("Folder does not exist.");
          const entries = await this.dbIndexRequest("by_folder", "getAllKeys", [IDBKeyRange.only(path)]);
          const files = await Promise.all(entries.map(async (e) => {
            let subEntry = await this.dbRequest("get", [e]);
            if (subEntry === void 0) {
              subEntry = await this.dbRequest("get", [e + "/"]);
            }
            return {
              name: e.substring(path.length + 1),
              type: subEntry.type,
              size: subEntry.size,
              ctime: subEntry.ctime,
              mtime: subEntry.mtime,
              uri: subEntry.path
            };
          }));
          return { files };
        }
        /**
         * Return full File URI for a path and directory
         * @param options the options for the stat operation
         * @return a promise that resolves with the file stat result
         */
        async getUri(options) {
          const path = this.getPath(options.directory, options.path);
          let entry = await this.dbRequest("get", [path]);
          if (entry === void 0) {
            entry = await this.dbRequest("get", [path + "/"]);
          }
          return {
            uri: (entry === null || entry === void 0 ? void 0 : entry.path) || path
          };
        }
        /**
         * Return data about a file
         * @param options the options for the stat operation
         * @return a promise that resolves with the file stat result
         */
        async stat(options) {
          const path = this.getPath(options.directory, options.path);
          let entry = await this.dbRequest("get", [path]);
          if (entry === void 0) {
            entry = await this.dbRequest("get", [path + "/"]);
          }
          if (entry === void 0)
            throw Error("Entry does not exist.");
          return {
            name: entry.path.substring(path.length + 1),
            type: entry.type,
            size: entry.size,
            ctime: entry.ctime,
            mtime: entry.mtime,
            uri: entry.path
          };
        }
        /**
         * Rename a file or directory
         * @param options the options for the rename operation
         * @return a promise that resolves with the rename result
         */
        async rename(options) {
          await this._copy(options, true);
          return;
        }
        /**
         * Copy a file or directory
         * @param options the options for the copy operation
         * @return a promise that resolves with the copy result
         */
        async copy(options) {
          return this._copy(options, false);
        }
        async requestPermissions() {
          return { publicStorage: "granted" };
        }
        async checkPermissions() {
          return { publicStorage: "granted" };
        }
        /**
         * Function that can perform a copy or a rename
         * @param options the options for the rename operation
         * @param doRename whether to perform a rename or copy operation
         * @return a promise that resolves with the result
         */
        async _copy(options, doRename = false) {
          let { toDirectory } = options;
          const { to, from, directory: fromDirectory } = options;
          if (!to || !from) {
            throw Error("Both to and from must be provided");
          }
          if (!toDirectory) {
            toDirectory = fromDirectory;
          }
          const fromPath = this.getPath(fromDirectory, from);
          const toPath = this.getPath(toDirectory, to);
          if (fromPath === toPath) {
            return {
              uri: toPath
            };
          }
          if (isPathParent(fromPath, toPath)) {
            throw Error("To path cannot contain the from path");
          }
          let toObj;
          try {
            toObj = await this.stat({
              path: to,
              directory: toDirectory
            });
          } catch (e) {
            const toPathComponents = to.split("/");
            toPathComponents.pop();
            const toPath2 = toPathComponents.join("/");
            if (toPathComponents.length > 0) {
              const toParentDirectory = await this.stat({
                path: toPath2,
                directory: toDirectory
              });
              if (toParentDirectory.type !== "directory") {
                throw new Error("Parent directory of the to path is a file");
              }
            }
          }
          if (toObj && toObj.type === "directory") {
            throw new Error("Cannot overwrite a directory with a file");
          }
          const fromObj = await this.stat({
            path: from,
            directory: fromDirectory
          });
          const updateTime = async (path, ctime2, mtime) => {
            const fullPath = this.getPath(toDirectory, path);
            const entry = await this.dbRequest("get", [fullPath]);
            entry.ctime = ctime2;
            entry.mtime = mtime;
            await this.dbRequest("put", [entry]);
          };
          const ctime = fromObj.ctime ? fromObj.ctime : Date.now();
          switch (fromObj.type) {
            // The "from" object is a file
            case "file": {
              const file = await this.readFile({
                path: from,
                directory: fromDirectory
              });
              if (doRename) {
                await this.deleteFile({
                  path: from,
                  directory: fromDirectory
                });
              }
              let encoding;
              if (!(file.data instanceof Blob) && !this.isBase64String(file.data)) {
                encoding = Encoding.UTF8;
              }
              const writeResult = await this.writeFile({
                path: to,
                directory: toDirectory,
                data: file.data,
                encoding
              });
              if (doRename) {
                await updateTime(to, ctime, fromObj.mtime);
              }
              return writeResult;
            }
            case "directory": {
              if (toObj) {
                throw Error("Cannot move a directory over an existing object");
              }
              try {
                await this.mkdir({
                  path: to,
                  directory: toDirectory,
                  recursive: false
                });
                if (doRename) {
                  await updateTime(to, ctime, fromObj.mtime);
                }
              } catch (e) {
              }
              const contents = (await this.readdir({
                path: from,
                directory: fromDirectory
              })).files;
              for (const filename of contents) {
                await this._copy({
                  from: `${from}/${filename.name}`,
                  to: `${to}/${filename.name}`,
                  directory: fromDirectory,
                  toDirectory
                }, doRename);
              }
              if (doRename) {
                await this.rmdir({
                  path: from,
                  directory: fromDirectory
                });
              }
            }
          }
          return {
            uri: toPath
          };
        }
        isBase64String(str) {
          try {
            return btoa(atob(str)) == str;
          } catch (err) {
            return false;
          }
        }
      };
      FilesystemWeb._debug = true;
    }
  });

  // src/mobile/bridge.js
  var bridge_exports = {};
  __export(bridge_exports, {
    installMobileBridge: () => installMobileBridge
  });

  // src/mobile/ytm-client.js
  var _apiKey = null;
  var _context = null;
  var _visitorData = null;
  var _initDone = false;
  var _initP = null;
  function generateCpn() {
    const chars = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_";
    return Array.from({ length: 16 }, () => chars[Math.floor(Math.random() * 64)]).join("");
  }
  var ANDROID_CONTEXT = {
    client: {
      clientName: "ANDROID_VR",
      clientVersion: "1.65.10",
      deviceMake: "Oculus",
      deviceModel: "Quest 3",
      androidSdkVersion: 32,
      osName: "Android",
      osVersion: "12L",
      hl: "en",
      gl: "US",
      userAgent: "com.google.android.apps.youtube.vr.oculus/1.65.10 (Linux; U; Android 12L; eureka-user Build/SQ3A.220605.009.A1) gzip"
    }
  };
  async function initSession() {
    if (_initDone) return;
    if (_initP) return _initP;
    _initP = (async () => {
      try {
        const resp = await fetch("https://music.youtube.com/", { cache: "no-store" });
        const html = await resp.text();
        const visitorMatch = html.match(/"VISITOR_DATA"\s*:\s*"([^"]+)"/);
        if (visitorMatch?.[1]) _visitorData = visitorMatch[1];
        const keyMatch = html.match(/"INNERTUBE_API_KEY"\s*:\s*"([^"]+)"/);
        const ctxMatch = html.match(/"INNERTUBE_CONTEXT"\s*:\s*(\{[\s\S]*?\})\s*,\s*"INNERTUBE_CONTEXT_CLIENT_NAME"/);
        if (keyMatch?.[1]) _apiKey = keyMatch[1];
        if (ctxMatch?.[1]) {
          try {
            _context = JSON.parse(ctxMatch[1]);
          } catch (_) {
          }
        }
      } catch (_) {
      }
      if (!_apiKey) _apiKey = "AIzaSyC9XL3ZjWddXya6X74dJoCTL-KNEFJOM";
      if (!_context) _context = {
        client: {
          clientName: "WEB_REMIX",
          clientVersion: "1.20241231.01.00",
          hl: "en",
          gl: "US",
          platform: "DESKTOP",
          userAgent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36"
        }
      };
      _initDone = true;
      _initP = null;
    })();
    return _initP;
  }
  async function musicRequest(endpoint, body) {
    await initSession();
    const resp = await fetch(
      `https://music.youtube.com/youtubei/v1/${endpoint}?key=${_apiKey}&prettyPrint=false`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ context: _context, ...body })
      }
    );
    return resp.json();
  }
  function getBestThumbnail(thumbnails) {
    if (!thumbnails?.length) return "";
    return [...thumbnails].sort((a, b) => (b.width || 0) - (a.width || 0))[0]?.url || "";
  }
  function getSquareThumbnail(thumbnails, size = 226) {
    const url = getBestThumbnail(thumbnails);
    if (!url) return "";
    if (url.includes("lh3.googleusercontent.com")) {
      return url.replace(/=(?:w\d+-h\d+|s\d+|p-w\d+).*$/, `=w${size}-h${size}-l90-rj`);
    }
    return url;
  }
  function parseArtistsFromRuns(runs) {
    if (!runs?.length) return [];
    const artistRuns = runs.filter((r) => {
      const pageType = r.navigationEndpoint?.browseEndpoint?.browseEndpointContextSupportedConfigs?.browseEndpointContextMusicConfig?.pageType;
      return pageType === "MUSIC_PAGE_TYPE_ARTIST";
    });
    if (artistRuns.length > 0) {
      return artistRuns.map((r) => ({
        name: r.text,
        id: r.navigationEndpoint.browseEndpoint.browseId
      }));
    }
    if (runs.length >= 1 && !runs[0].navigationEndpoint) {
      const text = runs.map((r) => r.text).join("");
      const dotIdx = text.indexOf(" \u2022 ");
      const artistText = dotIdx >= 0 ? text.slice(0, dotIdx) : text;
      return artistText.split(/,\s*|\s*&\s*/).filter(Boolean).map((name) => ({ name: name.trim(), id: null }));
    }
    return [];
  }
  function buildArtistFields(artists) {
    if (!artists?.length) return { artist: "Unknown Artist", artistId: null, artists: [] };
    return {
      artist: artists.map((a) => a.name).join(", "),
      artistId: artists[0].id || null,
      artists
    };
  }
  function mapSongFromShelf(r) {
    const cols = r.flexColumns || [];
    const videoId = cols[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs?.[0]?.navigationEndpoint?.watchEndpoint?.videoId;
    if (!videoId) return null;
    const titleRuns = cols[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || [];
    const allRuns = cols[1]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || [];
    const dotIdx = allRuns.findIndex((run) => run.text === " \u2022 ");
    const artistRuns = dotIdx >= 0 ? allRuns.slice(0, dotIdx) : allRuns;
    const artists = parseArtistsFromRuns(artistRuns);
    let album = null, albumId = null;
    for (let i = 2; i < cols.length; i++) {
      const runs = cols[i]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || [];
      const albumRun = runs.find(
        (run) => run.navigationEndpoint?.browseEndpoint?.browseEndpointContextSupportedConfigs?.browseEndpointContextMusicConfig?.pageType === "MUSIC_PAGE_TYPE_ALBUM"
      );
      if (albumRun) {
        album = albumRun.text;
        albumId = albumRun.navigationEndpoint.browseEndpoint.browseId;
        break;
      }
    }
    const durationText = r.fixedColumns?.[0]?.musicResponsiveListItemFixedColumnRenderer?.text?.runs?.[0]?.text || "";
    const thumbs = r.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails || [];
    return {
      id: videoId,
      title: titleRuns.map((r2) => r2.text).join("") || "Unknown",
      ...buildArtistFields(artists),
      album,
      albumId,
      thumbnail: getSquareThumbnail(thumbs),
      duration: durationText,
      durationMs: (() => {
        if (!durationText) return 0;
        const p = durationText.split(":").map(Number);
        if (p.length === 2) return (p[0] * 60 + p[1]) * 1e3;
        if (p.length === 3) return (p[0] * 3600 + p[1] * 60 + p[2]) * 1e3;
        return 0;
      })(),
      url: `https://music.youtube.com/watch?v=${videoId}`
    };
  }
  async function search(query, musicOnly = true) {
    try {
      if (musicOnly) {
        const rawData = await musicRequest("search", {
          query,
          params: "EgWKAQIIAWoOEAMQBBAJEAoQBRAREBU%3D"
        });
        const shelves = rawData?.contents?.tabbedSearchResultsRenderer?.tabs?.[0]?.tabRenderer?.content?.sectionListRenderer?.contents || [];
        const items = [];
        for (const s2 of shelves) {
          for (const entry of s2?.musicShelfRenderer?.contents || []) {
            const r = entry?.musicResponsiveListItemRenderer;
            if (r) {
              const t = mapSongFromShelf(r);
              if (t) items.push(t);
            }
          }
        }
        return items;
      } else {
        const rawData = await musicRequest("search", { query });
        const shelves = rawData?.contents?.tabbedSearchResultsRenderer?.tabs?.[0]?.tabRenderer?.content?.sectionListRenderer?.contents || [];
        const items = [];
        for (const s2 of shelves) {
          for (const entry of s2?.musicShelfRenderer?.contents || []) {
            const r = entry?.musicResponsiveListItemRenderer;
            if (r) {
              const t = mapSongFromShelf(r);
              if (t) items.push(t);
            }
          }
        }
        return items;
      }
    } catch (err) {
      console.error("[YTM] search error", err);
      return [];
    }
  }
  async function searchSuggestions(query) {
    try {
      const rawData = await musicRequest("music/get_search_suggestions", { input: query });
      const sections = rawData?.contents ?? [];
      const textSuggestions = [];
      const directResults = [];
      for (const section of sections) {
        const items = section?.searchSuggestionsSectionRenderer?.contents ?? [];
        for (const item of items) {
          if (item.searchSuggestionRenderer) {
            const text = (item.searchSuggestionRenderer.suggestion?.runs ?? []).map((r) => r.text).join("");
            if (text) textSuggestions.push(text);
            continue;
          }
          const renderer = item.musicResponsiveListItemRenderer;
          if (!renderer) continue;
          const navEndpoint = renderer.navigationEndpoint;
          const thumbs = renderer.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails;
          const thumbnail = thumbs?.length ? thumbs[thumbs.length - 1].url : "";
          const cols = renderer.flexColumns ?? [];
          const titleRuns = cols[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs ?? [];
          const subtitleRuns = cols[1]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs ?? [];
          const title = titleRuns.map((r) => r.text).join("");
          const subtitle = subtitleRuns.map((r) => r.text).join("");
          if (navEndpoint?.browseEndpoint) {
            const pageType = navEndpoint.browseEndpoint?.browseEndpointContextSupportedConfigs?.browseEndpointContextMusicConfig?.pageType;
            if (pageType === "MUSIC_PAGE_TYPE_ARTIST") {
              directResults.push({ type: "artist", name: title, artistId: navEndpoint.browseEndpoint.browseId, thumbnail, subtitle });
              continue;
            }
            if (pageType === "MUSIC_PAGE_TYPE_ALBUM") {
              directResults.push({ type: "album", name: title, albumId: navEndpoint.browseEndpoint.browseId, thumbnail, subtitle });
              continue;
            }
          }
          if (navEndpoint?.watchEndpoint?.videoId) {
            const videoId = navEndpoint.watchEndpoint.videoId;
            const artists = parseArtistsFromRuns(subtitleRuns);
            directResults.push({
              type: "song",
              id: videoId,
              title,
              ...buildArtistFields(artists),
              thumbnail,
              url: `https://music.youtube.com/watch?v=${videoId}`
            });
          }
        }
      }
      return { textSuggestions, directResults };
    } catch (err) {
      console.error("[YTM] searchSuggestions error", err);
      return { textSuggestions: [], directResults: [] };
    }
  }
  async function searchArtists(query) {
    try {
      const rawData = await musicRequest("search", {
        query,
        params: "Eg-KAQwIABAAGAAgASgAMABqChAEEAMQCRAFEAo%3D"
      });
      const items = [];
      for (const s2 of rawData?.contents?.tabbedSearchResultsRenderer?.tabs?.[0]?.tabRenderer?.content?.sectionListRenderer?.contents || []) {
        for (const entry of s2?.musicShelfRenderer?.contents || []) {
          const r = entry?.musicResponsiveListItemRenderer;
          if (!r) continue;
          const cols = r.flexColumns || [];
          const runs = cols.flatMap((c) => c?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || []);
          const browseId = r.navigationEndpoint?.browseEndpoint?.browseId || "";
          const name = runs[0]?.text || "";
          const thumbnails = r.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails || [];
          if (browseId && name) items.push({
            artistId: browseId,
            name,
            thumbnail: getBestThumbnail(thumbnails)
          });
        }
      }
      return items;
    } catch (err) {
      console.error("[YTM] searchArtists error", err);
      return [];
    }
  }
  async function searchAlbums(query) {
    try {
      const rawData = await musicRequest("search", {
        query,
        params: "EgWKAQIYAWoOEAMQBBAJEAoQBRAREBU%3D"
      });
      const items = [];
      for (const s2 of rawData?.contents?.tabbedSearchResultsRenderer?.tabs?.[0]?.tabRenderer?.content?.sectionListRenderer?.contents || []) {
        for (const entry of s2?.musicShelfRenderer?.contents || []) {
          const r = entry?.musicResponsiveListItemRenderer;
          if (!r) continue;
          const cols = r.flexColumns || [];
          const titleRuns = cols[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || [];
          const subtitleRuns = cols[1]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || [];
          const browseId = titleRuns[0]?.navigationEndpoint?.browseEndpoint?.browseId || "";
          if (!browseId) continue;
          const artists = parseArtistsFromRuns(subtitleRuns);
          const artistData = buildArtistFields(artists);
          const thumbs = r.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails || [];
          items.push({
            albumId: browseId,
            name: titleRuns.map((r2) => r2.text).join("") || "Unknown",
            artist: artistData.artist,
            artistId: artistData.artistId,
            thumbnail: getSquareThumbnail(thumbs)
          });
        }
      }
      return items;
    } catch (err) {
      console.error("[YTM] searchAlbums error", err);
      return [];
    }
  }
  async function searchVideos(query) {
    try {
      const rawData = await musicRequest("search", {
        query,
        params: "EgWKAQIQAWoOEAMQBBAJEAoQBRAREBU%3D"
      });
      const items = [];
      for (const s2 of rawData?.contents?.tabbedSearchResultsRenderer?.tabs?.[0]?.tabRenderer?.content?.sectionListRenderer?.contents || []) {
        for (const entry of s2?.musicShelfRenderer?.contents || []) {
          const r = entry?.musicResponsiveListItemRenderer;
          if (!r) continue;
          const cols = r.flexColumns || [];
          const videoId = cols[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs?.[0]?.navigationEndpoint?.watchEndpoint?.videoId;
          if (!videoId) continue;
          const title = cols[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs?.[0]?.text || "Unknown";
          const allRuns = cols[1]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || [];
          const artists = parseArtistsFromRuns(allRuns);
          const durationText = r.fixedColumns?.[0]?.musicResponsiveListItemFixedColumnRenderer?.text?.runs?.[0]?.text || "";
          const thumbs = r.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails || [];
          items.push({
            id: videoId,
            title,
            ...buildArtistFields(artists),
            thumbnail: getBestThumbnail(thumbs),
            duration: durationText,
            durationMs: 0,
            url: `https://music.youtube.com/watch?v=${videoId}`
          });
        }
      }
      return items;
    } catch (err) {
      console.error("[YTM] searchVideos error", err);
      return [];
    }
  }
  async function searchPlaylists(query) {
    try {
      const rawData = await musicRequest("search", {
        query,
        params: "EgWKAQIoAWoOEAMQBBAJEAoQBRAREBU%3D"
      });
      const items = [];
      for (const s2 of rawData?.contents?.tabbedSearchResultsRenderer?.tabs?.[0]?.tabRenderer?.content?.sectionListRenderer?.contents || []) {
        for (const entry of s2?.musicShelfRenderer?.contents || []) {
          const r = entry?.musicResponsiveListItemRenderer;
          if (!r) continue;
          const cols = r.flexColumns || [];
          const titleRuns = cols[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || [];
          const browseId = titleRuns[0]?.navigationEndpoint?.browseEndpoint?.browseId || "";
          if (!browseId) continue;
          const subtitleRuns = cols[1]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || [];
          const thumbs = r.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails || [];
          items.push({
            playlistId: browseId,
            name: titleRuns.map((r2) => r2.text).join("") || "Unknown",
            author: subtitleRuns.map((r2) => r2.text).join("").replace(/^.*\u2022\s*/, "").trim(),
            thumbnail: getSquareThumbnail(thumbs, 300)
          });
        }
      }
      return items;
    } catch (err) {
      console.error("[YTM] searchPlaylists error", err);
      return [];
    }
  }
  async function getPlaylistVideos(playlistId) {
    try {
      const rawData = await musicRequest("browse", { browseId: playlistId });
      const shelf = rawData?.contents?.twoColumnBrowseResultsRenderer?.secondaryContents?.sectionListRenderer?.contents?.[0]?.musicPlaylistShelfRenderer || rawData?.header?.musicImmersiveHeaderRenderer || null;
      const shelfContents = rawData?.contents?.twoColumnBrowseResultsRenderer?.secondaryContents?.sectionListRenderer?.contents?.[0]?.musicPlaylistShelfRenderer?.contents || rawData?.contents?.singleColumnBrowseResultsRenderer?.tabs?.[0]?.tabRenderer?.content?.sectionListRenderer?.contents?.[0]?.musicShelfRenderer?.contents || [];
      return shelfContents.map((item) => {
        const r = item?.musicResponsiveListItemRenderer;
        if (!r) return null;
        return mapSongFromShelf(r);
      }).filter(Boolean);
    } catch (err) {
      console.error("[YTM] getPlaylistVideos error", err);
      return [];
    }
  }
  async function artistInfo(artistId) {
    try {
      const rawData = await musicRequest("browse", { browseId: artistId });
      const header = rawData?.header?.musicImmersiveHeaderRenderer || rawData?.header?.musicVisualHeaderRenderer;
      const name = header?.title?.runs?.[0]?.text || "Unknown";
      const monthlyListeners = header?.monthlyListenerCount?.runs?.[0]?.text || "";
      const bannerThumbs = header?.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails || [];
      const bannerUrl = getBestThumbnail(bannerThumbs);
      const banner = bannerUrl?.includes("lh3.googleusercontent.com") ? bannerUrl.replace(/=(?:w\d+-h\d+|s\d+|p-w\d+).*$/, "=w1440-h600-p-l90-rj") : bannerUrl;
      const thumbnails = header?.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails || header?.foregroundThumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails || [];
      const avatar = getSquareThumbnail(thumbnails, 512);
      const sections = rawData?.contents?.singleColumnBrowseResultsRenderer?.tabs?.[0]?.tabRenderer?.content?.sectionListRenderer?.contents || [];
      const topSongs = [];
      const topAlbums = [];
      const topSingles = [];
      const topVideos = [];
      const fansAlsoLike = [];
      const livePerformances = [];
      const featuredOn = [];
      const rawTopSongsArtists = {};
      for (const section of sections) {
        const shelf = section?.musicShelfRenderer;
        if (shelf) {
          for (const item of shelf.contents || []) {
            const r = item?.musicResponsiveListItemRenderer;
            if (!r) continue;
            const cols = r.flexColumns || [];
            const videoId = cols[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs?.[0]?.navigationEndpoint?.watchEndpoint?.videoId;
            if (!videoId) continue;
            const artists = parseArtistsFromRuns(cols[1]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || []);
            if (artists.length) rawTopSongsArtists[videoId] = artists;
            const track = mapSongFromShelf(r);
            if (track) topSongs.push(track);
          }
        }
        const carousel = section?.musicCarouselShelfRenderer;
        if (!carousel) continue;
        const title = (carousel?.header?.musicCarouselShelfBasicHeaderRenderer?.title?.runs?.[0]?.text || "").toLowerCase();
        if (title.includes("fans") && title.includes("like")) {
          for (const item of carousel.contents || []) {
            const r = item?.musicTwoRowItemRenderer;
            const browseId = r?.navigationEndpoint?.browseEndpoint?.browseId || "";
            if (!r || !browseId.startsWith("UC")) continue;
            fansAlsoLike.push({
              artistId: browseId,
              name: r?.title?.runs?.[0]?.text || "Unknown",
              thumbnail: getSquareThumbnail(r?.thumbnailRenderer?.musicThumbnailRenderer?.thumbnail?.thumbnails || [], 226)
            });
          }
        } else if (title.includes("album")) {
          for (const item of carousel.contents || []) {
            const r = item?.musicTwoRowItemRenderer;
            const albumId = r?.navigationEndpoint?.browseEndpoint?.browseId || "";
            if (!r || !albumId) continue;
            const thumbs = r?.thumbnailRenderer?.musicThumbnailRenderer?.thumbnail?.thumbnails || [];
            topAlbums.push({
              albumId,
              name: r?.title?.runs?.[0]?.text || "Unknown",
              year: parseInt(r?.subtitle?.runs?.find((s2) => /\d{4}/.test(s2.text))?.text) || null,
              type: "Album",
              thumbnail: getSquareThumbnail(thumbs, 300)
            });
          }
        } else if (title.includes("single") || title.includes("ep")) {
          for (const item of carousel.contents || []) {
            const r = item?.musicTwoRowItemRenderer;
            const albumId = r?.navigationEndpoint?.browseEndpoint?.browseId || "";
            if (!r || !albumId) continue;
            const thumbs = r?.thumbnailRenderer?.musicThumbnailRenderer?.thumbnail?.thumbnails || [];
            topSingles.push({
              albumId,
              name: r?.title?.runs?.[0]?.text || "Unknown",
              year: parseInt(r?.subtitle?.runs?.find((s2) => /\d{4}/.test(s2.text))?.text) || null,
              type: "Single",
              thumbnail: getSquareThumbnail(thumbs, 300)
            });
          }
        } else if (title.includes("video") || title.includes("live")) {
          for (const item of carousel.contents || []) {
            const r = item?.musicTwoRowItemRenderer;
            const videoId = r?.navigationEndpoint?.watchEndpoint?.videoId;
            if (!r || !videoId) continue;
            const subtitleRuns = r?.subtitle?.runs || [];
            const artists = parseArtistsFromRuns(subtitleRuns);
            const thumbs = r?.thumbnailRenderer?.musicThumbnailRenderer?.thumbnail?.thumbnails || [];
            topVideos.push({
              videoId,
              name: r?.title?.runs?.[0]?.text || "Untitled",
              ...buildArtistFields(artists),
              thumbnail: getBestThumbnail(thumbs),
              duration: ""
            });
          }
        } else if (title.includes("featured")) {
          for (const item of carousel.contents || []) {
            const r = item?.musicTwoRowItemRenderer;
            const playlistId = r?.navigationEndpoint?.browseEndpoint?.browseId || "";
            if (!r || !playlistId) continue;
            const thumbs = r?.thumbnailRenderer?.musicThumbnailRenderer?.thumbnail?.thumbnails || [];
            featuredOn.push({
              playlistId,
              name: r?.title?.runs?.[0]?.text || "Unknown Playlist",
              thumbnail: getSquareThumbnail(thumbs, 300)
            });
          }
        }
      }
      for (const song of topSongs) {
        if (rawTopSongsArtists[song.id]) {
          const af = buildArtistFields(rawTopSongsArtists[song.id]);
          Object.assign(song, af);
        }
      }
      return {
        name,
        artistId,
        monthlyListeners,
        banner,
        avatar,
        description: "",
        followers: 0,
        tags: [],
        topSongs: topSongs.slice(0, 10),
        topAlbums,
        topSingles,
        topVideos,
        fansAlsoLike,
        livePerformances,
        featuredOn
      };
    } catch (err) {
      console.error("[YTM] artistInfo error", err);
      return null;
    }
  }
  async function albumTracks(albumId) {
    try {
      const rawData = await musicRequest("browse", { browseId: albumId });
      const headerRuns = rawData?.contents?.twoColumnBrowseResultsRenderer?.tabs?.[0]?.tabRenderer?.content?.sectionListRenderer?.contents?.[0]?.musicResponsiveHeaderRenderer?.straplineTextOne?.runs || [];
      const albumArtists = parseArtistsFromRuns(headerRuns);
      const titleRuns = rawData?.contents?.twoColumnBrowseResultsRenderer?.tabs?.[0]?.tabRenderer?.content?.sectionListRenderer?.contents?.[0]?.musicResponsiveHeaderRenderer?.title?.runs || [];
      const albumName = titleRuns.map((r) => r.text).join("") || "Unknown Album";
      const thumbs = rawData?.header?.musicImmersiveHeaderRenderer?.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails || rawData?.contents?.twoColumnBrowseResultsRenderer?.tabs?.[0]?.tabRenderer?.content?.sectionListRenderer?.contents?.[0]?.musicResponsiveHeaderRenderer?.thumbnail?.croppedSquareThumbnailRenderer?.thumbnail?.thumbnails || [];
      const year = (() => {
        const subtitleRuns = rawData?.contents?.twoColumnBrowseResultsRenderer?.tabs?.[0]?.tabRenderer?.content?.sectionListRenderer?.contents?.[0]?.musicResponsiveHeaderRenderer?.subtitle?.runs || [];
        const yearText = subtitleRuns.find((r) => /^\d{4}$/.test(r.text?.trim()));
        return yearText ? parseInt(yearText.text) : null;
      })();
      const shelfContents = rawData?.contents?.twoColumnBrowseResultsRenderer?.secondaryContents?.sectionListRenderer?.contents?.[0]?.musicShelfRenderer?.contents || [];
      const rawArtistsMap = {};
      for (const item of shelfContents) {
        const r = item?.musicResponsiveListItemRenderer;
        if (!r) continue;
        const cols = r.flexColumns || [];
        const videoId = cols[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs?.[0]?.navigationEndpoint?.watchEndpoint?.videoId;
        if (!videoId) continue;
        const artists = parseArtistsFromRuns(cols[1]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || []);
        if (artists.length) rawArtistsMap[videoId] = artists;
      }
      const tracks = shelfContents.map((item) => {
        const r = item?.musicResponsiveListItemRenderer;
        if (!r) return null;
        const track = mapSongFromShelf(r);
        if (!track) return null;
        const artists = rawArtistsMap[track.id] || (albumArtists.length ? albumArtists : null);
        if (artists) Object.assign(track, buildArtistFields(artists));
        if (!track.album) {
          track.album = albumName;
          track.albumId = albumId;
        }
        return track;
      }).filter(Boolean);
      const albumArtistFields = albumArtists.length ? buildArtistFields(albumArtists) : { artist: "Unknown Artist", artistId: null };
      return {
        name: albumName,
        artist: albumArtistFields.artist,
        artistId: albumArtistFields.artistId,
        year,
        thumbnail: getSquareThumbnail(thumbs, 300),
        tracks
      };
    } catch (err) {
      console.error("[YTM] albumTracks error", err);
      return null;
    }
  }
  async function getUpNexts(videoId) {
    try {
      const rawData = await musicRequest("next", {
        videoId,
        playlistId: `RDAMVM${videoId}`,
        isAudioOnly: true
      });
      const contents = rawData?.contents?.singleColumnMusicWatchNextResultsRenderer?.tabbedRenderer?.watchNextTabbedResultsRenderer?.tabs?.[0]?.tabRenderer?.content?.musicQueueRenderer?.content?.playlistPanelRenderer?.contents || [];
      return contents.slice(1).map((item) => {
        const r = item?.playlistPanelVideoRenderer;
        const vid = r?.navigationEndpoint?.watchEndpoint?.videoId;
        if (!r || !vid) return null;
        const allRuns = r.longBylineText?.runs || [];
        const dotIdx = allRuns.findIndex((run) => run.text === " \u2022 ");
        const artists = parseArtistsFromRuns(dotIdx >= 0 ? allRuns.slice(0, dotIdx) : allRuns);
        const durationText = r.lengthText?.runs?.[0]?.text || "";
        const parts = durationText.split(":").map(Number);
        let durationMs = 0;
        if (parts.length === 2) durationMs = (parts[0] * 60 + parts[1]) * 1e3;
        else if (parts.length === 3) durationMs = (parts[0] * 3600 + parts[1] * 60 + parts[2]) * 1e3;
        return {
          id: vid,
          title: r.title?.runs?.[0]?.text || "Unknown",
          ...buildArtistFields(artists),
          thumbnail: getSquareThumbnail(r.thumbnail?.thumbnails || []),
          duration: durationText,
          durationMs,
          url: `https://music.youtube.com/watch?v=${vid}`
        };
      }).filter(Boolean);
    } catch (err) {
      console.error("[YTM] getUpNexts error", err);
      return [];
    }
  }
  async function explore() {
    try {
      const rawData = await musicRequest("browse", { browseId: "FEmusic_explore" });
      const sections = rawData?.contents?.singleColumnBrowseResultsRenderer?.tabs?.[0]?.tabRenderer?.content?.sectionListRenderer?.contents || [];
      const result = { newAlbums: [], moods: [], newMusicVideos: [] };
      for (const section of sections) {
        const carousel = section?.musicCarouselShelfRenderer;
        if (!carousel) continue;
        const title = (carousel?.header?.musicCarouselShelfBasicHeaderRenderer?.title?.runs?.[0]?.text || "").toLowerCase();
        if (title.includes("new album") || title.includes("new release")) {
          result.newAlbums = (carousel.contents || []).map((item) => {
            const r = item?.musicTwoRowItemRenderer;
            if (!r) return null;
            const albumId = r?.navigationEndpoint?.browseEndpoint?.browseId || "";
            if (!albumId) return null;
            const subtitleRuns = r?.subtitle?.runs || [];
            const artists = parseArtistsFromRuns(subtitleRuns);
            const artistFields = artists.length ? buildArtistFields(artists) : { artist: subtitleRuns.map((s2) => s2.text).join(""), artistId: null };
            return {
              albumId,
              name: r?.title?.runs?.[0]?.text || "Unknown",
              ...artistFields,
              thumbnail: getSquareThumbnail(r?.thumbnailRenderer?.musicThumbnailRenderer?.thumbnail?.thumbnails || [], 300),
              year: null,
              type: "Album"
            };
          }).filter(Boolean);
        } else if (title.includes("music video")) {
          result.newMusicVideos = (carousel.contents || []).map((item) => {
            const r = item?.musicTwoRowItemRenderer;
            const videoId = r?.navigationEndpoint?.watchEndpoint?.videoId || "";
            if (!r || !videoId) return null;
            const subtitleRuns = r?.subtitle?.runs || [];
            const artists = parseArtistsFromRuns(subtitleRuns);
            const artistFields = artists.length ? buildArtistFields(artists) : { artist: subtitleRuns.map((s2) => s2.text).join(""), artistId: null };
            return {
              id: videoId,
              title: r?.title?.runs?.[0]?.text || "Unknown",
              ...artistFields,
              thumbnail: getBestThumbnail(r?.thumbnailRenderer?.musicThumbnailRenderer?.thumbnail?.thumbnails || []),
              duration: "",
              durationMs: 0,
              url: `https://music.youtube.com/watch?v=${videoId}`
            };
          }).filter(Boolean);
        } else if (title.includes("mood") || title.includes("genre")) {
          result.moods = (carousel.contents || []).map((item) => {
            const r = item?.musicNavigationButtonRenderer || item?.musicTwoRowItemRenderer;
            if (!r) return null;
            const browseId = r?.clickCommand?.browseEndpoint?.browseId || r?.navigationEndpoint?.browseEndpoint?.browseId || "";
            const params = r?.clickCommand?.browseEndpoint?.params || r?.navigationEndpoint?.browseEndpoint?.params || "";
            const label = r?.buttonText?.runs?.[0]?.text || r?.title?.runs?.[0]?.text || "";
            const color = r?.solid?.leftStripeColor;
            if (!browseId || !label) return null;
            return {
              browseId,
              params,
              label,
              color: color ? `#${(color >>> 0).toString(16).padStart(8, "0").slice(2)}` : null
            };
          }).filter(Boolean);
        }
      }
      return result;
    } catch (err) {
      console.error("[YTM] explore error", err);
      return null;
    }
  }
  async function charts() {
    try {
      const rawData = await musicRequest("browse", { browseId: "FEmusic_charts" });
      let sections = rawData?.contents?.singleColumnBrowseResultsRenderer?.tabs?.[0]?.tabRenderer?.content?.sectionListRenderer?.contents || rawData?.contents?.sectionListRenderer?.contents || [];
      const result = { topSongs: [], topVideos: [], topArtists: [] };
      let trendingPlaylistId = null;
      for (const section of sections) {
        const carousel = section?.musicCarouselShelfRenderer;
        if (!carousel) continue;
        const title = (carousel?.header?.musicCarouselShelfBasicHeaderRenderer?.title?.runs?.[0]?.text || "").toLowerCase();
        if (title.includes("trending") || title.includes("video chart")) {
          for (const item of carousel.contents || []) {
            const r = item?.musicTwoRowItemRenderer;
            if (!r) continue;
            const itemTitle = (r?.title?.runs || []).map((run) => run.text).join("").toLowerCase();
            const browseId = r?.navigationEndpoint?.browseEndpoint?.browseId || "";
            if (itemTitle.includes("trending") && browseId) {
              trendingPlaylistId = browseId;
              break;
            }
          }
        } else if (title.includes("top artist") || title.includes("trending artist")) {
          result.topArtists = (carousel.contents || []).slice(0, 20).map((item) => {
            const r = item?.musicResponsiveListItemRenderer || item?.musicTwoRowItemRenderer;
            if (!r) return null;
            const artistId = r?.navigationEndpoint?.browseEndpoint?.browseId || "";
            if (!artistId || !artistId.startsWith("UC")) return null;
            const name = r?.title?.runs?.[0]?.text || r?.flexColumns?.[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs?.[0]?.text || "Unknown";
            return {
              artistId,
              name,
              thumbnail: getSquareThumbnail(
                r?.thumbnailRenderer?.musicThumbnailRenderer?.thumbnail?.thumbnails || r?.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails || [],
                226
              )
            };
          }).filter(Boolean);
        }
      }
      if (trendingPlaylistId) {
        try {
          const plRaw = await musicRequest("browse", { browseId: trendingPlaylistId });
          const shelfContents = plRaw?.contents?.twoColumnBrowseResultsRenderer?.secondaryContents?.sectionListRenderer?.contents?.[0]?.musicPlaylistShelfRenderer?.contents || [];
          result.topSongs = shelfContents.map((item) => {
            const r = item?.musicResponsiveListItemRenderer;
            if (!r) return null;
            const cols = r.flexColumns || [];
            const videoId = r?.overlay?.musicItemThumbnailOverlayRenderer?.content?.musicPlayButtonRenderer?.playNavigationEndpoint?.watchEndpoint?.videoId || cols[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs?.[0]?.navigationEndpoint?.watchEndpoint?.videoId;
            if (!videoId) return null;
            const trackName = cols[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs?.[0]?.text || "Unknown";
            const artistRuns = cols[1]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || [];
            const artists = parseArtistsFromRuns(artistRuns);
            const rank = r?.customIndexColumn?.musicCustomIndexColumnRenderer?.text?.runs?.[0]?.text || "";
            const durText = r?.fixedColumns?.[0]?.musicResponsiveListItemFixedColumnRenderer?.text?.runs?.[0]?.text || "";
            return {
              id: videoId,
              title: trackName,
              ...buildArtistFields(artists),
              thumbnail: getSquareThumbnail(r.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails || []),
              rank: parseInt(rank, 10) || 0,
              duration: durText,
              url: `https://music.youtube.com/watch?v=${videoId}`
            };
          }).filter(Boolean);
        } catch (_) {
        }
      }
      return result;
    } catch (err) {
      console.error("[YTM] charts error", err);
      return null;
    }
  }
  async function browseMood(browseId, params) {
    try {
      const rawData = await musicRequest("browse", { browseId, params });
      const grid = rawData?.contents?.singleColumnBrowseResultsRenderer?.tabs?.[0]?.tabRenderer?.content?.sectionListRenderer?.contents || [];
      const playlists = [];
      for (const section of grid) {
        for (const item of section?.gridRenderer?.items || section?.musicCarouselShelfRenderer?.contents || []) {
          const r = item?.musicTwoRowItemRenderer;
          const playlistId = r?.navigationEndpoint?.browseEndpoint?.browseId || "";
          if (!r || !playlistId) continue;
          playlists.push({
            playlistId,
            name: r?.title?.runs?.[0]?.text || "Unknown",
            subtitle: (r?.subtitle?.runs || []).map((s2) => s2.text).join(""),
            thumbnail: getSquareThumbnail(r?.thumbnailRenderer?.musicThumbnailRenderer?.thumbnail?.thumbnails || [], 300)
          });
        }
      }
      return playlists;
    } catch (err) {
      console.error("[YTM] browseMood error", err);
      return [];
    }
  }
  async function getTrackInfo(videoId) {
    try {
      const rawData = await musicRequest("next", { videoId, isAudioOnly: true });
      const contents = rawData?.contents?.singleColumnMusicWatchNextResultsRenderer?.tabbedRenderer?.watchNextTabbedResultsRenderer?.tabs?.[0]?.tabRenderer?.content?.musicQueueRenderer?.content?.playlistPanelRenderer?.contents || [];
      const r = contents[0]?.playlistPanelVideoRenderer;
      if (!r) {
        const songData = await musicRequest("next", { videoId });
        const r2 = songData?.contents?.singleColumnMusicWatchNextResultsRenderer?.tabbedRenderer?.watchNextTabbedResultsRenderer?.tabs?.[0]?.tabRenderer?.content?.musicQueueRenderer?.content?.playlistPanelRenderer?.contents?.[0]?.playlistPanelVideoRenderer;
        if (!r2) return null;
        return parseWatchRenderer(r2, videoId);
      }
      return parseWatchRenderer(r, videoId);
    } catch (err) {
      console.error("[YTM] getTrackInfo error", err);
      return null;
    }
  }
  function parseWatchRenderer(r, videoId) {
    const allRuns = r.longBylineText?.runs || [];
    const dotIdx = allRuns.findIndex((run) => run.text === " \u2022 ");
    const artists = parseArtistsFromRuns(dotIdx >= 0 ? allRuns.slice(0, dotIdx) : allRuns);
    const durationText = r.lengthText?.runs?.[0]?.text || "";
    return {
      id: videoId,
      title: r.title?.runs?.[0]?.text || "Unknown",
      ...buildArtistFields(artists),
      thumbnail: getSquareThumbnail(r.thumbnail?.thumbnails || []),
      duration: durationText,
      url: `https://music.youtube.com/watch?v=${videoId}`
    };
  }
  async function fetchPlayerData(videoId) {
    const resp = await fetch(
      "https://www.youtube.com/youtubei/v1/player?prettyPrint=false",
      {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "X-YouTube-Client-Name": "28",
          "X-YouTube-Client-Version": ANDROID_CONTEXT.client.clientVersion,
          "User-Agent": ANDROID_CONTEXT.client.userAgent,
          ..._visitorData ? { "X-Goog-Visitor-Id": _visitorData } : {}
        },
        body: JSON.stringify({
          context: {
            ...ANDROID_CONTEXT,
            client: {
              ...ANDROID_CONTEXT.client,
              ..._visitorData ? { visitorData: _visitorData } : {}
            }
          },
          videoId,
          contentCheckOk: true,
          racyCheckOk: true,
          playbackContext: {
            contentPlaybackContext: {
              html5Preference: "HTML5_PREF_WANTS"
            }
          }
        })
      }
    );
    return resp.json();
  }
  async function getStreamUrl(videoUrl, quality = "bestaudio") {
    await initSession();
    const videoId = videoUrl?.includes("watch?v=") ? new URL(videoUrl).searchParams.get("v") : videoUrl;
    if (!videoId) throw new Error("Invalid video URL");
    const cpn = generateCpn();
    const data = await fetchPlayerData(videoId);
    const status = data?.playabilityStatus?.status;
    if (status === "OK") {
      const af = data?.streamingData?.adaptiveFormats ?? [];
      let audioFormats = af.filter((f2) => f2.mimeType?.startsWith("audio/") && f2.url);
      if (!audioFormats.length) {
        console.log("[YTM] No direct audio URLs, trying Piped API\u2026");
        try {
          const piped = await fetch(`https://pipedapi.kavin.rocks/streams/${videoId}`).then((r) => r.json());
          audioFormats = (piped?.audioStreams ?? []).filter((s2) => s2.url).map((s2) => ({ mimeType: s2.mimeType ?? "audio/webm", bitrate: s2.bitrate ?? 0, url: s2.url }));
        } catch (e) {
          console.error("[YTM] Piped API failed:", e);
        }
      }
      if (audioFormats.length) {
        const sorted = quality === "worstaudio" ? [...audioFormats].sort((a, b) => (a.bitrate || 0) - (b.bitrate || 0)) : [...audioFormats].sort((a, b) => (b.bitrate || 0) - (a.bitrate || 0));
        return `${sorted[0].url}&cpn=${cpn}`;
      }
      const muxed = (data?.streamingData?.formats ?? []).filter((f2) => f2.url);
      if (muxed.length) return `${muxed[0].url}&cpn=${cpn}`;
    }
    console.error("[YTM] Player response:", JSON.stringify(data?.playabilityStatus));
    throw new Error(`No stream URLs found (status: ${data?.playabilityStatus?.status})`);
  }
  async function getVideoStreamUrl(videoId, quality = "720", premuxed = false) {
    await initSession();
    let data = await fetchPlayerData(videoId);
    const height = parseInt(quality) || 720;
    const hasVideo = (data?.streamingData?.adaptiveFormats ?? []).some((f2) => f2.url && f2.mimeType?.includes("video/"));
    if (premuxed) {
      const muxed = (data?.streamingData?.formats || []).filter((f2) => f2.url && (f2.height || 0) <= height).sort((a, b) => (b.height || 0) - (a.height || 0));
      if (muxed.length) return { videoUrl: muxed[0].url, audioUrl: null };
    }
    const adaptive = data?.streamingData?.adaptiveFormats || [];
    const videoFmts = adaptive.filter((f2) => f2.url && f2.mimeType?.includes("video/mp4") && (f2.height || 0) <= height).sort((a, b) => (b.height || 0) - (a.height || 0));
    const audioFmts = adaptive.filter((f2) => f2.url && f2.mimeType?.startsWith("audio/")).sort((a, b) => (b.bitrate || 0) - (a.bitrate || 0));
    if (!videoFmts.length || !audioFmts.length) {
      const muxed = (data?.streamingData?.formats || []).filter((f2) => f2.url);
      if (muxed.length) return { videoUrl: muxed[0].url, audioUrl: null };
      throw new Error("No suitable video/audio formats");
    }
    return {
      videoUrl: videoFmts[0].url,
      audioUrl: audioFmts[0].url
    };
  }

  // src/mobile/lyrics-client.js
  var LRCLIB = "https://lrclib.net/api";
  var _cache = /* @__PURE__ */ new Map();
  function cacheGet(k) {
    return _cache.get(k);
  }
  function cacheSet(k, v) {
    if (_cache.size >= 50) _cache.delete(_cache.keys().next().value);
    _cache.set(k, v);
  }
  async function getLyrics(trackName, artistName, albumName, durationSec) {
    const key = `${trackName}|${artistName}|${durationSec}`;
    const cached = cacheGet(key);
    if (cached !== void 0) return cached;
    try {
      const params = new URLSearchParams({
        track_name: trackName || "",
        artist_name: artistName || "",
        album_name: albumName || ""
      });
      if (durationSec && durationSec > 0) {
        params.set("duration", Math.round(durationSec).toString());
      }
      const resp = await fetch(`${LRCLIB}/get?${params.toString()}`, {
        headers: { "Lrclib-Client": "Snowify Mobile" }
      });
      if (!resp.ok) {
        cacheSet(key, null);
        return null;
      }
      const data = await resp.json();
      let synced = null;
      if (data.syncedLyrics) {
        const lines = [];
        for (const raw of data.syncedLyrics.split("\n")) {
          const m = raw.match(/^\[(\d{1,2}):(\d{2})\.(\d{1,3})\]\s*(.*)/);
          if (!m) continue;
          const timeMs = (parseInt(m[1]) * 60 + parseFloat(`${m[2]}.${m[3]}`)) * 1e3;
          lines.push({ time: timeMs, text: m[4] });
        }
        if (lines.length) synced = lines;
      }
      const plain = data.plainLyrics || null;
      if (!synced && !plain) {
        cacheSet(key, null);
        return null;
      }
      const result = { synced, plain, source: "lrclib" };
      cacheSet(key, result);
      return result;
    } catch (err) {
      console.warn("[Lyrics] lrclib fetch failed:", err.message);
      cacheSet(key, null);
      return null;
    }
  }

  // node_modules/@capacitor/filesystem/dist/esm/index.js
  init_dist();

  // node_modules/@capacitor/synapse/dist/synapse.mjs
  function s(t) {
    t.CapacitorUtils.Synapse = new Proxy(
      {},
      {
        get(e, n) {
          return new Proxy({}, {
            get(w, o) {
              return (c, p, r) => {
                const i = t.Capacitor.Plugins[n];
                if (i === void 0) {
                  r(new Error(`Capacitor plugin ${n} not found`));
                  return;
                }
                if (typeof i[o] != "function") {
                  r(new Error(`Method ${o} not found in Capacitor plugin ${n}`));
                  return;
                }
                (async () => {
                  try {
                    const a = await i[o](c);
                    p(a);
                  } catch (a) {
                    r(a);
                  }
                })();
              };
            }
          });
        }
      }
    );
  }
  function u(t) {
    t.CapacitorUtils.Synapse = new Proxy(
      {},
      {
        get(e, n) {
          return t.cordova.plugins[n];
        }
      }
    );
  }
  function f(t = false) {
    typeof window > "u" || (window.CapacitorUtils = window.CapacitorUtils || {}, window.Capacitor !== void 0 && !t ? s(window) : window.cordova !== void 0 && u(window));
  }

  // node_modules/@capacitor/filesystem/dist/esm/index.js
  init_definitions();
  var Filesystem = registerPlugin("Filesystem", {
    web: () => Promise.resolve().then(() => (init_web(), web_exports)).then((m) => new m.FilesystemWeb())
  });
  f();

  // node_modules/@capacitor/status-bar/dist/esm/index.js
  init_dist();

  // node_modules/@capacitor/status-bar/dist/esm/definitions.js
  var Style;
  (function(Style2) {
    Style2["Dark"] = "DARK";
    Style2["Light"] = "LIGHT";
    Style2["Default"] = "DEFAULT";
  })(Style || (Style = {}));
  var Animation;
  (function(Animation2) {
    Animation2["None"] = "NONE";
    Animation2["Slide"] = "SLIDE";
    Animation2["Fade"] = "FADE";
  })(Animation || (Animation = {}));

  // node_modules/@capacitor/status-bar/dist/esm/index.js
  var StatusBar = registerPlugin("StatusBar");

  // src/mobile/bridge.js
  var PROXY_PORT = 17890;
  var proxyUrl = (url) => `http://127.0.0.1:${PROXY_PORT}/stream?url=${encodeURIComponent(url)}`;
  var DATA_DIR = Directory.Data;
  async function fsRead(path) {
    try {
      const { data } = await Filesystem.readFile({ path, directory: DATA_DIR, encoding: Encoding.UTF8 });
      return data;
    } catch (_) {
      return null;
    }
  }
  async function fsWrite(path, data) {
    const parts = path.split("/");
    if (parts.length > 1) {
      try {
        await Filesystem.mkdir({ path: parts.slice(0, -1).join("/"), directory: DATA_DIR, recursive: true });
      } catch (_) {
      }
    }
    await Filesystem.writeFile({ path, directory: DATA_DIR, data, encoding: Encoding.UTF8 });
  }
  async function fsDelete(path) {
    try {
      await Filesystem.deleteFile({ path, directory: DATA_DIR });
    } catch (_) {
    }
  }
  async function fsList(path) {
    try {
      const { files } = await Filesystem.readdir({ path, directory: DATA_DIR });
      return files || [];
    } catch (_) {
      return [];
    }
  }
  async function fsReadBinary(path) {
    try {
      const { data } = await Filesystem.readFile({ path, directory: DATA_DIR });
      return data;
    } catch (_) {
      return null;
    }
  }
  async function fsWriteBinary(path, base64data) {
    const parts = path.split("/");
    if (parts.length > 1) {
      try {
        await Filesystem.mkdir({ path: parts.slice(0, -1).join("/"), directory: DATA_DIR, recursive: true });
      } catch (_) {
      }
    }
    await Filesystem.writeFile({ path, directory: DATA_DIR, data: base64data });
  }
  function pickFile(accept) {
    return new Promise((resolve2) => {
      const input = document.createElement("input");
      input.type = "file";
      input.accept = accept;
      input.addEventListener("change", async () => {
        const file = input.files?.[0];
        if (!file) return resolve2(null);
        const reader = new FileReader();
        reader.onload = (e) => resolve2({ name: file.name, data: e.target.result, size: file.size });
        reader.readAsDataURL(file);
      }, { once: true });
      input.click();
    });
  }
  function pickFiles(accept, multiple = false) {
    return new Promise((resolve2) => {
      const input = document.createElement("input");
      input.type = "file";
      input.accept = accept;
      input.multiple = multiple;
      input.addEventListener("change", async () => {
        const files = [...input.files || []];
        if (!files.length) return resolve2([]);
        const results = await Promise.all(files.map((file) => new Promise((res) => {
          const reader = new FileReader();
          reader.onload = (e) => res({ name: file.name, data: e.target.result, path: file.name, size: file.size });
          reader.readAsDataURL(file);
        })));
        resolve2(results);
      }, { once: true });
      input.click();
    });
  }
  var THEMES_DIR = "snowify/themes";
  var MKT_META_FILE = "snowify/themes/marketplace.json";
  async function scanThemes() {
    const files = await fsList(THEMES_DIR);
    return files.filter((f2) => (f2.name || f2).endsWith(".css")).map((f2) => {
      const n = f2.name || f2;
      return { id: n.replace(".css", ""), name: n.replace(".css", ""), installed: true };
    });
  }
  async function loadTheme(id) {
    return fsRead(`${THEMES_DIR}/${id}.css`);
  }
  async function addTheme() {
    const file = await pickFile(".css");
    if (!file) return null;
    const b64 = file.data.split(",")[1];
    const cssText = atob(b64);
    const id = file.name.replace(/\.css$/i, "").replace(/[^a-zA-Z0-9_-]/g, "_");
    await fsWrite(`${THEMES_DIR}/${id}.css`, cssText);
    return { id, name: id };
  }
  async function removeTheme(id) {
    await fsDelete(`${THEMES_DIR}/${id}.css`);
  }
  async function getInstalledMarketplaceThemes() {
    const raw = await fsRead(MKT_META_FILE);
    return raw ? JSON.parse(raw) : [];
  }
  async function installMarketplaceTheme(entry) {
    const resp = await fetch(entry.css);
    const css = await resp.text();
    await fsWrite(`${THEMES_DIR}/${entry.id}.css`, css);
    const installed = await getInstalledMarketplaceThemes();
    const updated = installed.filter((t) => t.id !== entry.id);
    updated.push({ id: entry.id, name: entry.name, version: entry.version });
    await fsWrite(MKT_META_FILE, JSON.stringify(updated));
    return true;
  }
  async function uninstallMarketplaceTheme(id) {
    await fsDelete(`${THEMES_DIR}/${id}.css`);
    const installed = await getInstalledMarketplaceThemes();
    await fsWrite(MKT_META_FILE, JSON.stringify(installed.filter((t) => t.id !== id)));
  }
  var PLUGINS_DIR = "snowify/plugins";
  var PLUGIN_REGISTRY_URL = "https://raw.githubusercontent.com/nyakuoff/Snowify/main/plugins/registry.json";
  async function getPluginRegistry() {
    try {
      const resp = await fetch(PLUGIN_REGISTRY_URL);
      return resp.json();
    } catch (_) {
      return [];
    }
  }
  async function getInstalledPlugins() {
    const dirs = await fsList(PLUGINS_DIR);
    const plugins = [];
    for (const d of dirs) {
      const id = d.name || d;
      const raw = await fsRead(`${PLUGINS_DIR}/${id}/snowify-plugin.json`);
      if (!raw) continue;
      try {
        const manifest = JSON.parse(raw);
        if (manifest.logoUrl && !manifest.logoUrl.startsWith("data:") && !manifest.logoUrl.startsWith("http")) {
          const b64 = await fsReadBinary(`${PLUGINS_DIR}/${id}/${manifest.logoUrl}`);
          if (b64) {
            const ext = manifest.logoUrl.split(".").pop().toLowerCase();
            const mime = ext === "svg" ? "image/svg+xml" : `image/${ext}`;
            manifest.logoUrl = `data:${mime};base64,${b64}`;
          }
        }
        plugins.push(manifest);
      } catch (_) {
      }
    }
    return plugins;
  }
  async function installPlugin(entry) {
    const base = `https://raw.githubusercontent.com/${entry.repo}/${entry.branch || "main"}/${entry.path || entry.id}`;
    const manifestText = await (await fetch(`${base}/snowify-plugin.json`)).text();
    await fsWrite(`${PLUGINS_DIR}/${entry.id}/snowify-plugin.json`, manifestText);
    if (entry.renderer) {
      const jsText = await (await fetch(`${base}/${entry.renderer}`)).text();
      await fsWrite(`${PLUGINS_DIR}/${entry.id}/${entry.renderer}`, jsText);
    }
    if (entry.styles) {
      const cssText = await (await fetch(`${base}/${entry.styles}`)).text();
      await fsWrite(`${PLUGINS_DIR}/${entry.id}/${entry.styles}`, cssText);
    }
    if (entry.logoUrl && !entry.logoUrl.startsWith("http")) {
      try {
        const logoResp = await fetch(`${base}/${entry.logoUrl}`);
        const blob = await logoResp.blob();
        const b64 = await new Promise((res) => {
          const r = new FileReader();
          r.onload = (e) => res(e.target.result.split(",")[1]);
          r.readAsDataURL(blob);
        });
        await fsWriteBinary(`${PLUGINS_DIR}/${entry.id}/${entry.logoUrl}`, b64);
      } catch (_) {
      }
    }
    return true;
  }
  async function uninstallPlugin(id) {
    try {
      await Filesystem.rmdir({ path: `${PLUGINS_DIR}/${id}`, directory: DATA_DIR, recursive: true });
    } catch (_) {
    }
  }
  async function getPluginFiles(id) {
    const manifestRaw = await fsRead(`${PLUGINS_DIR}/${id}/snowify-plugin.json`);
    if (!manifestRaw) return null;
    const manifest = JSON.parse(manifestRaw);
    const js = manifest.renderer ? await fsRead(`${PLUGINS_DIR}/${id}/${manifest.renderer}`) : null;
    const css = manifest.styles ? await fsRead(`${PLUGINS_DIR}/${id}/${manifest.styles}`) : null;
    return { manifest, js, css };
  }
  var COVERS_DIR = "snowify/covers";
  async function pickImage() {
    const file = await pickFile("image/*");
    if (!file) return null;
    return file.data;
  }
  async function saveImage(playlistId, sourceDataUrl) {
    if (!sourceDataUrl) return null;
    const b64 = sourceDataUrl.split(",")[1];
    const ext = sourceDataUrl.includes("image/png") ? "png" : "jpg";
    const path = `${COVERS_DIR}/${playlistId}.${ext}`;
    await fsWriteBinary(path, b64);
    return `snowify-cover://${playlistId}.${ext}`;
  }
  async function deleteImage(imagePath) {
    if (!imagePath) return;
    const name = imagePath.replace("snowify-cover://", "");
    await fsDelete(`${COVERS_DIR}/${name}`);
  }
  async function exportLibrary(jsonStr) {
    const blob = new Blob([jsonStr], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `snowify-library-${(/* @__PURE__ */ new Date()).toISOString().slice(0, 10)}.json`;
    document.body.appendChild(a);
    a.click();
    setTimeout(() => {
      URL.revokeObjectURL(url);
      a.remove();
    }, 1e3);
    return { ok: true };
  }
  async function importLibrary() {
    const file = await pickFile(".json");
    if (!file) return null;
    const b64 = file.data.split(",")[1];
    return atob(b64);
  }
  async function spotifyPickCsv() {
    const file = await pickFile(".csv");
    if (!file) return null;
    const b64 = file.data.split(",")[1];
    const csvText = atob(b64);
    const lines = csvText.split(/\r?\n/).filter(Boolean);
    const headers = lines[0].split(",").map((h) => h.replace(/^"|"$/g, "").trim());
    const titleIdx = headers.findIndex((h) => /title|track/i.test(h));
    const artistIdx = headers.findIndex((h) => /artist/i.test(h));
    const albumIdx = headers.findIndex((h) => /album/i.test(h));
    const tracks = [];
    for (let i = 1; i < lines.length; i++) {
      const cols = parseCSVLine(lines[i]);
      const title = cols[titleIdx]?.trim() || "";
      const artist = cols[artistIdx]?.trim() || "";
      const album = cols[albumIdx]?.trim() || "";
      if (title) tracks.push({ title, artist, album });
    }
    return tracks;
  }
  function parseCSVLine(line) {
    const result = [];
    let cur = "", inQuote = false;
    for (let i = 0; i < line.length; i++) {
      const c = line[i];
      if (c === '"') {
        inQuote = !inQuote;
      } else if (c === "," && !inQuote) {
        result.push(cur);
        cur = "";
      } else {
        cur += c;
      }
    }
    result.push(cur);
    return result;
  }
  async function spotifyMatchTrack(title, artist) {
    const results = await search(`${title} ${artist}`, true);
    return results[0] || null;
  }
  async function exportPlaylistCsv(name, tracks) {
    const header = "Title,Artist,Album\n";
    const rows = tracks.map(
      (t) => [
        `"${(t.title || "").replace(/"/g, '""')}"`,
        `"${(t.artist || "").replace(/"/g, '""')}"`,
        `"${(t.album || "").replace(/"/g, '""')}"`
      ].join(",")
    ).join("\n");
    const csv = header + rows;
    const blob = new Blob([csv], { type: "text/csv" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `${name || "playlist"}.csv`;
    document.body.appendChild(a);
    a.click();
    setTimeout(() => {
      URL.revokeObjectURL(url);
      a.remove();
    }, 1e3);
    return { ok: true };
  }
  async function pickAudioFiles() {
    const files = await pickFiles("audio/*,.mp3,.flac,.ogg,.wav,.aac,.m4a,.opus,.wma,.aiff", true);
    return files.map((f2) => ({
      path: f2.name,
      title: f2.name.replace(/\.[^.]+$/, ""),
      artist: "Unknown",
      album: "Local",
      duration: 0,
      isLocal: true,
      localPath: f2.data
      // data URL for playback
    }));
  }
  function pickAudioFolder() {
    return Promise.resolve(null);
  }
  function scanAudioFolder() {
    return Promise.resolve([]);
  }
  function copyToPlaylistFolder() {
    return Promise.resolve({ ok: true });
  }
  var GITHUB_API = "https://api.github.com/repos/nyakuoff/Snowify";
  async function getChangelog(version) {
    try {
      const resp = await fetch(`${GITHUB_API}/releases/tags/${version}`);
      if (!resp.ok) return null;
      const data = await resp.json();
      return data.body || null;
    } catch (_) {
      return null;
    }
  }
  async function getRecentReleases() {
    try {
      const resp = await fetch(`${GITHUB_API}/releases?per_page=20`);
      return resp.ok ? resp.json() : [];
    } catch (_) {
      return [];
    }
  }
  var NativeAudioShim = class extends EventTarget {
    constructor(shimId) {
      super();
      this.shimId = shimId;
      this.id = shimId === "a" ? "audio-player" : "audio-player-b";
      this._src = "";
      this._paused = true;
      this._currentTime = 0;
      this._duration = 0;
      this._volume = 1;
      this._readyState = 0;
      this._error = null;
      this.style = {};
      this.preload = "auto";
      this.className = "";
      const P = () => window.Capacitor?.Plugins?.MobilePlayer;
      const attach = () => {
        const plugin = P();
        if (!plugin) return;
        plugin.addListener("playerReady", (d) => {
          if (d.id !== this.shimId) return;
          this._duration = d.durationMs > 0 ? d.durationMs / 1e3 : 0;
          this._readyState = 4;
          this._fire("loadedmetadata");
          this._fire("canplay");
          this._fire("canplaythrough");
        });
        plugin.addListener("playerTimeUpdate", (d) => {
          if (d.id !== this.shimId) return;
          this._currentTime = d.positionMs / 1e3;
          if (d.durationMs > 0) this._duration = d.durationMs / 1e3;
          this._fire("timeupdate");
        });
        plugin.addListener("playerEnded", (d) => {
          if (d.id !== this.shimId) return;
          this._paused = true;
          this._fire("ended");
        });
        plugin.addListener("playerError", (d) => {
          if (d.id !== this.shimId) return;
          this._paused = true;
          this._error = { code: 4, message: d.message };
          this._fire("error");
        });
      };
      if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", attach, { once: true });
      } else {
        attach();
      }
    }
    _fire(name) {
      this.dispatchEvent(new Event(name));
    }
    // ── src property ──────────────────────────────────────────────────────────
    get src() {
      return this._src;
    }
    set src(url) {
      this._src = url ?? "";
      this._readyState = 0;
      this._duration = 0;
      this._currentTime = 0;
      if (this._src) {
        window.Capacitor?.Plugins?.MobilePlayer?.load({ id: this.shimId, url: this._src });
      }
    }
    // ── playback control ──────────────────────────────────────────────────────
    load() {
      if (this._src) {
        window.Capacitor?.Plugins?.MobilePlayer?.load({ id: this.shimId, url: this._src });
      }
    }
    play() {
      const plugin = window.Capacitor?.Plugins?.MobilePlayer;
      if (!plugin) return Promise.resolve();
      this._paused = false;
      return plugin.play({ id: this.shimId }).catch((e) => {
        this._paused = true;
        throw e;
      });
    }
    pause() {
      this._paused = true;
      window.Capacitor?.Plugins?.MobilePlayer?.pause({ id: this.shimId });
    }
    // ── attributes ────────────────────────────────────────────────────────────
    removeAttribute(attr) {
      if (attr === "src") {
        this._src = "";
        this._readyState = 0;
        this._paused = true;
        window.Capacitor?.Plugins?.MobilePlayer?.stop({ id: this.shimId });
      }
    }
    setAttribute() {
    }
    // no-op
    // ── volume ────────────────────────────────────────────────────────────────
    get volume() {
      return this._volume;
    }
    set volume(v) {
      this._volume = Math.max(0, Math.min(1, v));
      window.Capacitor?.Plugins?.MobilePlayer?.setVolume({ id: this.shimId, volume: this._volume });
    }
    // ── position / duration ───────────────────────────────────────────────────
    get currentTime() {
      return this._currentTime;
    }
    set currentTime(t) {
      this._currentTime = t;
      const plugin = window.Capacitor?.Plugins?.MobilePlayer;
      if (plugin) {
        plugin.seekTo({ id: this.shimId, positionMs: Math.round(t * 1e3) }).then(() => this._fire("seeked")).catch(() => {
        });
      }
    }
    get duration() {
      return this._duration || 0;
    }
    get paused() {
      return this._paused;
    }
    get readyState() {
      return this._readyState;
    }
    get error() {
      return this._error;
    }
  };
  function installMobileBridge() {
    ["audio-player", "audio-player-b"].forEach((id) => {
      const el = document.getElementById(id);
      if (el) el.removeAttribute("crossorigin");
    });
    const ua = navigator.userAgent || "";
    const platform = /android/i.test(ua) ? "android" : /iphone|ipad|ipod/i.test(ua) ? "darwin" : "linux";
    if (platform === "android") {
      const shimA = new NativeAudioShim("a");
      const shimB = new NativeAudioShim("b");
      const _origGetById = document.getElementById.bind(document);
      const _origQuerySel = document.querySelector.bind(document);
      document.getElementById = (id) => {
        if (id === "audio-player") return shimA;
        if (id === "audio-player-b") return shimB;
        return _origGetById(id);
      };
      document.querySelector = (sel) => {
        if (sel === "#audio-player") return shimA;
        if (sel === "#audio-player-b") return shimB;
        return _origQuerySel(sel);
      };
    }
    window.snowify = {
      // Platform
      platform,
      // Window controls (no titlebar on mobile)
      minimize: () => {
      },
      maximize: () => {
      },
      close: () => {
      },
      setMinimizeToTray: () => {
      },
      setOpenAtLogin: () => {
      },
      // YouTube Music
      search: (q, musicOnly) => search(q, musicOnly),
      searchArtists: (q) => searchArtists(q),
      searchAlbums: (q) => searchAlbums(q),
      searchVideos: (q) => searchVideos(q),
      searchPlaylists: (q) => searchPlaylists(q),
      getPlaylistVideos: (id) => getPlaylistVideos(id),
      searchSuggestions: (q) => searchSuggestions(q),
      // On Android, ExoPlayer fetches URLs directly — no local proxy needed.
      // On iOS, keep routing through the proxy as before.
      getStreamUrl: async (url, q) => {
        const rawUrl = await getStreamUrl(url, q);
        return platform === "android" ? rawUrl : proxyUrl(rawUrl);
      },
      getVideoStreamUrl: async (id, q, premuxed) => {
        const r = await getVideoStreamUrl(id, q, premuxed);
        return {
          videoUrl: r.videoUrl ? proxyUrl(r.videoUrl) : null,
          audioUrl: r.audioUrl ? proxyUrl(r.audioUrl) : null
        };
      },
      getTrackInfo: (id) => getTrackInfo(id),
      artistInfo: (id) => artistInfo(id),
      albumTracks: (id) => albumTracks(id),
      getUpNexts: (id) => getUpNexts(id),
      explore: () => explore(),
      charts: () => charts(),
      browseMood: (bid, params) => browseMood(bid, params),
      setCountry: () => Promise.resolve(true),
      // Caching: no-ops (no yt-dlp cache on mobile)
      downloadAudio: () => Promise.resolve(null),
      saveSong: () => Promise.resolve({ canceled: true }),
      deleteCachedAudio: () => Promise.resolve({ ok: true }),
      clearAudioCache: () => Promise.resolve({ ok: true }),
      cancelDownload: () => Promise.resolve({ ok: true }),
      // Lyrics
      getLyrics: (t, a, al, d) => getLyrics(t, a, al, d),
      // External links
      openExternal: (url) => {
        window.open(url, "_blank");
        return Promise.resolve();
      },
      // Themes
      scanThemes: () => scanThemes(),
      loadTheme: (id) => loadTheme(id),
      reloadTheme: (id) => loadTheme(id),
      addTheme: () => addTheme(),
      removeTheme: (id) => removeTheme(id),
      openThemesFolder: () => Promise.resolve(),
      getInstalledMarketplaceThemes: () => getInstalledMarketplaceThemes(),
      installMarketplaceTheme: (entry) => installMarketplaceTheme(entry),
      uninstallMarketplaceTheme: (id) => uninstallMarketplaceTheme(id),
      // Plugins
      getPluginRegistry: () => getPluginRegistry(),
      getInstalledPlugins: () => getInstalledPlugins(),
      installPlugin: (entry) => installPlugin(entry),
      uninstallPlugin: (id) => uninstallPlugin(id),
      getPluginFiles: (id) => getPluginFiles(id),
      restartApp: () => {
        location.reload();
        return Promise.resolve();
      },
      // Playlist covers
      pickImage: () => pickImage(),
      saveImage: (id, src) => saveImage(id, src),
      deleteImage: (path) => deleteImage(path),
      // Export
      exportPlaylistCsv: (n, t) => exportPlaylistCsv(n, t),
      exportLibrary: (json) => exportLibrary(json),
      importLibrary: () => importLibrary(),
      // Spotify import
      spotifyPickCsv: () => spotifyPickCsv(),
      spotifyMatchTrack: (t, a) => spotifyMatchTrack(t, a),
      // Local audio
      pickAudioFiles: () => pickAudioFiles(),
      pickAudioFolder: () => pickAudioFolder(),
      scanAudioFolder: (path) => scanAudioFolder(path),
      copyToPlaylistFolder: (fp, dir) => copyToPlaylistFolder(fp, dir),
      // App meta
      getVersion: () => Promise.resolve("2.0.0"),
      getLocale: () => Promise.resolve(navigator.language?.split("-")[0] || "en"),
      setLocale: () => Promise.resolve(true),
      // Changelog
      getChangelog: (v) => getChangelog(v),
      getRecentReleases: () => getRecentReleases(),
      // Auto-updater (no-op on mobile — handled by app store / manual APK)
      checkForUpdates: () => Promise.resolve(null),
      installUpdate: () => {
      },
      onUpdateStatus: () => {
      },
      // Thumbbar (no-op on mobile)
      updateThumbar: () => {
      },
      onThumbarPrev: () => {
      },
      onThumbarPlayPause: () => {
      },
      onThumbarNext: () => {
      },
      // Discord (no-op)
      connectDiscord: () => Promise.resolve(null),
      disconnectDiscord: () => Promise.resolve(null),
      updatePresence: () => Promise.resolve(null),
      clearPresence: () => Promise.resolve(null),
      // Graceful close (no-op — no beforeunload hook needed on mobile)
      onBeforeClose: () => {
      },
      closeReady: () => {
      },
      // Debug logs
      getLogs: () => Promise.resolve([]),
      appendLog: (entry) => {
        console.log("[PL]", entry?.message || entry);
        return Promise.resolve();
      },
      // Deep links (stub; real deep links handled by Capacitor App plugin separately)
      onDeepLink: () => {
      },
      getPendingDeepLink: () => Promise.resolve(null),
      // Generic HTTP GET for plugins (fetch goes through CapacitorHttp natively)
      httpGet: async (url, headers = {}) => {
        try {
          const resp = await fetch(url, { headers });
          const body = await resp.json().catch(() => null);
          return { status: resp.status, body };
        } catch (_) {
          return null;
        }
      }
    };
    try {
      StatusBar.setStyle({ style: Style.Dark });
      StatusBar.setBackgroundColor({ color: "#0d0d0d" });
    } catch (_) {
    }
    document.documentElement.classList.add("platform-mobile");
    document.documentElement.classList.add(`platform-${platform}`);
  }
  if (!window.snowify) {
    installMobileBridge();
  }
  return __toCommonJS(bridge_exports);
})();
/*! Bundled license information:

@capacitor/core/dist/index.js:
  (*! Capacitor: https://capacitorjs.com/ - MIT License *)
*/

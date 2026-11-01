// Media origin probe, run unchanged in Electron (a file:// page, like the app) and in Tauri
// (through the shim's proxy routing). Fixtures come from window.__MEDIA_FIXTURES__:
// { png, wav, wavHash: absolute paths; gv: googlevideo URL or null }.
(async () => {
  const fx = window.__MEDIA_FIXTURES__;
  const out = {};
  const fileUrl = (p) => 'file://' + p; // utils.js pathToFileUrl for POSIX paths
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  try {
    // A cover inserted the way library.js does it: an innerHTML string.
    const holder = document.createElement('div');
    holder.innerHTML = `<img src="${fileUrl(encodeURI(fx.png))}" alt="" />`;
    document.body.appendChild(holder);
    const img = holder.firstChild;
    await new Promise((r) => { img.onload = img.onerror = r; setTimeout(r, 5000); });
    out.img = { width: img.naturalWidth, attr: img.getAttribute('src'), src: img.src, currentSrc: img.currentSrc };

    // A missing file: error event, no size.
    const bad = new Image();
    bad.src = fileUrl('/nonexistent/snowify-missing.png');
    out.missing = await new Promise((r) => { bad.onload = () => r('load'); bad.onerror = () => r('error'); setTimeout(() => r('timeout'), 5000); });

    // Local audio through Web Audio, as player.js plays cached/local files.
    const measure = async (src) => {
      const ctx = new AudioContext();
      const a = new Audio();
      a.crossOrigin = 'anonymous';
      a.src = src;
      const res = { attr: a.getAttribute('src'), src: a.src };
      const node = ctx.createMediaElementSource(a);
      const an = ctx.createAnalyser();
      node.connect(an); an.connect(ctx.destination);
      await ctx.resume();
      try { await a.play(); res.played = true; } catch (e) { res.played = String(e.name); }
      await wait(3000);
      const buf = new Float32Array(an.fftSize);
      an.getFloatTimeDomainData(buf);
      res.audible = Math.sqrt(buf.reduce((s, x) => s + x * x, 0) / buf.length) > 0.01;
      res.currentSrc = a.currentSrc;
      res.advanced = a.currentTime > 1;
      a.pause();
      await ctx.close();
      return res;
    };
    out.wav = await measure(fileUrl(fx.wav));
    // pathToFileUrl does not escape '#': the rest of the name becomes a fragment.
    out.wavHash = await measure(fileUrl(fx.wavHash));
    if (fx.gv) {
      const r = await measure(fx.gv);
      out.gv = { audible: r.audible, played: r.played, advanced: r.advanced, attrIsOriginal: r.attr === fx.gv, srcIsOriginal: r.src === fx.gv, currentSrcIsOriginal: r.currentSrc === fx.gv };
    }
  } catch (e) {
    out.error = `${e.name}: ${e.message}`;
  }
  await probe.done(JSON.stringify(out));
})();

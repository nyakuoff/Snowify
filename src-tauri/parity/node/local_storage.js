// Evaluates the actual Rust-generated initialization script in Electron 33's V8.
const fs=require('fs'), vm=require('vm');
const input=JSON.parse(fs.readFileSync(0,'utf8'));
const units=s=>Array.from({length:s.length},(_,i)=>s.charCodeAt(i));
function run(initial, failAt, origin='tauri://localhost') {
  const map=new Map(initial), before=Array.from(map);
  let calls=0;
  const localStorage={getItem:k=>map.has(k)?map.get(k):null,
    setItem(k,v){if (++calls===failAt) throw Error('quota');map.set(String(k),String(v));},
    removeItem:k=>map.delete(k)};
  const window={localStorage};window.top=window;
  vm.runInNewContext(input.script,{window,location:{origin},console:{error(){}}});
  return {pairs:Array.from(map,([k,v])=>[units(k),units(v)]),unchanged:JSON.stringify(before)===JSON.stringify(Array.from(map))};
}
const empty=run([]);
const guards=['snowify_state','snowfy_state',input.marker].map(k=>run([[k,'']]));
const failure=run([['ascii','previous']],3);
// Only the app's own origins (Linux, Windows) receive the data.
guards.push(run([],0,'https://example.com'), run([],0,'null'));
const windows=run([],0,'http://tauri.localhost');
console.log(JSON.stringify({empty,windows,guards,failure}));

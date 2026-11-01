// Generates src/ytm/schemas.json: prints zodToJsonSchema(type, "schema") for each type checkType uses, from ytmusic-api's
// own bundle (its exports don't include the schemas, so they are pulled out of the module text).
const fs = require('fs');
const path = require('path');
const src = fs.readFileSync(require.resolve('ytmusic-api'), 'utf8');
const names = ['PlaylistFull', 'PlaylistDetailed', 'SongFull', 'SongDetailed', 'AlbumFull', 'AlbumDetailed', 'VideoDetailed', 'ArtistFull', 'ArtistDetailed'];
const body = src.replace(/module\.exports = exports\.default;?\s*$/, '') + `\nmodule.exports = { zodToJsonSchema, ${names.join(', ')} };`;
const m = new module.constructor();
m.paths = module.paths.concat([path.dirname(require.resolve('ytmusic-api'))]);
m._compile(body, require.resolve('ytmusic-api'));
const out = {};
for (const n of names) out[n] = m.exports.zodToJsonSchema(m.exports[n], 'schema');
process.stdout.write(JSON.stringify(out));

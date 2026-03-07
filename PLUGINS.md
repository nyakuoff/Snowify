# Snowify Plugins

Snowify supports curated plugins from the marketplace. This document explains the plugin format, the official monorepo, and how third-party developers can submit plugins.

## Plugin Types

| Type | Repo | Tag in app |
|---|---|---|
| **Official** | [nyakuoff/Snowify-Plugins](https://github.com/nyakuoff/Snowify-Plugins) (monorepo) | `Official` |
| **Third-party** | Author's own repo | `Community` |

## Plugin Structure

Every plugin (whether official or third-party) has the same file structure:

```
my-plugin/
  snowify-plugin.json   # Plugin manifest (required)
  renderer.js           # Renderer script, injected into the app (required)
  styles.css            # CSS styles (optional)
```

- **Official plugins** live as subdirectories in the `nyakuoff/snowify-plugins` monorepo.
- **Third-party plugins** live at the root of their own GitHub repo.

### Manifest (`snowify-plugin.json`)

```json
{
  "id": "my-plugin",
  "name": "My Plugin",
  "version": "1.0.0",
  "description": "A short description of what the plugin does",
  "author": "your-github-username",
  "renderer": "renderer.js",
  "styles": "styles.css",
  "minAppVersion": "1.3.0"
}
```

| Field | Required | Description |
|---|---|---|
| `id` | Yes | Unique identifier (lowercase, hyphens only) |
| `name` | Yes | Display name |
| `version` | Yes | Semver version string |
| `description` | Yes | Short description (shown in marketplace) |
| `author` | Yes | Author name or GitHub username |
| `renderer` | Yes | Path to the main renderer JS file |
| `styles` | No | Path to the CSS file |
| `minAppVersion` | No | Minimum Snowify version required |

### Renderer Script

Your `renderer.js` is injected into the app's renderer process. It has access to:

- The full DOM (`document.querySelector`, etc.)
- The `window.snowify` IPC bridge (search, streaming, lyrics, etc.)
- `I18n.t(key)` for localization
- Standard Web APIs

Wrap your plugin in an IIFE to avoid polluting the global scope:

```js
(function() {
  'use strict';
  // Your plugin code here
  console.log('My plugin loaded!');
})();
```

### CSS

Your `styles.css` is injected as a `<style>` tag. Use unique class prefixes to avoid collisions (e.g., `.myplugin-*`).

## Submitting a Plugin

### Third-party plugins

1. Create your plugin in a public GitHub repository (manifest at repo root)
2. Ensure it has a valid `snowify-plugin.json` manifest
3. Open a Pull Request on [nyakuoff/Snowify](https://github.com/nyakuoff/Snowify) adding your entry to `plugins/registry.json`

### Registry Entry Format

**Third-party** (manifest at repo root — no `path` needed):

```json
{
  "id": "my-plugin",
  "name": "My Plugin",
  "description": "A short description",
  "author": "your-username",
  "version": "1.0.0",
  "icon": "🔌",
  "repo": "your-username/snowify-my-plugin"
}
```

**Official** (monorepo subdirectory — uses `path`):

```json
{
  "id": "internet-radio",
  "name": "Internet Radio",
  "description": "Listen to radio stations worldwide",
  "author": "Snowify Team",
  "version": "1.0.0",
  "icon": "📻",
  "repo": "nyakuoff/Snowify-Plugins",
  "path": "internet-radio",
  "official": true
}
```

| Field | Required | Description |
|---|---|---|
| `id` | Yes | Unique identifier (lowercase, hyphens only) |
| `name` | Yes | Display name |
| `description` | Yes | Short description (shown in marketplace) |
| `author` | Yes | Author name or GitHub username |
| `version` | Yes | Semver version string |
| `repo` | Yes | GitHub `owner/repo` to download from |
| `icon` | No | Emoji icon shown in the card |
| `path` | No | Subdirectory in the repo (for monorepos) |
| `branch` | No | Branch to fetch from (default: `main`) |
| `official` | No | `true` for official Snowify plugins |

All plugins are reviewed before being added to the marketplace. The repository owner has final approval on which plugins are listed.

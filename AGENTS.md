# Snowify Agent Instructions

This file defines required rules for any human or AI agent modifying Snowify.
Follow these rules before writing code.

## 1. Project Mission

Snowify is an Electron music player. The renderer is modular ES modules.
Do not re-grow a monolithic renderer file. Keep logic in module files.

## 2. Core Architecture

- Main renderer entry: `src/renderer/app.js`
- Feature modules: `src/renderer/modules/*.js`
- State singleton: `src/renderer/modules/state.js`
- Shared utils: `src/renderer/modules/utils.js`
- Theme system: `src/renderer/modules/theme.js`

`app.js` should only contain:
- Boot/init orchestration
- Global cloud-sync lifecycle and persistence orchestration
- Welcome overlay state glue
- Global app-level event wiring that is intentionally cross-feature
- Callback injection into module callback objects

All feature logic belongs in modules.

## 3. Module Ownership

Keep responsibilities in these files:

- `player.js`: playback transport, queue stepping integration, volume/progress, now-playing bar controls, prefetch/normalizer accessors
- `queue.js`: queue panel, history panel, play-next/add-to-queue/radio helpers
- `library.js`: playlists, playlist detail view, drag/drop reorder and track movement
- `context-menus.js`: all context menu rendering and actions
- `home.js`: home view rendering and release cache
- `explore.js`: explore charts/trending rendering and explore cache
- `album.js`: album and external playlist detail pages
- `artist.js`: artist page rendering, artist navigation, artist link binding
- `search.js`: search view behavior, suggestions, search history UI interactions
- `settings.js`: settings page behavior, auth/profile controls, updater/changelog/dev section/source lists
- `plugins.js`: plugin loading/state and marketplace rendering
- `lyrics.js`: lyrics panel and sync behavior
- `now-playing.js`: full-screen now-playing and lyrics styling/sync there
- `video-player.js`: video overlay/miniplayer behavior
- `audio-ref.js`: shared audio engine/audio element references and volume constants
- `callbacks.js`: cross-module callback injection for save/view/meta bridges

If you touch functionality, edit the owning module.

## 4. Cross-Module Contract Rules

### 4.1 callbacks injection

`callbacks.js` is dependency-inversion glue for modules that need app-owned functions.

- `callbacks.saveState`
- `callbacks.maybeEnrichTrackMeta`
- `callbacks.switchView`

Rules:
- Modules call these callbacks.
- `app.js` must assign real implementations in `finishInit()`.
- Do not import `app.js` from modules.

### 4.2 settingsCallbacks injection

`settings.js` exports `settingsCallbacks` to avoid settings importing app internals.

Required app assignments in `finishInit()`:
- `forceCloudSave`
- `cloudLoadAndMerge`
- `updateSyncStatus`
- `getCloudUser`
- `getCloudSyncPaused`
- `setCloudSyncPaused`
- `clearCloudSaveTimeout`
- `showWelcomeOverlay`
- `hideWelcomeOverlay`
- `setWelcomeError`
- `updateAccountUI`

### 4.3 Circular dependency policy

- Avoid cycles by using callback injection or argument injection.
- If needed, dynamic import inside event handlers is acceptable for low-frequency UI actions.
- Never solve cycles by copying logic across files.

## 5. State and Persistence Rules

- Global mutable app data must live on `state`.
- Any user-visible persistent change must call `callbacks.saveState()`.
- Queue/play log/genre cache persistence shape must remain backward compatible.
- Cloud payload must not include local-only tracks.

## 6. UI and i18n Rules

- User-facing strings must use `I18n.t(...)` keys.
- Do not hardcode display text unless it is a strict temporary debug message.
- Preserve mobile behavior and gestures.
- Respect reduced-motion and existing animation toggles.

## 7. Styling and DOM Rules

- Reuse existing CSS classes and component patterns.
- Do not introduce major style-system changes without updating all related views.
- Keep DOM queries local to module scope unless shared by design.

## 8. Plugin and Source Registry Rules

- `window.SnowifySources` registration API is app-owned and initialized in `finishInit()` before plugin use.
- Plugins may register song/meta sources only through registry methods.
- Source list UI (`settings.js`) is the single source-order control surface.

## 9. Safety Rules for Edits

Before editing:
- Identify owner module.
- Check for existing callback contract.
- Check for i18n keys and mobile behavior impact.

When editing:
- Make the smallest targeted change.
- Do not move unrelated logic.
- Do not rename public exports unless all imports are updated.

After editing:
- Run syntax checks for touched files.
- Verify no renderer compile errors.
- Verify no unresolved references.

## 10. Required Validation Checklist

For renderer changes, run all:

1. `node --check src/renderer/app.js`
2. `node --check` for every touched module file
3. Editor diagnostics clean for touched files

For behavior-sensitive changes, ask for manually verification:

1. App boot succeeds
2. Navigation between home/explore/search/library/settings works
3. Playback controls and keyboard shortcuts work
4. Settings open and save flows work
5. Plugin list still loads

## 11. Anti-Patterns (Do Not Do)

- Do not add new large feature blocks back into `app.js`.
- Do not directly import app internals into modules.
- Do not duplicate helper logic between modules when a shared helper exists.
- Do not bypass `callbacks.saveState()` for persisted state updates.
- Do not use destructive git commands to "fix" unrelated changes.

## 12. Adding New Features

When adding a feature:

1. Pick or create a feature module under `src/renderer/modules/`.
2. Export only the public functions needed by callers.
3. Add callback injection points if app-owned dependencies are needed.
4. Wire imports and callback assignments in `app.js`.
5. Add i18n keys to locale files.
6. Run validation checklist.

## 13. Migration Discipline

If extracting code from `app.js`:

1. Move code to module with minimal behavior change.
2. Export moved functions.
3. Replace old logic with imports/callback wiring.
4. Remove dead code from `app.js` immediately.
5. Verify all references compile.

The target is stable modular architecture, not temporary dual paths.

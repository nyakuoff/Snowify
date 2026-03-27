# Changelog

All notable changes to this project are documented in this file.

## Unreleased

### Added
- GitHub Actions Android APK build in `.github/workflows/release.yml`.
- APK release asset upload in release finalization step (`Snowify-v<version>-android-debug.apk`).

### Changed
- Mobile playback fallback strategy now prioritizes InnerTube clients that return playable data on restricted tracks (`ANDROID`, then `IOS`, then `ANDROID_VR`).
- Mobile HTTP JSON parsing now handles empty/non-JSON string responses more safely before fallback logic runs.
- Mobile bridge now resolves and proxifies remote artwork URLs across search, profile, and cloud/library surfaces for more reliable image loading on Android.
- Renderer image application pipeline now resolves artwork URLs before lazy load probing and direct image assignment (now playing, max player, plugin artwork).
- Mobile layout spacing and alignment refinements in settings/account views to prevent edge collisions and improve content gutters.
- Home recently played carousel alignment refined to match section text start and preserve balanced side spacing.

## 2026-03-27

### Fixed
- Mobile playback failing with `LOGIN_REQUIRED` and `No stream URLs found` on affected tracks by using a resilient multi-client player request chain.
- Mobile image regression where artwork failed to display consistently due to unresolved or non-proxied URLs.
- Mobile settings account panel content touching screen borders.
- Recently played cards not aligning with home section text baseline.

### Internal
- Regenerated bundled mobile bridge (`src/renderer/mobile-bridge.js`) after bridge/runtime changes.
- Updated release workflow to include Android APK artifact generation and attachment.

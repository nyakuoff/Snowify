# Contributing to Snowify

Thanks for your interest in contributing! All skill levels are welcome.

## Before You Start

- Check existing [issues](https://github.com/nyakuoff/Snowify/issues) and [pull requests](https://github.com/nyakuoff/Snowify/pulls) to avoid duplicating work.
- For large changes, open an issue first to discuss the approach before writing code.
- Read [AGENTS.md](AGENTS.md) if you're touching renderer code — it defines module ownership and required validation steps.

## Reporting Bugs

Use the **Bug Report** issue template. Include:
- Steps to reproduce
- What you expected vs what actually happened
- Your OS and Snowify version

## Suggesting Features

Use the **Feature Request** issue template. Explain the use case, not just the feature.

## Submitting a Pull Request

1. Fork the repo and create a branch from `main`.
2. Make your changes, following the guidelines below.
3. Run the validation checklist (see [AGENTS.md §10](AGENTS.md#10-required-validation-checklist)).
4. Open a PR against `main` using the pull request template.

### Code Guidelines

- Keep changes focused — one concern per PR.
- Follow the module ownership rules in [AGENTS.md](AGENTS.md). Feature logic belongs in `src/renderer/modules/`, not `app.js`.
- All user-facing strings must use `I18n.t(...)` — no hardcoded display text.
- If you change `src/mobile/bridge.js`, run `npm run build:mobile` and commit `src/renderer/mobile-bridge.js` alongside it.
- Run `node --check` on any touched renderer files before opening a PR.

## Translating

Snowify supports multiple languages through JSON files in `src/renderer/locales/`.

1. **Copy the English file** — Duplicate `src/renderer/locales/en.json` and rename it to your language's [ISO 639-1 code](https://en.wikipedia.org/wiki/List_of_ISO_639-1_codes) (e.g. `nl.json` for Dutch).
2. **Translate the values** — Each file is a flat `"key": "value"` JSON object. Translate only the **values**, never the keys. Keep any `{{placeholder}}` variables intact:
   ```json
   "home.greeting.morning": "Goedemorgen",
   "queue.trackCount": "{{count}} nummers"
   ```
3. **Register the language** — Add the new language code to the `SUPPORTED` array in `src/renderer/i18n.js`, add an `<option>` for it in the language selector in `src/renderer/index.html`, and add it to the supported languages list in the README.
4. **Open a PR** — Submit your translation file along with the registration changes.

**Currently supported:** English, Spanish, Portuguese, French, German, Japanese, Korean, Chinese, Italian, Turkish, Russian, Hindi.

## AI Disclaimer

Parts of this project were assisted or written by AI. If that's something you're not comfortable with, no hard feelings. The code may have flaws — if you spot something that could be improved, contributions are very welcome.

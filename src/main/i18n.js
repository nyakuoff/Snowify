const path = require('path');
const fs = require('fs');
const { app } = require('electron');

const _mainTranslations = {};

function loadMainTranslations(overrideLocale) {
  const supported = ['en','es','pt','fr','de','ja','ko','zh','it','tr','ru','hi'];
  const lang = (overrideLocale || app.getLocale()).split('-')[0].toLowerCase();
  const locale = supported.includes(lang) ? lang : 'en';
  const filePath = path.join(__dirname, '..', 'renderer', 'locales', locale + '.json');
  try {
    _mainTranslations.data = JSON.parse(fs.readFileSync(filePath, 'utf-8'));
  } catch {
    _mainTranslations.data = JSON.parse(fs.readFileSync(path.join(__dirname, '..', 'renderer', 'locales', 'en.json'), 'utf-8'));
  }
}

function mt(key, params) {
  let str = _mainTranslations.data?.[key] ?? key;
  if (params) for (const [k, v] of Object.entries(params)) str = str.replaceAll('{{' + k + '}}', v);
  return str;
}

module.exports = { loadMainTranslations, mt };

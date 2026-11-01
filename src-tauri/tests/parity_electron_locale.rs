//! Replays recorded Electron 33 `app.getLocale()` results (`parity/corpus/electron-locale.tsv`)
//! through `snowify::electron_locale`. The `LANGUAGE=german` case depends on the system's
//! `/usr/share/locale/locale.alias`, as it did when recorded.

use serde_json::json;
use snowify::electron_locale::{app_locale, parse_locale_alias};
use std::collections::HashMap;

mod common;
use common::{Diff, root};

#[test]
fn app_locale_matches_recorded_electron() {
    let aliases = parse_locale_alias(
        &std::fs::read_to_string("/usr/share/locale/locale.alias").unwrap_or_default(),
    );
    let corpus = std::fs::read_to_string(root().join("parity/corpus/electron-locale.tsv")).unwrap();
    let mut d = Diff::default();
    for line in corpus
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let (envs, expected) = line.split_once('\t').unwrap();
        let env: HashMap<&str, &str> = envs
            .split(' ')
            .filter_map(|kv| kv.split_once('='))
            .collect();
        let got = app_locale(|k| env.get(k).map(|v| v.to_string()), &aliases);
        d.check(envs, &json!(expected), &json!(got));
    }
    assert!(d.compared > 1000, "corpus too small: {}", d.compared);
    d.finish("electron_locale");
}

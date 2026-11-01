//! Pure TaskbarHost mapping tests run on Linux too; Wine's taskbar COM is a stub.
mod common;
use snowify::{
    i18n::Translations,
    thumbbar::{self, BUTTON_ID_BASE, Button, Icon, Operation, State},
};
fn translations(locale: &str) -> Translations {
    Translations::load(&common::root().join("../src/renderer/locales"), locale).unwrap()
}
#[test]
fn thumbbar_buttons_match_snowify_and_electron_33() {
    // taskbar_host.cc@v33.4.11: ID base 40001, THB_FLAGS=8, ICON=2, TOOLTIP=4,
    // ENABLED=0, HIDDEN=8. It always submits seven buttons, including four fillers.
    for (playing, middle, tip) in [(false, Icon::Play, "Play"), (true, Icon::Pause, "Pause")] {
        let buttons = thumbbar::buttons(&translations("en"), playing);
        let slots = thumbbar::slots(&buttons).unwrap();
        assert_eq!(
            buttons.iter().map(|b| b.icon.unwrap()).collect::<Vec<_>>(),
            vec![Icon::Prev, middle, Icon::Next]
        );
        assert_eq!(
            buttons.iter().map(|b| b.channel).collect::<Vec<_>>(),
            vec!["thumbar:prev", "thumbar:playPause", "thumbar:next"]
        );
        assert_eq!(
            buttons
                .iter()
                .map(|b| b.tooltip.as_str())
                .collect::<Vec<_>>(),
            vec!["Previous", tip, "Next"]
        );
        for (i, slot) in slots.iter().enumerate() {
            assert_eq!(slot.id, 40001 + i as u32);
            assert_eq!(
                (slot.mask, slot.flags),
                if i < 3 { (14, 0) } else { (8, 8) }
            );
            assert_eq!(slot.channel.is_some(), i < 3);
        }
    }
    let french = thumbbar::buttons(&translations("fr"), false);
    assert_eq!(
        french[0].tooltip,
        translations("fr")
            .mt("player.previous", &[])
            .unwrap()
            .to_string()
    );
    assert_ne!(french[0].tooltip, "Previous");
    let fallback = thumbbar::buttons(&Translations::default(), false);
    assert_eq!(fallback[1].tooltip, "player.play");
}
#[test]
fn thumbbar_flags_limit_tooltips_and_clicks_match_taskbar_host() {
    let make = |flags: Vec<String>| Button {
        icon: None,
        tooltip: String::new(),
        flags,
        channel: "thumbar:next",
    };
    for (flag, bits) in [
        ("disabled", 1),
        ("dismissonclick", 2),
        ("nobackground", 4),
        ("hidden", 8),
        ("noninteractive", 16),
    ] {
        let mut state = State::default();
        let (_, slots) = state.apply(vec![make(vec![flag.into()])]).unwrap();
        assert_eq!((slots[0].mask, slots[0].flags), (8, bits));
        // Flags affect taskbar interaction; Electron still registers each supplied callback.
        assert_eq!(state.click((0x1800 << 16) | 40001), Some("thumbar:next"));
    }
    assert_eq!(
        thumbbar::flags(
            &[
                "disabled",
                "dismissonclick",
                "nobackground",
                "hidden",
                "noninteractive"
            ]
            .map(str::to_owned)
        ),
        Ok(31)
    );
    for unknown in ["enabled", "Disabled", ""] {
        assert!(thumbbar::flags(&[unknown.into()]).is_err());
    }
    assert!(thumbbar::slots(&vec![make(vec![]); 8]).is_err());
    assert_eq!(thumbbar::slots(&vec![make(vec![]); 7]).unwrap().len(), 7);
    assert!(
        thumbbar::slots(&[])
            .unwrap()
            .iter()
            .all(|s| s.flags == 8 && s.mask == 8)
    );
    let mut b = make(vec![]);
    b.tooltip = format!("{}🎵", "a".repeat(258));
    let s = thumbbar::slots(&[b]).unwrap();
    assert_eq!(s[0].tip[258], 0xd83c);
    assert_eq!(s[0].tip[259], 0);
    let mut state = State::default();
    state
        .apply(thumbbar::buttons(&translations("en"), false))
        .unwrap();
    assert_eq!(
        state.click((0x1800 << 16) | 40002),
        Some("thumbar:playPause")
    );
    for wparam in [
        40001,
        (0x1801 << 16) | 40001,
        (0x1800 << 16) | 40000,
        (0x1800 << 16) | 40004,
    ] {
        assert_eq!(state.click(wparam), None);
    }
}
#[test]
fn thumbbar_add_update_restore_lifetime_matches_electron() {
    let buttons = thumbbar::buttons(&translations("en"), false);
    let mut state = State::default();
    assert_eq!(state.apply(buttons.clone()).unwrap().0, Operation::Add);
    // State remains added even if the COM HRESULT fails; that is Electron's policy.
    assert!(state.added);
    assert_eq!(state.apply(buttons.clone()).unwrap().0, Operation::Update);
    assert_eq!(
        state
            .apply(thumbbar::buttons(&translations("en"), true))
            .unwrap()
            .0,
        Operation::Update
    );
    assert_eq!(state.last[1].icon, Some(Icon::Pause));
    // Explorer restart: RestoreThumbarButtons clears added and replays last buttons.
    state.added = false;
    assert_eq!(state.apply(state.last.clone()).unwrap().0, Operation::Add);
    // win.hide resets added; the next renderer update must Add, not Update.
    state.added = false;
    assert_eq!(state.apply(buttons).unwrap().0, Operation::Add);
    assert_eq!(BUTTON_ID_BASE, 40001);
    #[cfg(windows)]
    {
        use windows::Win32::UI::Shell::*;
        assert_eq!((THB_FLAGS.0, THB_ICON.0, THB_TOOLTIP.0), (8, 2, 4));
        assert_eq!((THBF_ENABLED.0, THBF_DISABLED.0, THBF_HIDDEN.0), (0, 1, 8));
        assert_eq!(THBN_CLICKED, 0x1800);
    }
}
#[cfg(windows)]
#[path = "windows/login_items.rs"]
mod login_items;

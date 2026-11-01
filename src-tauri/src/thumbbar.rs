//! `src/main/thumbbar.js` plus Electron 33 TaskbarHost's seven-slot button mapping.
use crate::i18n::Translations;

pub const MAX_BUTTONS: usize = 7;
pub const BUTTON_ID_BASE: u32 = 40001;
pub const CLICKED: usize = 0x1800;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Prev,
    Play,
    Pause,
    Next,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Button {
    pub tooltip: String,
    pub icon: Option<Icon>,
    pub flags: Vec<String>,
    pub channel: &'static str,
}
pub fn buttons(translations: &Translations, playing: bool) -> Vec<Button> {
    [
        ("player.previous", Icon::Prev, "thumbar:prev"),
        (
            if playing {
                "player.pause"
            } else {
                "player.play"
            },
            if playing { Icon::Pause } else { Icon::Play },
            "thumbar:playPause",
        ),
        ("player.next", Icon::Next, "thumbar:next"),
    ]
    .into_iter()
    .map(|(key, icon, channel)| Button {
        tooltip: translations
            .mt(key, &[])
            .map(|s| s.to_string())
            .unwrap_or_else(|_| key.into()),
        icon: Some(icon),
        flags: vec![],
        channel,
    })
    .collect()
}
pub fn flags(flags: &[String]) -> Result<u32, &'static str> {
    flags.iter().try_fold(0, |bits, flag| {
        Ok(bits
            | match flag.as_str() {
                "disabled" => 1,
                "dismissonclick" => 2,
                "nobackground" => 4,
                "hidden" => 8,
                "noninteractive" => 16,
                _ => return Err("unknown thumbbar flag"),
            })
    })
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slot {
    pub id: u32,
    pub mask: u32,
    pub flags: u32,
    pub tip: [u16; 260],
    pub icon: Option<Icon>,
    pub channel: Option<&'static str>,
}
pub fn slots(buttons: &[Button]) -> Result<[Slot; MAX_BUTTONS], &'static str> {
    if buttons.len() > MAX_BUTTONS {
        return Err("at most seven thumbbar buttons");
    }
    let mut out = std::array::from_fn(|i| Slot {
        id: BUTTON_ID_BASE + i as u32,
        mask: 8,
        flags: 8,
        tip: [0; 260],
        icon: None,
        channel: None,
    });
    for (slot, button) in out.iter_mut().zip(buttons) {
        slot.flags = flags(&button.flags)?;
        slot.icon = button.icon;
        slot.channel = Some(button.channel);
        if button.icon.is_some() {
            slot.mask |= 2;
        }
        if !button.tooltip.is_empty() {
            slot.mask |= 4;
            for (dest, unit) in slot.tip[..259]
                .iter_mut()
                .zip(button.tooltip.encode_utf16().take_while(|u| *u != 0))
            {
                *dest = unit;
            }
        }
    }
    Ok(out)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Add,
    Update,
}
#[derive(Default)]
pub struct State {
    pub added: bool,
    pub last: Vec<Button>,
}
impl State {
    // Electron marks added and retains last even when Add/Update returns a failing HRESULT.
    pub fn apply(
        &mut self,
        buttons: Vec<Button>,
    ) -> Result<(Operation, [Slot; MAX_BUTTONS]), &'static str> {
        let slots = slots(&buttons)?;
        let operation = if self.added {
            Operation::Update
        } else {
            Operation::Add
        };
        self.added = true;
        self.last = buttons;
        Ok((operation, slots))
    }
    pub fn click(&self, wparam: usize) -> Option<&'static str> {
        if wparam >> 16 & 0xffff != CLICKED {
            return None;
        }
        let i = (wparam & 0xffff).checked_sub(BUTTON_ID_BASE as usize)?;
        self.last.get(i).map(|b| b.channel)
    }
}
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{install, update};

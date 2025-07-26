use indexmap::IndexMap;
use serde::Deserialize;

use crate::color::Color;
use crate::key::SingleKey;

use super::{ConfigAnchor, Font};

#[derive(Deserialize, Default)]
#[serde(transparent)]
pub struct Entries(pub IndexMap<SingleKey, Entry>);

#[derive(Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    pub background: Color,
    pub color: Color,
    pub border: Color,

    pub anchor: ConfigAnchor,
    pub margin_top: i32,
    pub margin_right: i32,
    pub margin_bottom: i32,
    pub margin_left: i32,

    pub font: Font,
    pub separator: String,
    pub border_width: f64,
    pub corner_r: f64,
    // defaults to `corner_r`
    pub padding: Option<f64>,

    pub menu: Entries,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            background: Color::from_rgba_hex(0x282828ff),
            color: Color::from_rgba_hex(0xfbf1c7ff),
            border: Color::from_rgba_hex(0x8ec07cff),
            anchor: ConfigAnchor::default(),
            margin_top: i32::default(),
            margin_right: i32::default(),
            margin_bottom: i32::default(),
            margin_left: i32::default(),
            font: Font::new("monospace 10"),
            separator: " ➜ ".into(),
            border_width: 4.0,
            corner_r: 20.0,
            padding: Option::default(),
            menu: Entries::default(),
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum Entry {
    Cmd {
        cmd: String,
        desc: String,
        #[serde(default)]
        keep_open: bool,
    },
    Recursive {
        submenu: Entries,
        desc: String,
    },
}

impl From<Config> for super::Config {
    fn from(value: Config) -> Self {
        fn map_entries(value: Entries) -> Vec<super::Entry> {
            value
                .0
                .into_iter()
                .map(|(key, entry)| match entry {
                    Entry::Cmd {
                        cmd,
                        desc,
                        keep_open,
                    } => super::Entry::Cmd {
                        key: key.into(),
                        cmd,
                        desc,
                        keep_open,
                    },
                    Entry::Recursive { submenu, desc } => super::Entry::Recursive {
                        key: key.into(),
                        submenu: map_entries(submenu),
                        desc,
                    },
                })
                .collect()
        }

        Self {
            background: value.background,
            color: value.color,
            border: value.border,
            anchor: value.anchor,
            margin_top: value.margin_top,
            margin_right: value.margin_right,
            margin_bottom: value.margin_bottom,
            margin_left: value.margin_left,
            font: value.font,
            separator: value.separator,
            border_width: value.border_width,
            corner_r: value.corner_r,
            padding: value.padding,
            rows_per_column: None,
            column_padding: None,
            menu: map_entries(value.menu),
            inhibit_compositor_keyboard_shortcuts: false,
            auto_kbd_layout: false,
        }
    }
}

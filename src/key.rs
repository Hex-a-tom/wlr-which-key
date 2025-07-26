use std::fmt;
use std::str::FromStr;

use serde::de;
use smithay_client_toolkit::seat::keyboard::{Keysym, Modifiers};

#[derive(Clone)]
pub struct Key {
    any_of: Vec<SingleKey>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ModifierState {
    pub mod_ctrl: bool,
    pub mod_alt: bool,
    pub mod_mod4: bool,
}

impl ModifierState {
    pub fn from_sctk_modifiers(mods: &Modifiers) -> Self {
        Self {
            mod_ctrl: mods.ctrl,
            mod_alt: mods.alt,
            mod_mod4: mods.logo,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct SingleKey {
    pub keysym: Keysym,
    pub repr: String,
    pub modifiers: ModifierState,
}

impl Key {
    pub fn matches(&self, sym: Keysym, modifiers: ModifierState) -> bool {
        self.any_of
            .iter()
            .any(|key| key.modifiers == modifiers && key.keysym == sym)
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, key) in self.any_of.iter().enumerate() {
            f.write_str(&key.repr)?;
            if i + 1 != self.any_of.len() {
                f.write_str(" | ")?
            }
        }
        Ok(())
    }
}

impl From<SingleKey> for Key {
    fn from(value: SingleKey) -> Self {
        Self {
            any_of: vec![value],
        }
    }
}

impl FromStr for SingleKey {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "+" {
            return Ok(Self {
                keysym: Keysym::plus,
                repr: String::from("+"),
                modifiers: Default::default(),
            });
        }

        let mut components = s.split('+');
        let key = components.next_back().unwrap_or(s);
        let keysym = to_keysym(key).ok_or_else(|| format!("invalid key '{key}'"))?;

        let mut modifiers = ModifierState::default();
        for modifier in components {
            if modifier.eq_ignore_ascii_case("ctrl") {
                modifiers.mod_ctrl = true;
            } else if modifier.eq_ignore_ascii_case("alt") {
                modifiers.mod_alt = true;
            } else if modifier.eq_ignore_ascii_case("mod4") || modifier.eq_ignore_ascii_case("logo")
            {
                modifiers.mod_mod4 = true;
            } else {
                return Err(format!("unknown modifier '{modifier}"));
            }
        }

        Ok(Self {
            keysym,
            repr: s.to_owned(),
            modifiers,
        })
    }
}

fn to_keysym(s: &str) -> Option<Keysym> {
    let mut chars = s.chars();
    let first_char = chars.next()?;

    let keysym = if chars.next().is_none() {
        Keysym::from_char(first_char)
    } else {
        match &*s.to_ascii_uppercase() {
            "F1" => Keysym::F1,
            "F2" => Keysym::F2,
            "F3" => Keysym::F3,
            "F4" => Keysym::F4,
            "F5" => Keysym::F5,
            "F6" => Keysym::F6,
            "F7" => Keysym::F7,
            "F8" => Keysym::F8,
            "F9" => Keysym::F9,
            "F10" => Keysym::F10,
            "F11" => Keysym::F11,
            "F12" => Keysym::F12,
            "F13" => Keysym::F13,
            "F14" => Keysym::F14,
            "F15" => Keysym::F15,
            "F16" => Keysym::F16,
            "F17" => Keysym::F17,
            "F18" => Keysym::F18,
            "F19" => Keysym::F19,
            "F20" => Keysym::F20,
            "F21" => Keysym::F21,
            "F22" => Keysym::F22,
            "F23" => Keysym::F23,
            "F24" => Keysym::F24,
            _ => Keysym::NoSymbol,
        }
    };

    if keysym == Keysym::NoSymbol {
        None
    } else {
        Some(keysym)
    }
}

impl<'de> de::Deserialize<'de> for Key {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct KeyVisitor;

        impl<'de> de::Visitor<'de> for KeyVisitor {
            type Value = Key;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a key or a list of keys")
            }

            fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(Key::from(s.parse::<SingleKey>().map_err(E::custom)?))
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut any_of = Vec::new();
                while let Some(next) = seq.next_element()? {
                    any_of.push(next);
                }
                Ok(Key { any_of })
            }
        }

        deserializer.deserialize_any(KeyVisitor)
    }
}

impl<'de> de::Deserialize<'de> for SingleKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct KeyVisitor;

        impl de::Visitor<'_> for KeyVisitor {
            type Value = SingleKey;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a key")
            }

            fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                s.parse().map_err(E::custom)
            }
        }

        deserializer.deserialize_str(KeyVisitor)
    }
}

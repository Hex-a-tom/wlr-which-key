use serde::Deserialize;
use smithay_client_toolkit::shell::wlr_layer::Anchor;

/// Light wrapper around `Anchor` which also supports the "no anchor" value.
///
/// This type is also requires to derive `Deserialize` for the foreign type.
#[derive(Deserialize, Default, Clone, Copy)]
#[serde(rename_all(deserialize = "kebab-case"))]
pub enum ConfigAnchor {
    #[default]
    Center,
    Top,
    Bottom,
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Convert this anchor into the type expected by `wayrs`.
impl From<ConfigAnchor> for Anchor {
    fn from(value: ConfigAnchor) -> Self {
        match value {
            ConfigAnchor::Center => Anchor::empty(),
            ConfigAnchor::Top => Anchor::TOP,
            ConfigAnchor::Bottom => Anchor::BOTTOM,
            ConfigAnchor::Left => Anchor::LEFT,
            ConfigAnchor::Right => Anchor::RIGHT,
            ConfigAnchor::TopLeft => Anchor::TOP | Anchor::LEFT,
            ConfigAnchor::TopRight => Anchor::TOP | Anchor::RIGHT,
            ConfigAnchor::BottomLeft => Anchor::BOTTOM | Anchor::LEFT,
            ConfigAnchor::BottomRight => Anchor::BOTTOM | Anchor::RIGHT,
        }
    }
}

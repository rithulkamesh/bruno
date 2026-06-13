use bruno_orb::Mood;

/// Window placement for the Bruno orb companion.
#[derive(Debug, Clone, Copy)]
pub enum OrbPosition {
    BottomRight,
    Center,
    Custom { x: i32, y: i32 },
}

/// Desktop shell configuration for the example app.
#[derive(Debug, Clone)]
pub struct OrbConfig {
    pub mood: Mood,
    pub window_size: u32,
    pub screen_margin: u32,
    /// Extra inset from bottom edge (dock area).
    pub margin_bottom: u32,
    /// Extra inset from right edge.
    pub margin_right: u32,
    pub always_on_top: bool,
    /// macOS: show on every Space/desktop.
    pub visible_on_all_spaces: bool,
    pub position: OrbPosition,
    /// Pass mouse clicks through to apps below (macOS).
    pub click_through: bool,
}

impl Default for OrbConfig {
    fn default() -> Self {
        Self {
            mood: Mood::Neutral,
            window_size: 240,
            click_through: true,
            screen_margin: 10,
            margin_bottom: 22,
            margin_right: 6,
            always_on_top: true,
            visible_on_all_spaces: true,
            position: OrbPosition::BottomRight,
        }
    }
}

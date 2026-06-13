use bruno_orb::Mood;

#[derive(Debug, Clone, Copy)]
pub enum OrbPosition {
    BottomRight,
    #[expect(dead_code)]
    Center,
    #[expect(dead_code)]
    Custom { x: i32, y: i32 },
}

#[derive(Debug, Clone)]
pub struct OrbConfig {
    pub window_size: u32,
    pub screen_margin: u32,
    pub margin_bottom: u32,
    pub margin_right: u32,
    pub always_on_top: bool,
    pub visible_on_all_spaces: bool,
    pub position: OrbPosition,
    pub click_through: bool,
}

impl Default for OrbConfig {
    fn default() -> Self {
        Self {
            window_size: 240,
            click_through: false,
            screen_margin: 10,
            margin_bottom: 22,
            margin_right: 6,
            always_on_top: true,
            visible_on_all_spaces: true,
            position: OrbPosition::BottomRight,
        }
    }
}

#[allow(dead_code)]
pub fn default_mood() -> Mood {
    Mood::Neutral
}

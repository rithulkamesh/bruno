use crate::mood::MoodConfig;

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Uniforms {
    pub time: f32,
    pub width: f32,
    pub height: f32,
    pub y_position: f32,
    pub speed: f32,
    pub intensity: f32,
    pub motion: f32,
    pub pulse: f32,
    pub core_color: [f32; 4],
    pub glow_color: [f32; 4],
    pub pointer: [f32; 2],
    pub presence: f32,
    /// 0 idle, 1 loading, 2 happy, 3 rage
    pub anim_style: f32,
}

/// Per-frame values supplied by the host (window size, time, pointer, etc.).
#[derive(Debug, Clone, Copy)]
pub struct FrameParams {
    pub time: f32,
    pub width: f32,
    pub height: f32,
    pub y_position: f32,
    pub pointer: [f32; 2],
    /// 0..1 startup fade-in, exit fade-out, or steady 1.0
    pub presence: f32,
}

impl Uniforms {
    pub fn from_mood(cfg: &MoodConfig, frame: FrameParams) -> Self {
        Self {
            time: frame.time,
            width: frame.width,
            height: frame.height,
            y_position: frame.y_position,
            speed: cfg.speed,
            intensity: cfg.intensity,
            motion: cfg.motion,
            pulse: cfg.pulse,
            core_color: [cfg.core_color[0], cfg.core_color[1], cfg.core_color[2], 0.0],
            glow_color: [cfg.glow_color[0], cfg.glow_color[1], cfg.glow_color[2], 0.0],
            pointer: frame.pointer,
            presence: frame.presence,
            anim_style: cfg.anim_style,
        }
    }
}

impl Default for Uniforms {
    fn default() -> Self {
        Self::from_mood(
            &crate::mood::Mood::Neutral.config(),
            FrameParams {
                time: 0.0,
                width: 400.0,
                height: 400.0,
                y_position: 0.0,
                pointer: [0.0; 2],
                presence: 1.0,
            },
        )
    }
}

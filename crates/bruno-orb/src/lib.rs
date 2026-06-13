//! Bruno orb renderer — mood palettes and GPU drawing for the presence indicator.
//!
//! Windowing and event loops are the host app's responsibility (see `examples/app`).
//!
//! ```no_run
//! use std::sync::Arc;
//! use bruno_orb::{FrameParams, Mood, RendererState, Uniforms};
//! use winit::window::Window;
//!
//! fn frame(renderer: &mut RendererState, window: Arc<Window>, mood: Mood, t: f32) {
//!     let cfg = mood.config();
//!     let u = Uniforms::from_mood(
//!         &cfg,
//!         FrameParams {
//!             time: t,
//!             width: renderer.width() as f32,
//!             height: renderer.height() as f32,
//!             y_position: 0.0,
//!             pointer: [0.0, 0.0],
//!             presence: 1.0,
//!         },
//!     );
//!     renderer.render(u);
//! }
//! ```

pub mod mood;
pub mod renderer;

pub use mood::{Mood, MoodConfig};
pub use renderer::{FrameParams, RendererState, Uniforms};

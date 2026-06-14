//! Winit orb window — GPU renderer + mood blending.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Instant;

use bruno_core::StartupGate;
use bruno_orb::{FrameParams, Mood, MoodConfig, RendererState, Uniforms};
use bruno_voice::{Stt, TtsRuntime};
use crate::hud::HudState;
use crate::platform::{configure_window, global_cursor_physical, window_center_physical};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId, WindowLevel};

use crate::commands::{orb_mood_to_mood, MoodCommand, SharedGeometry};
use crate::orb_config::{OrbConfig, OrbPosition};

const MOOD_BLEND_SECS: f32 = 0.85;
const FLEE_RADIUS: f32 = 200.0;
const FLEE_PUSH: f32 = 150.0;
const FLEE_SPRING_W: f32 = 38.0;
const FLEE_SPRING_ZETA: f32 = 0.72;
const RETURN_SPRING_W: f32 = 20.0;
const RETURN_SPRING_ZETA: f32 = 1.0;
const STARTUP_SECS: f32 = 0.7;
const EXIT_SECS: f32 = 0.5;

enum GpuState {
    Pending(std::sync::Arc<Window>),
    Ready(RendererState),
}

enum LifePhase {
    Starting(f32),
    Running,
    Exiting(f32),
}

pub struct OrbParams {
    pub mood_rx: Receiver<MoodCommand>,
    pub hud_rx: Receiver<crate::commands::HudCommand>,
    pub hud_tx: std::sync::mpsc::Sender<crate::commands::HudCommand>,
    pub geometry: SharedGeometry,
    pub voice_enabled: Arc<AtomicBool>,
    pub wiring_state: std::sync::Arc<std::sync::Mutex<crate::wiring::WiringState>>,
    pub tts_runtime: TtsRuntime,
    pub stt: Arc<Stt>,
    pub capture_rx: std::sync::mpsc::Receiver<
        tokio::sync::oneshot::Sender<Option<bruno_daemon::ScaledScreenshot>>,
    >,
    pub startup: Arc<StartupGate>,
    pub browser_rx: std::sync::mpsc::Receiver<crate::browser::Job>,
}

struct OrbApp {
    config: OrbConfig,
    gpu: Option<GpuState>,
    start: Instant,
    mood: Mood,
    mood_from: MoodConfig,
    mood_to: MoodConfig,
    mood_blend: f32,
    pointer: [f32; 2],
    last_frame: Instant,
    home_pos: Option<PhysicalPosition<i32>>,
    flee_offset: [f32; 2],
    flee_vel: [f32; 2],
    phase: LifePhase,
    mood_rx: Receiver<MoodCommand>,
    hud: HudState,
    geometry: SharedGeometry,
    voice_enabled: Arc<AtomicBool>,
    hud_tx: std::sync::mpsc::Sender<crate::commands::HudCommand>,
    wiring_state: std::sync::Arc<std::sync::Mutex<crate::wiring::WiringState>>,
    tts_runtime: TtsRuntime,
    stt: Arc<Stt>,
    voice_listening: Cell<bool>,
    orb_window_id: Option<WindowId>,
    /// Kept alive to hold the OS hotkey registration. `None` if registration failed.
    _hotkey_manager: Option<GlobalHotKeyManager>,
    hotkey_id: u32,
    capture_rx: std::sync::mpsc::Receiver<
        tokio::sync::oneshot::Sender<Option<bruno_daemon::ScaledScreenshot>>,
    >,
    capture_state: bruno_daemon::CapturePollState,
    startup: Arc<StartupGate>,
    ui_marked: bool,
    services_ready: bool,
    auto_voice_armed: bool,
    #[allow(dead_code)]
    browser_rx: Option<std::sync::mpsc::Receiver<crate::browser::Job>>,
    #[cfg(target_os = "macos")]
    browser: Option<crate::browser::Driver>,
}

impl OrbApp {
    fn new(params: OrbParams) -> Self {
        let mood = Mood::Thinking;
        let cfg = mood.config();

        // Global ⌘⇧B to toggle listening from anywhere (the orb is an accessory
        // window, so a winit key event would only arrive while it's focused).
        let (hotkey_manager, hotkey_id) = match GlobalHotKeyManager::new() {
            Ok(manager) => {
                let hotkey = HotKey::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyB);
                let id = hotkey.id();
                match manager.register(hotkey) {
                    Ok(()) => {
                        tracing::info!("global hotkey ⌘⇧B registered (toggle voice)");
                        (Some(manager), id)
                    }
                    Err(e) => {
                        tracing::warn!("failed to register global hotkey ⌘⇧B: {e}");
                        (None, 0)
                    }
                }
            }
            Err(e) => {
                tracing::warn!("global hotkey manager unavailable: {e}");
                (None, 0)
            }
        };

        Self {
            config: OrbConfig::default(),
            gpu: None,
            start: Instant::now(),
            mood,
            mood_from: cfg,
            mood_to: cfg,
            mood_blend: 1.0,
            pointer: [0.0; 2],
            last_frame: Instant::now(),
            home_pos: None,
            flee_offset: [0.0; 2],
            flee_vel: [0.0; 2],
            phase: LifePhase::Starting(0.0),
            mood_rx: params.mood_rx,
            hud: HudState::new(params.hud_rx),
            hud_tx: params.hud_tx,
            geometry: params.geometry,
            voice_enabled: params.voice_enabled,
            wiring_state: params.wiring_state,
            tts_runtime: params.tts_runtime,
            stt: params.stt,
            voice_listening: Cell::new(false),
            orb_window_id: None,
            _hotkey_manager: hotkey_manager,
            hotkey_id,
            capture_rx: params.capture_rx,
            capture_state: bruno_daemon::CapturePollState::new(),
            startup: params.startup,
            ui_marked: false,
            services_ready: false,
            auto_voice_armed: std::env::var("BRUNO_AUTO_VOICE").is_ok(),
            browser_rx: Some(params.browser_rx),
            #[cfg(target_os = "macos")]
            browser: None,
        }
    }

    fn set_mood(&mut self, mood: Mood) {
        if mood == self.mood && self.mood_blend >= 1.0 {
            return;
        }
        self.mood_from = MoodConfig::lerp(self.mood_from, self.mood_to, self.mood_blend);
        self.mood_to = mood.config();
        self.mood = mood;
        self.mood_blend = 0.0;
    }

    fn poll_commands(&mut self) {
        if !self.startup.is_ready() {
            return;
        }
        while let Ok(cmd) = self.mood_rx.try_recv() {
            let MoodCommand::Set(orb_mood) = cmd;
            self.set_mood(orb_mood_to_mood(orb_mood));
        }
    }

    fn tick_startup_mood(&mut self) {
        if self.services_ready {
            return;
        }
        if self.startup.is_ready() {
            self.services_ready = true;
            self.set_mood(Mood::Neutral);
        }
    }

    fn update_geometry(&self, window: &Window) {
        if let Ok(mut geo) = self.geometry.lock() {
            if let Ok(pos) = window.outer_position() {
                geo.x = pos.x;
                geo.y = pos.y;
            }
            let size = window.outer_size();
            geo.width = size.width;
            geo.height = size.height;
        }
        if self.hud.visible {
            self.hud.position(&self.geometry);
        }
    }

    fn ease_out_cubic(x: f32) -> f32 {
        let t = 1.0 - x;
        1.0 - t * t * t
    }

    fn ease_in_cubic(x: f32) -> f32 {
        x * x * x
    }

    fn presence(&self) -> f32 {
        match self.phase {
            LifePhase::Starting(t) => Self::ease_out_cubic((t / STARTUP_SECS).min(1.0)),
            LifePhase::Running => 1.0,
            LifePhase::Exiting(t) => 1.0 - Self::ease_in_cubic((t / EXIT_SECS).min(1.0)),
        }
    }

    fn tick_lifecycle(&mut self, frame_dt: f32, event_loop: &ActiveEventLoop) {
        match &mut self.phase {
            LifePhase::Starting(t) => {
                *t += frame_dt;
                if *t >= STARTUP_SECS {
                    self.phase = LifePhase::Running;
                }
            }
            LifePhase::Running => {}
            LifePhase::Exiting(t) => {
                *t += frame_dt;
                if *t >= EXIT_SECS {
                    event_loop.exit();
                }
            }
        }
    }

    fn request_exit(&mut self) {
        if !matches!(self.phase, LifePhase::Exiting(_)) {
            self.phase = LifePhase::Exiting(0.0);
        }
    }

    fn outer_pos(window: &Window) -> PhysicalPosition<i32> {
        window.outer_position().unwrap_or(PhysicalPosition::new(0, 0))
    }

    fn place_window(window: &Window, config: &OrbConfig) -> PhysicalPosition<i32> {
        let pos = match config.position {
            OrbPosition::Custom { x, y } => PhysicalPosition::new(x, y),
            OrbPosition::Center => {
                let monitor = window.current_monitor().or_else(|| window.primary_monitor());
                let Some(monitor) = monitor else {
                    return Self::outer_pos(window);
                };
                let screen = monitor.size();
                let win = window.outer_size();
                let x = (screen.width.saturating_sub(win.width)) / 2;
                let y = (screen.height.saturating_sub(win.height)) / 2;
                PhysicalPosition::new(x as i32, y as i32)
            }
            OrbPosition::BottomRight => {
                let monitor = window.current_monitor().or_else(|| window.primary_monitor());
                let Some(monitor) = monitor else {
                    return Self::outer_pos(window);
                };
                let screen = monitor.size();
                let win = window.outer_size();
                let x = screen
                    .width
                    .saturating_sub(win.width)
                    .saturating_sub(config.screen_margin)
                    .saturating_sub(config.margin_right) as i32;
                let y = screen
                    .height
                    .saturating_sub(win.height)
                    .saturating_sub(config.screen_margin)
                    .saturating_sub(config.margin_bottom) as i32;
                PhysicalPosition::new(x, y)
            }
        };
        let _ = window.set_outer_position(pos);
        pos
    }

    fn pointer_from_screen(window: &Window) -> [f32; 2] {
        let scale = window.scale_factor();
        let Some((cx, cy)) = global_cursor_physical(scale) else {
            return [0.0; 2];
        };
        let outer = Self::outer_pos(window);
        let size = window.outer_size();
        let w = size.width.max(1) as f32;
        let h = size.height.max(1) as f32;
        let lx = ((cx - outer.x as f32) / w).clamp(0.0, 1.0);
        let ly = ((cy - outer.y as f32) / h).clamp(0.0, 1.0);
        [lx * 2.0 - 1.0, 1.0 - ly * 2.0]
    }

    fn spring_step(pos: &mut f32, vel: &mut f32, goal: f32, dt: f32, omega: f32, zeta: f32) {
        let error = goal - *pos;
        let accel = omega * omega * error - 2.0 * zeta * omega * *vel;
        *vel += accel * dt;
        *pos += *vel * dt;
        if error.abs() < 0.4 && vel.abs() < 0.6 {
            *pos = goal;
            *vel = 0.0;
        }
    }

    fn update_flee(&mut self, frame_dt: f32) {
        let Some(home) = self.home_pos else { return };
        let window = match &self.gpu {
            Some(GpuState::Ready(s)) => s.window(),
            Some(GpuState::Pending(w)) => w.as_ref(),
            None => return,
        };

        let dt = frame_dt.min(0.032);
        let scale = window.scale_factor();
        let (mut target_x, mut target_y) = (0.0f32, 0.0f32);

        if let (Some((cx, cy)), Some((ox, oy))) = (
            global_cursor_physical(scale),
            window_center_physical(window),
        ) {
            let dx = cx - ox;
            let dy = cy - oy;
            let dist = (dx * dx + dy * dy).sqrt();

            if dist < FLEE_RADIUS && dist > 2.0 {
                let push = ((FLEE_RADIUS - dist) / FLEE_RADIUS).powf(1.15);
                let nx = -dx / dist;
                let ny = -dy / dist;
                target_x = nx * FLEE_PUSH * push;
                target_y = ny * FLEE_PUSH * push;
            }
        }

        let err_x = target_x - self.flee_offset[0];
        let err_y = target_y - self.flee_offset[1];
        let err_len = (err_x * err_x + err_y * err_y).sqrt();
        let target_active = target_x * target_x + target_y * target_y > 1.0;

        let (omega, zeta) = if target_active || err_len > 6.0 {
            (FLEE_SPRING_W, FLEE_SPRING_ZETA)
        } else {
            (RETURN_SPRING_W, RETURN_SPRING_ZETA)
        };

        Self::spring_step(
            &mut self.flee_offset[0],
            &mut self.flee_vel[0],
            target_x,
            dt,
            omega,
            zeta,
        );
        Self::spring_step(
            &mut self.flee_offset[1],
            &mut self.flee_vel[1],
            target_y,
            dt,
            omega,
            zeta,
        );

        let _ = window.set_outer_position(PhysicalPosition::new(
            home.x + self.flee_offset[0].round() as i32,
            home.y + self.flee_offset[1].round() as i32,
        ));
    }

    fn active_mood(&self) -> MoodConfig {
        MoodConfig::lerp(self.mood_from, self.mood_to, self.mood_blend)
    }

    fn handle_hotkey(&self) {
        let on = !self.voice_enabled.load(Ordering::Relaxed);
        crate::wiring::handle_hotkey(
            &self.hud_tx,
            &self.voice_enabled,
            &self.wiring_state,
            on,
        );
    }


    fn ensure_gpu(&mut self, _event_loop: &ActiveEventLoop) {
        if matches!(self.gpu, Some(GpuState::Ready(_))) {
            return;
        }
        if let Some(GpuState::Pending(window)) = self.gpu.take() {
            self.gpu = Some(GpuState::Ready(RendererState::new(window)));
        }
    }
}

impl ApplicationHandler for OrbApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let size = self.config.window_size;
        let mut attrs = Window::default_attributes()
            .with_title("Bruno")
            .with_inner_size(winit::dpi::LogicalSize::new(size, size))
            .with_transparent(true)
            .with_decorations(false);

        if self.config.always_on_top {
            attrs = attrs.with_window_level(WindowLevel::AlwaysOnTop);
        }

        let window = std::sync::Arc::new(event_loop.create_window(attrs).unwrap());

        configure_window(
            &window,
            self.config.visible_on_all_spaces,
            self.config.always_on_top,
            self.config.click_through,
        );
        self.home_pos = Some(Self::place_window(&window, &self.config));
        self.update_geometry(&window);

        self.hud.ensure_overlay(&window);

        self.gpu = Some(GpuState::Pending(window.clone()));
        self.orb_window_id = Some(window.id());
        window.request_redraw();
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if self.startup.is_ready() {
            bruno_daemon::poll_capture_requests(&self.capture_rx, &mut self.capture_state);
        }
        self.tts_runtime.poll();
        self.hud.poll_commands(&self.geometry);

        // Headless browser for the agent (main-thread WKWebView).
        #[cfg(target_os = "macos")]
        {
            if self.browser.is_none() {
                if let (Some(rx), Some(mtm)) =
                    (self.browser_rx.take(), objc2::MainThreadMarker::new())
                {
                    self.browser = Some(crate::browser::Driver::new(rx, mtm));
                }
            }
            if let Some(browser) = self.browser.as_mut() {
                browser.pump();
            }
        }

        // Global hotkey (⌘⇧B) — fires regardless of which app is focused.
        if self._hotkey_manager.is_some() {
            while let Ok(ev) = GlobalHotKeyEvent::receiver().try_recv() {
                if ev.id == self.hotkey_id && ev.state == HotKeyState::Pressed {
                    self.handle_hotkey();
                }
            }
        }

        if self.auto_voice_armed && self.startup.is_ready() && self.start.elapsed().as_secs() >= 2 {
            self.auto_voice_armed = false;
            tracing::info!("dev: auto-enabling voice (BRUNO_AUTO_VOICE)");
            self.voice_enabled.store(true, Ordering::Relaxed);
        }

        let listen = self.voice_enabled.load(Ordering::Relaxed);
        if listen != self.voice_listening.get() {
            self.stt.set_listening(listen);
            self.voice_listening.set(listen);
        }
        self.stt.poll_main();
        if self.voice_listening.get() && !self.stt.wants_listening() {
            self.voice_enabled.store(false, Ordering::Relaxed);
            self.voice_listening.set(false);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self.orb_window_id != Some(id) {
            return;
        }

        match event {
            WindowEvent::CloseRequested => self.request_exit(),

            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if self.startup.is_ready() {
                    self.hud.toggle(&self.geometry);
                }
                let on = self.voice_enabled.load(Ordering::Relaxed);
                self.voice_enabled.store(!on, Ordering::Relaxed);
            }

            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed =>
            {
                if let Key::Named(NamedKey::Escape) = event.logical_key {
                    self.hud.hide(&self.geometry);
                    self.voice_enabled.store(false, Ordering::Relaxed);
                }
            }

            WindowEvent::Resized(size) => {
                if let Some(GpuState::Ready(s)) = &mut self.gpu {
                    s.resize(size.width, size.height);
                    self.hud.on_orb_resized(s.window());
                } else if let Some(GpuState::Pending(w)) = &self.gpu {
                    self.hud.on_orb_resized(w);
                }
            }

            WindowEvent::Moved(_) => {
                if let Some(GpuState::Ready(s)) = &self.gpu {
                    self.update_geometry(s.window());
                } else if let Some(GpuState::Pending(w)) = &self.gpu {
                    self.update_geometry(w);
                }
            }

            WindowEvent::RedrawRequested => {
                self.poll_commands();
                self.tick_startup_mood();
                self.ensure_gpu(event_loop);

                let now = Instant::now();
                let frame_dt = (now - self.last_frame).as_secs_f32().clamp(0.001, 0.05);
                self.last_frame = now;

                self.tick_lifecycle(frame_dt, event_loop);
                self.update_flee(frame_dt);

                if self.mood_blend < 1.0 {
                    self.mood_blend = (self.mood_blend + frame_dt / MOOD_BLEND_SECS).min(1.0);
                }

                let dt = self.start.elapsed().as_secs_f32();
                let cfg = self.active_mood();
                let presence = self.presence();
                let y = cfg.motion
                    * (0.044 * (dt * 0.24).sin()
                        + 0.020 * (dt * 0.41).cos()
                        + 0.010 * (dt * 0.67).sin());

                if let Some(GpuState::Ready(s)) = &self.gpu {
                    let window = s.window();
                    let width = s.width() as f32;
                    let height = s.height() as f32;
                    self.pointer = Self::pointer_from_screen(&window);
                    self.update_geometry(&window);
                    if !self.ui_marked {
                        self.startup.mark_ui_ready();
                        self.ui_marked = true;
                    }
                    if let Some(GpuState::Ready(s)) = &mut self.gpu {
                        let uniforms = Uniforms::from_mood(
                            &cfg,
                            FrameParams {
                                time: dt,
                                width,
                                height,
                                y_position: y,
                                pointer: self.pointer,
                                presence,
                            },
                        );
                        s.render(uniforms);
                        s.window().request_redraw();
                    }
                } else if let Some(GpuState::Pending(w)) = &self.gpu {
                    w.request_redraw();
                }
            }

            _ => {}
        }
    }
}

pub fn run(params: OrbParams) {
    #[cfg(target_os = "macos")]
    let event_loop = {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        EventLoop::builder()
            .with_activation_policy(ActivationPolicy::Accessory)
            .build()
            .unwrap()
    };

    #[cfg(not(target_os = "macos"))]
    let event_loop = EventLoop::new().unwrap();

    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = OrbApp::new(params);
    event_loop.run_app(&mut app).unwrap();
}

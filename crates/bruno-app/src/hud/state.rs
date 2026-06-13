use std::sync::mpsc::Receiver;

use winit::window::Window;

use crate::commands::{HudCommand, SharedGeometry};

const LABEL_W: f64 = 224.0;
const LABEL_H: f64 = 44.0;
const LABEL_PAD: f64 = 8.0;

#[cfg(target_os = "macos")]
mod native {
    use super::{LABEL_H, LABEL_PAD, LABEL_W};
    use objc2::MainThreadMarker;
    
    use objc2::rc::Retained;
    use objc2_app_kit::{NSColor, NSFont, NSTextField, NSView, NSWindow};
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

    pub struct HudOverlay {
        label: Retained<NSTextField>,
    }

    impl HudOverlay {
        pub fn attach(window: &NSWindow) -> Self {
            let mtm = MainThreadMarker::new().expect("HUD must be created on the main thread");
            let content = window
                .contentView()
                .expect("orb window must have a content view");
            let bounds = content.bounds();
            let frame = NSRect::new(
                NSPoint::new(
                    (bounds.size.width - LABEL_W) * 0.5,
                    bounds.size.height - LABEL_H - LABEL_PAD,
                ),
                NSSize::new(LABEL_W, LABEL_H),
            );

            let label = NSTextField::new(mtm);
            label.setFrame(frame);
            label.setEditable(false);
            label.setSelectable(false);
            label.setBezeled(false);
            label.setDrawsBackground(true);
            label.setBackgroundColor(Some(&NSColor::colorWithCalibratedWhite_alpha(
                0.08, 0.92,
            )));
            label.setTextColor(Some(&NSColor::colorWithCalibratedRed_green_blue_alpha(
                0.89, 0.85, 0.95, 1.0,
            )));
            label.setFont(Some(&NSFont::systemFontOfSize(13.0)));
            label.setStringValue(&NSString::from_str(""));
            label.setHidden(true);
            content.addSubview(&label);
            Self { label }
        }

        pub fn relayout(&self, content: &NSView) {
            let bounds = content.bounds();
            let frame = NSRect::new(
                NSPoint::new(
                    (bounds.size.width - LABEL_W) * 0.5,
                    bounds.size.height - LABEL_H - LABEL_PAD,
                ),
                NSSize::new(LABEL_W, LABEL_H),
            );
            self.label.setFrame(frame);
        }

        pub fn set_text(&self, text: &str) {
            self.label.setStringValue(&NSString::from_str(text));
        }

        pub fn show(&self) {
            self.label.setHidden(false);
        }

        pub fn hide(&self) {
            self.label.setHidden(true);
        }
    }
}

pub struct HudState {
    #[cfg(target_os = "macos")]
    overlay: Option<native::HudOverlay>,
    pub visible: bool,
    rx: Receiver<HudCommand>,
}

impl HudState {
    pub fn new(rx: Receiver<HudCommand>) -> Self {
        Self {
            #[cfg(target_os = "macos")]
            overlay: None,
            visible: false,
            rx,
        }
    }

    #[allow(dead_code)]
    pub fn is_ready(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.overlay.is_some()
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }

    /// Attach a native label to the orb window (no extra AppKit/winit windows).
    pub fn ensure_overlay(&mut self, orb: &Window) {
        #[cfg(target_os = "macos")]
        {
            if self.overlay.is_some() {
                return;
            }
            let Some(ns_window) = crate::platform::ns_window_from(orb) else {
                tracing::warn!("hud: could not get NSWindow from orb");
                return;
            };
            tracing::info!("hud: attaching overlay");
            self.overlay = Some(native::HudOverlay::attach(&ns_window));
            if self.visible {
                if let Some(overlay) = &self.overlay {
                    overlay.show();
                }
            }
            tracing::info!("hud: overlay ready");
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = orb;
        }
    }

    pub fn on_orb_resized(&self, orb: &Window) {
        #[cfg(target_os = "macos")]
        if let (Some(overlay), Some(ns_window)) =
            (&self.overlay, crate::platform::ns_window_from(orb))
        {
            if let Some(content) = ns_window.contentView() {
                overlay.relayout(&content);
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = orb;
    }

    pub fn toggle(&mut self, geometry: &SharedGeometry) {
        if self.visible {
            self.apply(HudCommand::Hide, geometry);
        } else {
            self.apply(HudCommand::Show, geometry);
        }
    }

    pub fn hide(&mut self, geometry: &SharedGeometry) {
        self.apply(HudCommand::Hide, geometry);
    }

    pub fn poll_commands(&mut self, geometry: &SharedGeometry) {
        while let Ok(cmd) = self.rx.try_recv() {
            self.apply(cmd, geometry);
        }
    }

    fn apply(&mut self, cmd: HudCommand, geometry: &SharedGeometry) {
        match cmd {
            HudCommand::Show => {
                self.visible = true;
                self.position(geometry);
                #[cfg(target_os = "macos")]
                if let Some(overlay) = &self.overlay {
                    overlay.show();
                }
            }
            HudCommand::Hide => {
                self.visible = false;
                #[cfg(target_os = "macos")]
                if let Some(overlay) = &self.overlay {
                    overlay.hide();
                }
            }
            HudCommand::Toggle => {
                if self.visible {
                    self.apply(HudCommand::Hide, geometry);
                } else {
                    self.apply(HudCommand::Show, geometry);
                }
            }
            HudCommand::SetText(text) => {
                #[cfg(target_os = "macos")]
                if let Some(overlay) = &self.overlay {
                    overlay.set_text(&text);
                }
                #[cfg(not(target_os = "macos"))]
                let _ = text;
            }
            HudCommand::SetPulsing(_p) => {}
            HudCommand::ShowInput(_show) => {}
        }
    }

    pub fn position(&self, _geometry: &SharedGeometry) {
        // Overlay moves with the orb window; no separate screen positioning.
    }
}

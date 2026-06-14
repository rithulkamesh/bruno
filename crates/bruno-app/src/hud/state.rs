use std::sync::mpsc::Receiver;

use winit::window::Window;

use crate::commands::{HudCommand, SharedGeometry};

/// Preferred text column width; clamped to the window if narrower.
const TEXT_W: f64 = 360.0;
/// Horizontal / vertical padding inside the glass panel.
const PAD_X: f64 = 20.0;
const PAD_Y: f64 = 14.0;
/// Gap from the top edge of the content view.
const TOP_PAD: f64 = 10.0;
/// Side margin so the panel never touches the window edges.
const SIDE_MARGIN: f64 = 16.0;
const CORNER_RADIUS: f64 = 20.0;
const FONT_SIZE: f64 = 15.0;

#[cfg(target_os = "macos")]
mod native {
    use super::{CORNER_RADIUS, FONT_SIZE, PAD_X, PAD_Y, SIDE_MARGIN, TEXT_W, TOP_PAD};
    use objc2::MainThreadMarker;

    use objc2::rc::Retained;
    use objc2_app_kit::{
        NSColor, NSFont, NSTextAlignment, NSTextField, NSView, NSVisualEffectBlendingMode,
        NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindow,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

    /// A frosted-glass HUD panel (NSVisualEffectView) with a wrapping label.
    pub struct HudOverlay {
        content: Retained<NSView>,
        panel: Retained<NSVisualEffectView>,
        label: Retained<NSTextField>,
    }

    impl HudOverlay {
        pub fn attach(window: &NSWindow) -> Self {
            let mtm = MainThreadMarker::new().expect("HUD must be created on the main thread");
            let content = window
                .contentView()
                .expect("orb window must have a content view");

            // Frosted glass background that blurs the desktop behind the window.
            let panel = NSVisualEffectView::new(mtm);
            panel.setMaterial(NSVisualEffectMaterial::HUDWindow);
            panel.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
            panel.setState(NSVisualEffectState::Active);
            panel.setEmphasized(true);
            panel.setWantsLayer(true);
            if let Some(layer) = panel.layer() {
                layer.setCornerRadius(CORNER_RADIUS);
                layer.setMasksToBounds(true);
            }
            panel.setHidden(true);

            // Wrapping, multi-line label.
            let label = NSTextField::labelWithString(&NSString::from_str(""), mtm);
            label.setEditable(false);
            label.setSelectable(false);
            label.setBezeled(false);
            label.setDrawsBackground(false);
            label.setUsesSingleLineMode(false);
            label.setMaximumNumberOfLines(0);
            label.setAlignment(NSTextAlignment::Center);
            label.setTextColor(Some(&NSColor::colorWithCalibratedWhite_alpha(0.96, 1.0)));
            label.setFont(Some(&NSFont::systemFontOfSize(FONT_SIZE)));

            panel.addSubview(&label);
            content.addSubview(&panel);

            let overlay = Self {
                content,
                panel,
                label,
            };
            overlay.relayout();
            overlay
        }

        /// Recompute panel + label frames to fit the current text, bottom-anchored
        /// near the top of the content view (matching the orb HUD position).
        fn relayout(&self) {
            let bounds = self.content.bounds();
            let text_w = TEXT_W.min(bounds.size.width - 2.0 * (SIDE_MARGIN + PAD_X));
            let text_w = text_w.max(120.0);

            self.label.setPreferredMaxLayoutWidth(text_w);
            let fit = self.label.fittingSize();
            let text_h = fit.height.max(FONT_SIZE * 1.4);

            let panel_w = text_w + 2.0 * PAD_X;
            let panel_h = text_h + 2.0 * PAD_Y;
            let panel_x = (bounds.size.width - panel_w) * 0.5;
            let panel_y = bounds.size.height - panel_h - TOP_PAD;

            self.panel.setFrame(NSRect::new(
                NSPoint::new(panel_x, panel_y),
                NSSize::new(panel_w, panel_h),
            ));
            self.label.setFrame(NSRect::new(
                NSPoint::new(PAD_X, PAD_Y),
                NSSize::new(text_w, text_h),
            ));
        }

        pub fn relayout_in(&self, _content: &NSView) {
            self.relayout();
        }

        pub fn set_text(&self, text: &str) {
            self.label.setStringValue(&NSString::from_str(text));
            self.relayout();
        }

        pub fn show(&self) {
            self.panel.setHidden(false);
        }

        pub fn hide(&self) {
            self.panel.setHidden(true);
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
                overlay.relayout_in(&content);
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

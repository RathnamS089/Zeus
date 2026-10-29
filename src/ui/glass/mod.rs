//! Frosted glass under a vibrant header: what egui has already drawn beneath
//! a rectangle is blurred in place, as macOS blurs content scrolling under a
//! toolbar. egui has no blur of its own, so this is an OpenGL paint callback
//! that runs just before the header is drawn over it.
//!
//! The region is copied twice: sharp at full size, and at half of it,
//! where separable Gaussian passes blur it lightly, as macOS does: the
//! letters under a toolbar stay faintly legible. The last pass
//! writes it back, blurred at the top and easing to sharp over the bottom
//! `fade`, so the messages do not stop at a line. Callers keep it within
//! the header, as macOS does.
//!
//! The chat list's header frosts progressively ([`progressive`]): deeply
//! behind the title, search and filters, so nothing under them can be read,
//! then more lightly through the tail below, then sharp.
//!
//! Menus frost more deeply ([`rounded`]): a smaller copy, blurred more
//! times, written back only within the menu's rounded corners. The work is
//! done on frames that paint, and egui paints only when something changes,
//! so an idle window spends nothing on it.
//!
//! The work is split by what it touches: [`renderer`] runs one frost,
//! [`targets`] holds the off-screen textures it copies into, and
//! [`shaders`] compiles the programs that blur and compose them.

mod renderer;
mod shaders;
mod targets;

use std::sync::{Arc, Mutex, Weak};

use eframe::egui_glow;
use eframe::glow;

use renderer::Glass;

/// How lightly a bar frosts: its copy is half the size, blurred once, so
/// what scrolls under it stays faintly legible.
const BAR: Strength = Strength {
    downscale: 2,
    passes: 1,
    progressive: false,
};
/// How deeply a menu frosts: a quarter-size copy blurred three times, so
/// only the colours of what lies under it come through, as iOS's do.
const MENU: Strength = Strength {
    downscale: 4,
    passes: 3,
    progressive: false,
};
/// How the chat list's header frosts: as deeply as a menu behind the
/// header, easing through the copy after one pass to sharp below it.
const PROGRESSIVE: Strength = Strength {
    downscale: 4,
    passes: 3,
    progressive: true,
};

#[derive(Clone, Copy)]
pub(super) struct Strength {
    /// How much smaller the blurred copy is than the region.
    pub(super) downscale: i32,
    /// Horizontal and vertical passes over the small copy. Each widens the blur.
    pub(super) passes: usize,
    /// Whether the copy after the first pass is kept, for the lighter blur
    /// a progressive frost eases through on its way to sharp.
    pub(super) progressive: bool,
}

/// A paint callback that frosts `rect`, the bottom `fade` points easing from
/// blurred to sharp (the top ones, for a negative `fade`), within corners
/// rounded by `radius` points, the card's it lies across.
pub fn shape(rect: egui::Rect, fade: f32, radius: f32) -> egui::Shape {
    frost(rect, fade, radius, BAR)
}

/// A paint callback that frosts `rect` progressively: as deeply as a menu
/// down to the bottom `fade` points, then through a lighter blur to sharp.
pub fn progressive(rect: egui::Rect, fade: f32) -> egui::Shape {
    frost(rect, fade, 0.0, PROGRESSIVE)
}

/// A paint callback that frosts `rect` deeply, as under a menu, within
/// corners rounded by `radius` points.
pub fn rounded(rect: egui::Rect, radius: f32) -> egui::Shape {
    frost(rect, 0.0, radius, MENU)
}

fn frost(rect: egui::Rect, fade: f32, radius: f32, strength: Strength) -> egui::Shape {
    // Settings' Blur off: nothing is frosted, and what floats above is solid.
    if !crate::backdrop::blur() {
        return egui::Shape::Noop;
    }
    egui::Shape::Callback(egui::PaintCallback {
        rect,
        callback: Arc::new(egui_glow::CallbackFn::new(move |info, painter| {
            let fade = (fade * info.pixels_per_point).round() as i32;
            let radius = radius * info.pixels_per_point;
            let gl = painter.gl();
            let mut glass = GLASS.lock().unwrap_or_else(|p| p.into_inner());
            // A window made anew has a new context; the old one took its
            // objects with it. A context that cannot frost is asked once.
            if glass
                .as_ref()
                .is_none_or(|(made_for, _)| !made_for_this(made_for, gl))
            {
                // SAFETY: called on the painting thread with its context current.
                *glass = Some((Arc::downgrade(gl), unsafe { Glass::new(gl) }));
            }
            if let Some((_, Some(glass))) = glass.as_mut() {
                // SAFETY: as above.
                unsafe { glass.frost(gl, &info, fade, radius, strength) };
            }
        })),
    })
}

/// The objects for the current context, keyed by it, or nothing where it
/// cannot run them.
static GLASS: Mutex<Option<(Weak<glow::Context>, Option<Glass>)>> = Mutex::new(None);

/// Whether the objects made for `made_for` belong to `context`. The weak
/// reference keeps the old context's allocation, so a context made after it
/// closes cannot take its address. A bare address often went to the next
/// window's context, and the stale object names then drew over that
/// window's own textures, garbling it after a close and reopen.
fn made_for_this<T>(made_for: &Weak<T>, context: &Arc<T>) -> bool {
    std::ptr::eq(made_for.as_ptr(), Arc::as_ptr(context))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A context made after the old one closes is never taken for it, even
    /// where the allocator would hand the new one the old address.
    #[test]
    fn a_reopened_window_gets_new_objects() {
        let first = Arc::new([0u8; 64]);
        let made_for = Arc::downgrade(&first);
        assert!(made_for_this(&made_for, &first));
        drop(first);
        for _ in 0..64 {
            let next = Arc::new([0u8; 64]);
            assert!(!made_for_this(&made_for, &next));
        }
    }
}

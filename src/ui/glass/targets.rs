//! The off-screen textures a frost copies into and blurs between.

use eframe::glow::{self, HasContext};

/// One full-size and three small render targets: two to blur between, and
/// the lighter blur a progressive frost keeps.
pub(super) struct Targets {
    pub(super) sharp: Target,
    pub(super) small: [Target; 3],
    pub(super) used: u64,
}

pub(super) struct Target {
    pub(super) texture: glow::Texture,
    pub(super) framebuffer: glow::Framebuffer,
    pub(super) size: (i32, i32),
}

impl Target {
    pub(super) unsafe fn new(gl: &glow::Context) -> Option<Self> {
        unsafe {
            let texture = gl.create_texture().ok()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            for (parameter, value) in [
                (glow::TEXTURE_MIN_FILTER, glow::LINEAR),
                (glow::TEXTURE_MAG_FILTER, glow::LINEAR),
                (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE),
                (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
            ] {
                gl.tex_parameter_i32(glow::TEXTURE_2D, parameter, value as i32);
            }
            gl.bind_texture(glow::TEXTURE_2D, None);
            let framebuffer = gl.create_framebuffer().ok()?;
            Some(Self {
                texture,
                framebuffer,
                size: (0, 0),
            })
        }
    }

    /// Gives the texture `size` pixels, when it has another size.
    pub(super) unsafe fn resize(&mut self, gl: &glow::Context, size: (i32, i32)) {
        if self.size == size {
            return;
        }
        self.size = size;
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                size.0,
                size.1,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(self.framebuffer));
            gl.framebuffer_texture_2d(
                glow::DRAW_FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(self.texture),
                0,
            );
            gl.bind_texture(glow::TEXTURE_2D, None);
        }
    }
}

//! One frost: the copy of what lies under the region, its blur, and the
//! composite written back over it.

use eframe::egui_glow;
use eframe::glow::{self, HasContext};

use super::Strength;
use super::shaders::{self, BLUR, COMPOSE};
use super::targets::{Target, Targets};

/// Sets of render targets kept, one for each size frosted in a frame (the
/// header, the composer, a menu), so none is reallocated every frame.
const TARGET_SETS: usize = 4;
/// How much more saturated the blur is than what it covers, as glass that
/// gathers the colours behind it.
const SATURATION: f32 = 1.3;

/// The two programs, and the render targets for each size frosted lately.
pub(super) struct Glass {
    blur: glow::Program,
    compose: glow::Program,
    vao: glow::VertexArray,
    sets: Vec<Targets>,
    /// Counts frosts, to find the set used least lately.
    clock: u64,
}

impl Glass {
    /// The programs and an empty vertex array, or nothing where the context
    /// cannot run them; the header then goes without the blur.
    pub(super) unsafe fn new(gl: &glow::Context) -> Option<Self> {
        let version = egui_glow::ShaderVersion::get(gl);
        if !version.is_new_shader_interface() {
            log::info!("frosted header unavailable: {version:?} shaders");
            return None;
        }
        let header = version.version_declaration();
        unsafe {
            let blur = shaders::program(gl, header, BLUR)?;
            let compose = shaders::program(gl, header, COMPOSE)?;
            let vao = gl.create_vertex_array().ok()?;
            Some(Self {
                blur,
                compose,
                vao,
                sets: Vec::new(),
                clock: 0,
            })
        }
    }

    /// The targets already `size` pixels, else the ones used least lately,
    /// or new ones while there are fewer than [`TARGET_SETS`].
    unsafe fn targets(&mut self, gl: &glow::Context, size: (i32, i32)) -> Option<&mut Targets> {
        self.clock += 1;
        let at = match self.sets.iter().position(|set| set.sharp.size == size) {
            Some(at) => at,
            None if self.sets.len() < TARGET_SETS => {
                // SAFETY: called on the painting thread with its context current.
                let set = unsafe {
                    Targets {
                        sharp: Target::new(gl)?,
                        small: [Target::new(gl)?, Target::new(gl)?, Target::new(gl)?],
                        used: 0,
                    }
                };
                self.sets.push(set);
                self.sets.len() - 1
            }
            None => (0..self.sets.len()).min_by_key(|&at| self.sets[at].used)?,
        };
        let set = &mut self.sets[at];
        set.used = self.clock;
        Some(set)
    }

    /// Blurs the region of the bound framebuffer under `info`'s rectangle.
    pub(super) unsafe fn frost(
        &mut self,
        gl: &glow::Context,
        info: &egui::PaintCallbackInfo,
        fade: i32,
        radius: f32,
        strength: Strength,
    ) {
        let region = info.viewport_in_pixels();
        let clip = info.clip_rect_in_pixels();
        let (x, y, w, h) = (
            region.left_px,
            region.from_bottom_px,
            region.width_px,
            region.height_px,
        );
        if w <= 0 || h <= 0 {
            return;
        }
        let small = (
            (w / strength.downscale).max(1),
            (h / strength.downscale).max(1),
        );
        let (blur, compose, vao) = (self.blur, self.compose, self.vao);
        unsafe {
            let Some(set) = self.targets(gl, (w, h)) else {
                return;
            };
            let screen = gl.get_parameter_framebuffer(glow::DRAW_FRAMEBUFFER_BINDING);
            set.sharp.resize(gl, (w, h));
            let kept = if strength.progressive { 3 } else { 2 };
            for target in &mut set.small[..kept] {
                target.resize(gl, small);
            }
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::BLEND);
            // The sharp copy, and the small one to blur.
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, screen);
            for (target, filter) in [(&set.sharp, glow::NEAREST), (&set.small[0], glow::LINEAR)] {
                gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(target.framebuffer));
                gl.blit_framebuffer(
                    x,
                    y,
                    x + w,
                    y + h,
                    0,
                    0,
                    target.size.0,
                    target.size.1,
                    glow::COLOR_BUFFER_BIT,
                    filter,
                );
            }
            gl.bind_vertex_array(Some(vao));
            gl.use_program(Some(blur));
            gl.active_texture(glow::TEXTURE0);
            gl.uniform_1_i32(gl.get_uniform_location(blur, "u_texture").as_ref(), 0);
            let step = gl.get_uniform_location(blur, "u_step");
            gl.viewport(0, 0, small.0, small.1);
            for pass in 0..strength.passes {
                if pass == 1 && strength.progressive {
                    gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(set.small[0].framebuffer));
                    gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(set.small[2].framebuffer));
                    gl.blit_framebuffer(
                        0,
                        0,
                        small.0,
                        small.1,
                        0,
                        0,
                        small.0,
                        small.1,
                        glow::COLOR_BUFFER_BIT,
                        glow::NEAREST,
                    );
                }
                for (from, to, direction) in [(0, 1, (1.0, 0.0)), (1, 0, (0.0, 1.0))] {
                    gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(set.small[to].framebuffer));
                    gl.bind_texture(glow::TEXTURE_2D, Some(set.small[from].texture));
                    gl.uniform_2_f32(
                        step.as_ref(),
                        direction.0 / small.0 as f32,
                        direction.1 / small.1 as f32,
                    );
                    gl.draw_arrays(glow::TRIANGLES, 0, 3);
                }
            }
            // Back onto the screen, within the clip.
            gl.bind_framebuffer(glow::FRAMEBUFFER, screen);
            gl.viewport(x, y, w, h);
            gl.enable(glow::SCISSOR_TEST);
            gl.scissor(
                clip.left_px,
                clip.from_bottom_px,
                clip.width_px,
                clip.height_px,
            );
            gl.use_program(Some(compose));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(set.sharp.texture));
            gl.active_texture(glow::TEXTURE1);
            gl.bind_texture(glow::TEXTURE_2D, Some(set.small[0].texture));
            // Without a lighter copy kept, the deep blur stands in for it.
            let light = if strength.progressive && strength.passes > 1 {
                2
            } else {
                0
            };
            gl.active_texture(glow::TEXTURE2);
            gl.bind_texture(glow::TEXTURE_2D, Some(set.small[light].texture));
            let uniform = |name| gl.get_uniform_location(compose, name);
            gl.uniform_1_i32(uniform("u_sharp").as_ref(), 0);
            gl.uniform_1_i32(uniform("u_blur").as_ref(), 1);
            gl.uniform_1_i32(uniform("u_light").as_ref(), 2);
            gl.uniform_1_f32(uniform("u_fade").as_ref(), fade as f32 / h as f32);
            gl.uniform_1_f32(uniform("u_saturation").as_ref(), SATURATION);
            gl.uniform_2_f32(uniform("u_size").as_ref(), w as f32, h as f32);
            gl.uniform_1_f32(uniform("u_radius").as_ref(), radius);
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            gl.bind_texture(glow::TEXTURE_2D, None);
            gl.active_texture(glow::TEXTURE1);
            gl.bind_texture(glow::TEXTURE_2D, None);
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, None);
            // egui restores the rest of its state after a callback.
        }
    }
}

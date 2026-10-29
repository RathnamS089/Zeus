//! The programs a frost runs: a separable Gaussian blur, and the composite
//! that writes the blur back with its fade and rounded corners.

use eframe::glow::{self, HasContext};

pub(super) const VERTEX: &str = r"
out vec2 v_uv;
void main() {
    vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    v_uv = p;
    gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}
";

/// Nine taps folded into five linear samples.
pub(super) const BLUR: &str = r"
uniform sampler2D u_texture;
uniform vec2 u_step;
in vec2 v_uv;
out vec4 o_color;
void main() {
    vec2 near = u_step * 1.3846153846;
    vec2 far = u_step * 3.2307692308;
    o_color = texture(u_texture, v_uv) * 0.2270270270
        + (texture(u_texture, v_uv + near) + texture(u_texture, v_uv - near)) * 0.3162162162
        + (texture(u_texture, v_uv + far) + texture(u_texture, v_uv - far)) * 0.0702702703;
}
";

pub(super) const COMPOSE: &str = r"
uniform sampler2D u_sharp;
uniform sampler2D u_blur;
uniform sampler2D u_light;
uniform float u_fade;
uniform float u_saturation;
uniform vec2 u_size;
uniform float u_radius;
in vec2 v_uv;
out vec4 o_color;
vec4 saturate(vec4 blur) {
    float luma = dot(blur.rgb, vec3(0.2126, 0.7152, 0.0722));
    // Colours are premultiplied, so they stay within their alpha.
    blur.rgb = clamp(mix(vec3(luma), blur.rgb, u_saturation), 0.0, blur.a);
    return blur;
}
void main() {
    // A negative fade eases towards the top instead, for a bar at the bottom.
    float edge = u_fade < 0.0 ? 1.0 - v_uv.y : v_uv.y;
    float frost = u_fade != 0.0 ? smoothstep(0.0, abs(u_fade), edge) : 1.0;
    // Where both blurs are the same texture this mix changes nothing; a
    // progressive frost is light at the bottom of its fade and deep from
    // just past its top.
    float fade = abs(u_fade);
    float deep = u_fade != 0.0 ? smoothstep(fade * 0.3, min(fade * 1.25, 1.0), edge) : 1.0;
    vec4 blur = saturate(mix(texture(u_light, v_uv), texture(u_blur, v_uv), deep));
    // Outside rounded corners what is there stays, antialiased at the edge.
    vec2 half_size = u_size * 0.5;
    vec2 q = abs(v_uv * u_size - half_size) - (half_size - vec2(u_radius));
    float outside = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - u_radius;
    frost *= clamp(0.5 - outside, 0.0, 1.0);
    o_color = mix(texture(u_sharp, v_uv), blur, frost);
}
";

/// Compiles and links the shared vertex shader with `fragment`.
pub(super) unsafe fn program(
    gl: &glow::Context,
    header: &str,
    fragment: &str,
) -> Option<glow::Program> {
    unsafe {
        let program = gl.create_program().ok()?;
        let mut shaders = Vec::new();
        for (kind, source) in [
            (glow::VERTEX_SHADER, VERTEX),
            (glow::FRAGMENT_SHADER, fragment),
        ] {
            let shader = gl.create_shader(kind).ok()?;
            gl.shader_source(shader, &format!("{header}{source}"));
            gl.compile_shader(shader);
            if !gl.get_shader_compile_status(shader) {
                log::warn!("frosted header shader: {}", gl.get_shader_info_log(shader));
                return None;
            }
            gl.attach_shader(program, shader);
            shaders.push(shader);
        }
        gl.link_program(program);
        for shader in shaders {
            gl.detach_shader(program, shader);
            gl.delete_shader(shader);
        }
        if !gl.get_program_link_status(program) {
            log::warn!(
                "frosted header program: {}",
                gl.get_program_info_log(program)
            );
            return None;
        }
        Some(program)
    }
}

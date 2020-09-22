
#version 450
layout(set = 0, binding = 0) uniform Sizes {
    vec2 u_screen_size;
    vec2 u_tex_size;
};
layout(location = 0) in vec2 a_pos;
layout(location = 1) in uvec2 a_tc;
layout(location = 2) in uvec4 a_srgba;
layout(location = 0) out vec4 v_rgba;
layout(location = 1) out vec2 v_tc;

void main() {
    gl_Position =
      vec4(2.0 * a_pos.x / u_screen_size.x - 1.0, 1.0 - 2.0 * a_pos.y / u_screen_size.y, 0.0, 1.0);
    v_rgba = vec4(a_srgba / 255.0);
    v_tc = a_tc / u_tex_size;
}
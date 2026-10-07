//! The shader in `src/shaders` is compiled and checked at build time.

fn main() {
    lumen_shader_check::check("src/shaders");
}

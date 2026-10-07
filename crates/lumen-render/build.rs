//! Every shader in `src/shaders` is compiled and checked at build time
//! (`lumen-shader-check`): an error in one fails the build.

fn main() {
    lumen_shader_check::check("src/shaders");
}

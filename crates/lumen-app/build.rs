//! On Windows, draws the app icon and the project file icon
//! (`lumen_brand`, ADR-0117) into ICO files and embeds them in `lumen.exe`
//! as icon resources 1 (the program) and 2 (`.lumen` files, the
//! association's `DefaultIcon` is `"lumen.exe",-2`). No icon files are
//! checked in, so the icons always match the drawing.

use std::path::PathBuf;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    let app = out.join("lumen.ico");
    let project = out.join("lumen-project.ico");
    std::fs::write(&app, lumen_brand::app_ico()).expect("write lumen.ico");
    std::fs::write(&project, lumen_brand::project_ico()).expect("write lumen-project.ico");
    let quoted = |p: &PathBuf| p.display().to_string().replace('\\', "\\\\");
    let script = out.join("lumen.rc");
    std::fs::write(
        &script,
        format!(
            "1 ICON \"{}\"\n2 ICON \"{}\"\n",
            quoted(&app),
            quoted(&project)
        ),
    )
    .expect("write lumen.rc");
    embed_resource::compile_for(&script, ["lumen"], embed_resource::NONE)
        .manifest_optional()
        .expect("compile the icon resources (rc.exe from the Windows SDK, or llvm-rc)");
}

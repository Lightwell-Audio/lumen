fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=src/stretch/shim.cpp");
    println!("cargo::rerun-if-changed=vendor");
    cc::Build::new()
        .cpp(true)
        .std("c++14")
        .file("src/stretch/shim.cpp")
        .include("vendor")
        .warnings(false)
        .opt_level(2)
        .debug(false)
        .flag_if_supported("-fno-fast-math")
        .flag_if_supported("-ffp-contract=off")
        .flag_if_supported("/fp:precise")
        .compile("lumen_stretch");
}

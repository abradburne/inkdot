//! Where the SDL2 library comes from when inkdot is cross-built for the stock OS: the
//! handheld's own, copied into `sysroot/` at the top of the repository (docs/building.md). On
//! nibOS, in its build machine and on a Mac the system's is linked as usual.

fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if std::env::var_os("CARGO_FEATURE_STOCK").is_some()
        && target.starts_with("aarch64-unknown-linux")
    {
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
        println!("cargo:rustc-link-search=native={dir}/../../sysroot");
        // Symbols the stock library lacks are only an error if inkdot calls them.
        println!("cargo:rustc-link-arg=-Wl,--allow-shlib-undefined");
    }
    println!("cargo:rerun-if-changed=build.rs");
}

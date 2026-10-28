//! Builds the vendored rcheevos (rcheevos/README.md says which files and why) and inkdot's C
//! shim over it into one static library.

fn main() {
    let rc = "rcheevos";
    let mut build = cc::Build::new();
    build
        .include(format!("{rc}/include"))
        .include(format!("{rc}/src"))
        .include(format!("{rc}/src/rapi"))
        .include(format!("{rc}/src/rhash"))
        .file("csrc/shim.c")
        .file(format!("{rc}/src/rc_client.c"))
        .file(format!("{rc}/src/rc_util.c"))
        .file(format!("{rc}/src/rc_compat.c"))
        .file(format!("{rc}/src/rc_version.c"))
        .file(format!("{rc}/src/rhash/hash.c"))
        .file(format!("{rc}/src/rhash/hash_rom.c"))
        .file(format!("{rc}/src/rhash/md5.c"));
    // Whole directories: their translation units reference each other freely (trigger ->
    // condset -> condition -> operand -> memref), so a subset would break on an update.
    for dir in ["rapi", "rcheevos"] {
        let mut files: Vec<_> = std::fs::read_dir(format!("{rc}/src/{dir}"))
            .expect("the vendored rcheevos")
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "c"))
            .collect();
        files.sort();
        for f in files {
            build.file(f);
        }
    }
    // Disc, encrypted-image and zip hashing are for consoles inkdot never asks about; Lua is
    // not part of the achievement language the service uses.
    for def in [
        "RC_HASH_NO_DISC",
        "RC_HASH_NO_ENCRYPTED",
        "RC_HASH_NO_ZIP",
        "RC_DISABLE_LUA",
    ] {
        build.define(def, None);
    }
    // rcheevos is upstream's, warnings and all; the shim is held to the usual standard.
    build.warnings(false).compile("inkdot_rcheevos");
    println!("cargo:rerun-if-changed=csrc");
    println!("cargo:rerun-if-changed=rcheevos/src");
    println!("cargo:rerun-if-changed=rcheevos/include");
    println!("cargo:rerun-if-changed=build.rs");
}

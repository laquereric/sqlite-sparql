use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    let profile_dir = Path::new(&out_dir)
        .ancestors()
        .nth(3)
        .expect("could not locate profile dir from OUT_DIR")
        .to_path_buf();

    let ext = if cfg!(target_os = "macos") {
        "dylib"
    } else if cfg!(target_os = "windows") {
        "dll"
    } else {
        "so"
    };

    let lib_path = profile_dir.join(format!("libsqlite_sparql.{ext}"));
    println!("cargo:rustc-env=SQLITE_SPARQL_CDYLIB={}", lib_path.display());
}

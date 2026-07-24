//! Windows linker settings for the command-rich CLI binary.

/// Reserve enough virtual stack for Clap command construction in debug and release builds.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let flag = match std::env::var("CARGO_CFG_TARGET_ENV").as_deref() {
        Ok("msvc") => "/STACK:8388608",
        Ok("gnu") => "-Wl,--stack,8388608",
        _ => return,
    };
    println!("cargo:rustc-link-arg-bin=projectatlas={flag}");
}

//! Link setup for the LibRaw FFI (TASK-04-013).
//!
//! LibRaw is an external C++ library (LGPL/CDDL dual-licensed); the render
//! crate keeps it behind the `raw` feature and behind an isolated module
//! (`src/raw.rs`) so no other code touches the FFI. Preferred link source is
//! pkg-config (`libraw_r`, the thread-safe build, which is what every distro
//! package and `brew install libraw` provides); the fallback is a local
//! static install at `~/opt/libraw` (see the module docs of `src/raw.rs`).

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    // Only needed when the raw feature is actually compiled in (and it is
    // target-gated off on Windows until a CI/packaging leg lands, same as
    // the `heif` feature — recorded in TASK-04-013).
    let feature_on = env::var("CARGO_FEATURE_RAW").is_ok();
    let windows = env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    if !feature_on || windows {
        return;
    }

    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    if let Some(out) = run("pkg-config", &["--libs", "--cflags", "libraw_r"]) {
        emit_pkg_config_output(&out);
        return;
    }

    // Local fallback for developer machines without a system LibRaw.
    let home = env::var("HOME").unwrap_or_default();
    let dir = PathBuf::from(home).join("opt").join("libraw");
    if dir.join("lib").join("libraw_r.a").is_file() {
        println!("cargo:rustc-link-search=native={}", dir.join("lib").display());
        println!("cargo:rustc-link-lib=static=raw_r");
        // LibRaw's deflate-DNG decoder pulls zlib in (Libs.private: -lz).
        println!("cargo:rustc-link-lib=z");
        link_cpp_runtime();
        return;
    }

    panic!(
        "the `raw` feature needs LibRaw: install it with pkg-config (brew install libraw / \
         apt install libraw-dev) or build a static copy into ~/opt/libraw"
    );
}

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Translate pkg-config output into cargo link directives, keeping `-lstdc++`
/// handling explicit (pkg-config's `Libs:` already carries it on Linux; the
/// macOS static fallback needs `-lc++` from [`link_cpp_runtime`]).
fn emit_pkg_config_output(out: &str) {
    for part in out.split_whitespace() {
        if let Some(libs) = part.strip_prefix("-l") {
            if libs == "z" {
                println!("cargo:rustc-link-lib=z");
            } else if libs == "stdc++" {
                // Normalized below; a bare -lstdc++ does not link the C++
                // runtime on macOS.
                link_cpp_runtime();
            } else {
                println!("cargo:rustc-link-lib={libs}");
            }
        } else if let Some(dir) = part.strip_prefix("-L") {
            println!("cargo:rustc-link-search=native={dir}");
        }
    }
}

fn link_cpp_runtime() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-lib=c++");
    } else {
        println!("cargo:rustc-link-lib=stdc++");
    }
}

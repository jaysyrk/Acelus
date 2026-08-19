use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("cargo sets the manifest"));
    let procguard = manifest
        .join("..")
        .join("..")
        .join("native")
        .join("procguard");
    let out = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"));

    println!("cargo:rerun-if-changed={}", procguard.join("src").display());
    println!(
        "cargo:rerun-if-changed={}",
        procguard.join("build.zig").display()
    );
    println!("cargo:rerun-if-env-changed=ZIG");

    let zig = env::var("ZIG").unwrap_or_else(|_| "zig".to_string());
    let mut command = Command::new(&zig);
    command
        .current_dir(&procguard)
        .arg("build")
        .arg("--prefix")
        .arg(&out)
        .arg("--cache-dir")
        .arg(out.join("zig-cache"));

    if let Some(target) = cross_target() {
        command.arg(format!("-Dtarget={target}"));
    }

    if env::var("PROFILE").as_deref() == Ok("release") {
        command.arg("-Doptimize=ReleaseSafe");
    }

    let status = command.status().unwrap_or_else(|error| {
        panic!(
            "could not run `{zig} build` in {}: {error}. \
             Acelus needs Zig 0.16 or newer on PATH, or the ZIG variable pointing at it.",
            procguard.display()
        )
    });

    if !status.success() {
        panic!("`{zig} build` failed in {}", procguard.display());
    }

    println!(
        "cargo:rustc-link-search=native={}",
        out.join("lib").display()
    );
    println!("cargo:rustc-link-lib=static=procguard");
}

fn cross_target() -> Option<String> {
    let target = env::var("TARGET").ok()?;
    let host = env::var("HOST").ok()?;
    if target == host {
        return None;
    }

    let mut parts = target.split('-');
    let arch = match parts.next()? {
        "i686" | "i586" => "x86",
        other => other,
    };

    let rest: Vec<&str> = parts.collect();
    let suffix = match rest.as_slice() {
        ["apple", "darwin"] => "macos".to_string(),
        ["unknown", "linux", abi] => format!("linux-{abi}"),
        ["linux", abi] => format!("linux-{abi}"),
        ["pc", "windows", abi] => format!("windows-{abi}"),
        _ => return None,
    };

    Some(format!("{arch}-{suffix}"))
}

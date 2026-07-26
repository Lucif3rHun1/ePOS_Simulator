use std::env;
use std::path::PathBuf;

fn main() {
    let target = env::var("TARGET").unwrap_or_default();
    if !target.contains("windows") {
        return;
    }
    // Tell rustc to link winspool by name.
    println!("cargo:rustc-link-lib=winspool");
    // Tell rustc where to find winspool.lib. We try:
    //   1. WindowsSdkDir env var (set by vcvars64.bat)
    //   2. WindowsKitsDir env var (alternate name)
    //   3. Hard-coded well-known paths on windows-latest
    let arch = if target.contains("x86_64") { "x64" }
               else if target.contains("aarch64") { "arm64" }
               else if target.contains("i686") { "x86" }
               else { "x64" };
    let mut candidates: Vec<PathBuf> = Vec::new();
    for var in ["WindowsSdkDir", "WindowsKitsDir", "WindowsSDKLibVersion"] {
        if let Ok(v) = env::var(var) {
            candidates.push(PathBuf::from(v));
        }
    }
    // Fallback: enumerate %ProgramFiles(x86)%\Windows Kits\10\Lib\*\um\<arch>
    if let Ok(pf86) = env::var("ProgramFiles(x86)") {
        let base = PathBuf::from(pf86).join("Windows Kits").join("10").join("Lib");
        if base.exists() {
            if let Ok(entries) = std::fs::read_dir(&base) {
                for e in entries.flatten() {
                    candidates.push(e.path().join("um").join(arch));
                }
            }
        }
    }
    // Emit link-search for the first candidate that exists.
    for c in &candidates {
        if c.exists() {
            println!("cargo:rustc-link-search=native={}", c.display());
            break;
        }
    }
    // Re-run if any of these env vars change.
    println!("cargo:rerun-if-env-changed=WindowsSdkDir");
    println!("cargo:rerun-if-env-changed=WindowsKitsDir");
}

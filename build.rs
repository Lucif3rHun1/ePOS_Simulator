use std::env;

fn main() {
    let target = env::var("TARGET").unwrap_or_default();
    if target.ends_with("-windows-msvc") || target.ends_with("-windows-gnu") {
        // winspool.drv exports the printer spooler functions we call.
        println!("cargo:rustc-link-lib=winspool");
    }
}

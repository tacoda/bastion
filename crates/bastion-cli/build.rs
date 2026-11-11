//! The target this build is for, which `bastion update` downloads for.
fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    println!("cargo:rustc-env=BASTION_TARGET={target}");
    println!("cargo:rerun-if-env-changed=BASTION_VERSION");
}

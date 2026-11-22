
fn main() {
    println!("cargo:rerun-if-changed=native/windows-x64/mpv.lib");
    println!("cargo:rustc-link-search=native=native/windows-x64");
    println!("cargo:rustc-link-lib=dylib=mpv");
}

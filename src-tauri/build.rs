fn main() {
    #[cfg(feature = "desktop")]
    tauri_build::build();
    println!(
        "cargo:rustc-env=LOCALSCRIBE_TARGET={}",
        std::env::var("TARGET").unwrap()
    );
}

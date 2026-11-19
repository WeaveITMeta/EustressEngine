fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(feature = "spike")]
    compile_ui();
}

/// Compiles `ui/spike.slint`, which re-exports Studio's own `StudioWindow`
/// (from `crates/engine/ui/slint/main.slint`, read only) next to the spike's
/// `GlassDemo`. Same style as Studio's build script, and on a thread with a
/// large stack for the same reason: Studio's UI tree is deep enough to overflow
/// a build script's 1 MiB main-thread stack on Windows.
#[cfg(feature = "spike")]
fn compile_ui() {
    println!("cargo:rerun-if-changed=ui/");
    println!("cargo:rerun-if-changed=../engine/ui/slint/");
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(|| {
            let config = slint_build::CompilerConfiguration::new().with_style("fluent-dark".into());
            slint_build::compile_with_config("ui/spike.slint", config)
                .expect("Failed to compile the spike's Slint UI");
        })
        .expect("Failed to spawn the Slint compile thread")
        .join()
        .expect("The Slint compile thread panicked");
}

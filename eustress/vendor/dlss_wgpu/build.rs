use std::{env, path::PathBuf};

fn main() {
    if cfg!(feature = "mock") {
        return;
    }

    // Get SDK paths
    let dlss_sdk = env::var("DLSS_SDK")
        .expect("DLSS_SDK environment variable not set. Consult the dlss_wgpu readme.");
    let vulkan_sdk = env::var("VULKAN_SDK").expect("VULKAN_SDK environment variable not set");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // Link to needed libraries
    #[cfg(not(target_os = "windows"))]
    {
        println!("cargo:rustc-link-search=native={dlss_sdk}/lib/Linux_x86_64");
        println!("cargo:rustc-link-lib=static=nvsdk_ngx");
        println!("cargo:rustc-link-lib=dylib=stdc++");
        println!("cargo:rustc-link-lib=dylib=dl");
    }
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rustc-link-search=native={dlss_sdk}/lib/Windows_x86_64/x64");
        // NGX ships one static library per MSVC runtime: nvsdk_ngx_s.lib for the static CRT
        // (/MT) and nvsdk_ngx_d.lib for the dynamic CRT (/MD). A build script is compiled for
        // the host, so `cfg(target_feature)` here describes the host. Cargo passes the
        // target's features in CARGO_CFG_TARGET_FEATURE, which decides the CRT of the artifact.
        let crt_static = env::var("CARGO_CFG_TARGET_FEATURE")
            .map(|features| features.split(',').any(|feature| feature == "crt-static"))
            .unwrap_or(false);
        if crt_static {
            println!("cargo:rustc-link-lib=static=nvsdk_ngx_s");
        } else {
            println!("cargo:rustc-link-lib=static=nvsdk_ngx_d");
        }
        // NGX imports the registry API (RegOpenKeyExW, RegQueryValueExW, RegCloseKey) from
        // advapi32 and GetWindowThreadProcessId from user32. Its libraries carry no
        // /DEFAULTLIB directive for either, so they are linked here.
        println!("cargo:rustc-link-lib=dylib=advapi32");
        println!("cargo:rustc-link-lib=dylib=user32");
    }

    // Generate rust bindings
    #[cfg(not(target_os = "windows"))]
    let vulkan_sdk_include = "include";
    #[cfg(target_os = "windows")]
    let vulkan_sdk_include = "Include";
    bindgen::Builder::default()
        .header(format!("{}/src/wrapper.h", env!("CARGO_MANIFEST_DIR")))
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .wrap_static_fns(true)
        .wrap_static_fns_path(out_dir.join("wrap_static_fns"))
        .clang_arg(format!("-I{dlss_sdk}/include"))
        .clang_arg(format!("-I{vulkan_sdk}/{vulkan_sdk_include}"))
        .allowlist_item(".*NGX.*")
        .blocklist_item("Vk.*")
        .blocklist_item("PFN_vk.*")
        .blocklist_item(".*Cuda.*")
        .blocklist_item(".*CUDA.*")
        .generate()
        .unwrap()
        .write_to_file(out_dir.join("bindings.rs"))
        .unwrap();

    // Generate and link a library for static inline functions
    cc::Build::new()
        .file(out_dir.join("wrap_static_fns.c"))
        .includes([
            format!("{dlss_sdk}/include"),
            format!("{vulkan_sdk}/{vulkan_sdk_include}"),
        ])
        .compile("wrap_static_fns");
}

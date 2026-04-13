fn main() {
    #[cfg(target_os = "macos")]
    {
        const DEFAULT_MACOS_DEPLOYMENT_TARGET: &str = "13.0";

        let deployment_target = std::env::var("MACOSX_DEPLOYMENT_TARGET")
            .unwrap_or_else(|_| DEFAULT_MACOS_DEPLOYMENT_TARGET.to_string());
        let minimum_version_flag = format!("-mmacosx-version-min={deployment_target}");

        println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");

        cc::Build::new()
            .file("src/audio_backend/macos_sc_shim.m")
            .flag("-fobjc-arc")
            .flag(&minimum_version_flag)
            .compile("carrytalk_macos_sc_shim");
        println!("cargo:rustc-link-lib=framework=ScreenCaptureKit");
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=framework=CoreMedia");
        println!("cargo:rustc-link-lib=framework=ApplicationServices");
        println!("cargo:rustc-link-lib=framework=CoreAudio");
        println!("cargo:rustc-link-lib=framework=AudioToolbox");
    }

    tauri_build::build();
}

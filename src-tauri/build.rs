fn main() {
    #[cfg(target_os = "macos")]
    {
        cc::Build::new()
            .file("src/audio_backend/macos_sc_shim.m")
            .flag("-fobjc-arc")
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

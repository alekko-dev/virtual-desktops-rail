fn main() {
    println!("cargo:rerun-if-env-changed=LOCALAPPDATA");
    windows_reactor_setup::as_self_contained();
    // The preview setup helper logs some failed downloads/copies without failing.
    // A successful build must never silently produce a helper missing its runtime.
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let profile = std::env::var_os("PROFILE").unwrap();
    let directory = out
        .ancestors()
        .find(|p| p.file_name() == Some(profile.as_os_str()))
        .unwrap();
    let icon = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../rail/assets/rail.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    std::fs::copy(icon, directory.join("rail.ico")).expect("Stage Settings app icon");
    for name in [
        "Microsoft.UI.Xaml.dll",
        "Microsoft.WindowsAppRuntime.dll",
        "resources.pri",
        "Microsoft.Web.WebView2.Core.dll",
    ] {
        assert!(
            directory.join(name).is_file(),
            "Missing Settings runtime file {name}; check network access and the windows-reactor-setup cache, then rebuild."
        );
    }
}

fn main() {
    let root = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    println!("cargo:rerun-if-changed=assets/rail.ico");
    let resource = out.join("rail.rc");
    std::fs::write(
        &resource,
        format!(
            "101 ICON \"{}\"\n",
            root.join("assets/rail.ico")
                .display()
                .to_string()
                .replace('\\', "/")
        ),
    )
    .unwrap();
    let compiled = out.join("rail.res");
    // Cargo does not always inherit the Windows SDK tools in PATH.
    let sdk = std::path::PathBuf::from(std::env::var_os("ProgramFiles(x86)").unwrap())
        .join("Windows Kits/10/bin");
    let mut compilers: Vec<_> = std::fs::read_dir(sdk)
        .unwrap()
        .flatten()
        .map(|entry| entry.path().join("x64/rc.exe"))
        .filter(|path| path.is_file())
        .collect();
    compilers.sort();
    let status = std::process::Command::new(
        compilers
            .last()
            .expect("Install the Windows SDK resource compiler"),
    )
    .arg("/nologo")
    .arg("/fo")
    .arg(&compiled)
    .arg(&resource)
    .status()
    .expect("Run Windows SDK resource compiler");
    assert!(status.success(), "Icon resource compilation failed");
    println!("cargo:rustc-link-arg={}", compiled.display());
    let manifest =
        std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("rail.manifest");
    println!("cargo:rerun-if-changed=rail.manifest");
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
}

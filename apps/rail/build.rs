#[allow(dead_code)]
#[path = "src/build_identity.rs"]
mod identity;

fn main() {
    let root = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    // An absent input refreshes workspace-wide Git state on every Cargo build,
    // including new untracked files and commits without application edits.
    println!(
        "cargo:rerun-if-changed={}",
        out.join("refresh-provenance").display()
    );
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
    };
    let expected_root = root.join("../..").canonicalize().unwrap();
    let git_root = git(&["rev-parse", "--show-toplevel"])
        .and_then(|path| std::path::Path::new(path.trim()).canonicalize().ok());
    // Exported source must not inherit provenance from an enclosing repository.
    let provenance = (git_root.as_ref() == Some(&expected_root))
        .then(|| {
            let commit = git(&["rev-parse", "--verify", "HEAD"])?;
            let status = git(&["status", "--porcelain", "--untracked-files=all"])?;
            Some((commit.trim().to_string(), !status.trim().is_empty()))
        })
        .flatten();
    let commit = provenance.as_ref().map(|(commit, _)| commit.as_str());
    let dirty = provenance.as_ref().map(|(_, dirty)| *dirty);
    std::fs::write(out.join("build_identity.rs"), format!(
        "pub const COMMIT: Option<&str> = {commit:?};\npub const DIRTY: Option<bool> = {dirty:?};\n"
    )).unwrap();
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let parts = identity::windows_version(&version).expect("Valid Windows application version");
    let numeric = parts.map(|part| part.to_string());
    let dotted = numeric.join(".");
    let fixed = numeric.join(",");
    let product = identity::product_version(&version, commit, dirty);
    let resource = out.join("rail.rc");
    std::fs::write(
        &resource,
        format!(
            r#"101 ICON "{}"
1 VERSIONINFO
FILEVERSION {fixed}
PRODUCTVERSION {fixed}
FILEFLAGSMASK 0x3fL
FILEFLAGS 0
FILEOS 0x40004L
FILETYPE 1
FILESUBTYPE 0
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "FileDescription", "Virtual Desktop Rail\0"
            VALUE "FileVersion", "{dotted}\0"
            VALUE "ProductName", "Virtual Desktop Rail\0"
            VALUE "ProductVersion", "{product}\0"
            VALUE "OriginalFilename", "virtual-desktop-rail.exe\0"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#,
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
    let manifest = out.join("rail.manifest");
    let template = std::fs::read_to_string(root.join("rail.manifest")).unwrap();
    assert_eq!(template.matches("@APP_VERSION@").count(), 1);
    std::fs::write(&manifest, template.replace("@APP_VERSION@", &dotted)).unwrap();
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
}

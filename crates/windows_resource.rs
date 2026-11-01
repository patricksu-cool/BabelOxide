// Shared build-time Windows metadata. Uses the already installed SDK/windres.
use std::{env, fs, path::PathBuf, process::Command};

pub fn embed_windows_resource(public_binary: &str) {
    println!("cargo:rerun-if-changed=../windows_resource.rs");
    println!("cargo:rerun-if-changed=../../resources/babeloxide.ico");
    let target = env::var("TARGET").expect("Cargo target");
    if !target.contains("windows") {
        return;
    }
    let version = env::var("CARGO_PKG_VERSION").expect("Workspace version");
    let mut components: Vec<_> = version.split('.').collect();
    components.resize(4, "0");
    let tuple = components.join(",");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output"));
    let icon = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest"))
        .join("../../resources/babeloxide.ico")
        .canonicalize()
        .expect("Brand icon");
    let rc = out.join("babeloxide.rc");
    fs::write(
        &rc,
        format!(
            r#"
1 ICON "{}"
1 VERSIONINFO
FILEVERSION {tuple}
PRODUCTVERSION {tuple}
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "BabelOxide contributors\0"
      VALUE "FileDescription", "BabelOxide PDF Translator\0"
      VALUE "FileVersion", "{version}\0"
      VALUE "InternalName", "{public_binary}\0"
      VALUE "OriginalFilename", "{public_binary}.exe\0"
      VALUE "ProductName", "BabelOxide\0"
      VALUE "ProductVersion", "{version}\0"
      VALUE "LegalCopyright", "See LICENSE and THIRD_PARTY_NOTICES.md\0"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x0409, 1200
  END
END
"#,
            icon.to_string_lossy()
                .trim_start_matches(r"\\?\")
                .replace('\\', "/")
        ),
    )
    .expect("Write version resource");
    if target.ends_with("-gnu") {
        let object = out.join("babeloxide-resource.o");
        let status = Command::new("windres")
            .args(["-O", "coff", "-i"])
            .arg(&rc)
            .arg("-o")
            .arg(&object)
            .status()
            .expect("An existing windres/llvm-windres is required for Windows GNU metadata");
        assert!(status.success(), "Compile Windows metadata");
        println!("cargo:rustc-link-arg-bins={}", object.display());
    } else {
        let rc_tool =
            find_sdk_rc().expect("Windows SDK rc.exe is required for executable metadata");
        let resource = out.join("babeloxide.res");
        let status = Command::new(rc_tool)
            .arg("/nologo")
            .arg("/fo")
            .arg(&resource)
            .arg(&rc)
            .status()
            .expect("Run Windows resource compiler");
        assert!(status.success(), "Compile Windows metadata");
        println!("cargo:rustc-link-arg-bins={}", resource.display());
    }
}

fn find_sdk_rc() -> Option<PathBuf> {
    let sdk = PathBuf::from(env::var_os("ProgramFiles(x86)")?).join("Windows Kits/10/bin");
    let mut versions: Vec<_> = fs::read_dir(sdk)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    versions.sort();
    versions.reverse();
    versions
        .into_iter()
        .map(|version| version.join("x64/rc.exe"))
        .find(|path| path.is_file())
}

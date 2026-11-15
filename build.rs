//! Generates the app icon and compiles the Windows resources (icon, manifest,
//! version info) into the executable.

#[path = "src/icon_art.rs"]
#[allow(dead_code)]
mod icon_art;

use std::env;
use std::fs;
use std::path::PathBuf;

fn rc_path(p: &std::path::Path) -> String {
    p.display().to_string().replace('\\', "\\\\")
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/icon_art.rs");
    println!("cargo:rerun-if-changed=res/app.manifest");

    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    let ico = out.join("app.ico");
    fs::write(&ico, icon_art::ico(&[16, 20, 24, 32, 40, 48, 64, 256])).unwrap();

    let version = env::var("CARGO_PKG_VERSION").unwrap();
    let mut parts: Vec<u32> = version.split(['.', '-']).filter_map(|p| p.parse().ok()).collect();
    parts.resize(4, 0);
    let v_commas = format!("{},{},{},{}", parts[0], parts[1], parts[2], parts[3]);
    let v_dots = format!("{}.{}.{}.{}", parts[0], parts[1], parts[2], parts[3]);

    let rc = format!(
        r#"#pragma code_page(65001)
1 ICON "{ico}"
1 24 "{manifest}"

1 VERSIONINFO
FILEVERSION {v_commas}
PRODUCTVERSION {v_commas}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "FileDescription", "Askeri Ruler"
      VALUE "ProductName", "Askeri Ruler"
      VALUE "FileVersion", "{v_dots}"
      VALUE "ProductVersion", "{v_dots}"
      VALUE "OriginalFilename", "askeri-ruller.exe"
      VALUE "InternalName", "askeri-ruller"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        ico = rc_path(&ico),
        manifest = rc_path(&root.join("res").join("app.manifest")),
    );
    let rc_file = out.join("app.rc");
    fs::write(&rc_file, rc).unwrap();
    embed_resource::compile(&rc_file, embed_resource::NONE).manifest_required().unwrap();
}

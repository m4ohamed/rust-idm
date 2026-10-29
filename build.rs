fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=assets/icon.png");
    println!("cargo:rerun-if-changed=assets/icon_64.rgba");
    println!("cargo:rerun-if-changed=assets/app.rc");

    #[cfg(target_os = "windows")]
    {
        embed_windows_icon_if_available();
    }

    #[cfg(feature = "slint-ui")]
    {
        slint_build::compile("ui/app.slint").expect("Failed to compile Slint UI (ui/app.slint)");
    }
}

#[cfg(target_os = "windows")]
fn embed_windows_icon_if_available() {
    use std::path::PathBuf;
    use std::process::Command;

    let out_dir = match std::env::var_os("OUT_DIR") {
        Some(d) => PathBuf::from(d),
        None => return,
    };
    let rc_path = PathBuf::from("assets/app.rc");
    if !rc_path.exists() {
        return;
    }

    let res_file = out_dir.join("stormget_icon.res");
    for tool in ["rc.exe", "llvm-rc.exe"] {
        if let Ok(status) = Command::new(tool)
            .args(["/fo", &res_file.to_string_lossy(), "assets/app.rc"])
            .status()
        {
            if status.success() && res_file.exists() {
                println!("cargo:rustc-link-arg-bins={}", res_file.display());
                return;
            }
        }
    }

    let obj_file = out_dir.join("stormget_icon.o");
    if let Ok(status) = Command::new("windres")
        .args([
            "assets/app.rc",
            "-O",
            "coff",
            "-o",
            &obj_file.to_string_lossy(),
        ])
        .status()
    {
        if status.success() && obj_file.exists() {
            println!("cargo:rustc-link-arg-bins={}", obj_file.display());
        }
    }
}

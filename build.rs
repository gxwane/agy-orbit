//! agy-orbit cross-platform build script (build.rs)
//! Embeds Windows PE version metadata and application manifest on Windows targets.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    compile_windows_pe_resources();
}

#[cfg(windows)]
fn compile_windows_pe_resources() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let mut res = winres::WindowsResource::new();

    let version = env!("CARGO_PKG_VERSION");
    let desc = env!("CARGO_PKG_DESCRIPTION");
    let authors = env!("CARGO_PKG_AUTHORS");

    let major = env!("CARGO_PKG_VERSION_MAJOR");
    let minor = env!("CARGO_PKG_VERSION_MINOR");
    let patch = env!("CARGO_PKG_VERSION_PATCH");
    let pe_manifest_version = format!("{major}.{minor}.{patch}.0");

    res.set("FileDescription", desc);
    res.set("ProductName", "agy-orbit");
    res.set("OriginalFilename", "agyo.exe");
    res.set("LegalCopyright", &format!("Copyright (c) 2026 {authors}"));
    res.set("ProductVersion", version);
    res.set("FileVersion", version);

    const MANIFEST_TEMPLATE: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
    <assemblyIdentity
        version="__MANIFEST_VERSION__"
        processorArchitecture="*"
        name="GXWane.AgyOrbit.agyo"
        type="win32"
    />
    <description>Seamless Multi-Account Manager for Antigravity CLI</description>
    <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
        <security>
            <requestedPrivileges>
                <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
            </requestedPrivileges>
        </security>
    </trustInfo>
    <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
        <application>
            <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
        </application>
    </compatibility>
</assembly>
"#;

    let manifest_content = MANIFEST_TEMPLATE.replace("__MANIFEST_VERSION__", &pe_manifest_version);
    res.set_manifest(&manifest_content);

    if let Err(e) = res.compile() {
        eprintln!("cargo:warning=[agy-orbit] Failed to embed Windows PE resource: {e}");
    }
}

#[cfg(not(windows))]
fn compile_windows_pe_resources() {}

fn manifest_version() -> String {
    let ver = env!("CARGO_PKG_VERSION");
    let dots = ver.chars().filter(|&c| c == '.').count();
    match dots {
        2 => format!("{ver}.0"),
        3 => ver.to_string(),
        _ => "1.0.0.0".to_string(),
    }
}

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let manifest_ver = manifest_version();
        let mut res = winres::WindowsResource::new();
        res.set("FileDescription", "FlexDisplay Host - Android second monitor");
        res.set("ProductName", "FlexDisplay");
        res.set("CompanyName", "FlexDisplay");
        res.set("LegalCopyright", "Copyright (C) 2025 FlexDisplay");
        res.set("InternalName", "FlexDisplay");
        res.set("OriginalFilename", "FlexDisplay.exe");
        res.set("FileVersion", env!("CARGO_PKG_VERSION"));
        res.set("ProductVersion", env!("CARGO_PKG_VERSION"));
        res.set_manifest(&format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity
    version="{manifest_ver}"
    processorArchitecture="amd64"
    name="FlexDisplay.Host"
    type="win32"/>
  <description>FlexDisplay Host</description>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}}"/>
    </application>
  </compatibility>
</assembly>"#
        ));
        if let Err(e) = res.compile() {
            eprintln!("cargo:warning=winres compile failed (PE metadata skipped): {e}");
        }
    }
}

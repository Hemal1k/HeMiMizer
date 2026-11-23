// Embeds the icon, the version info and the manifest (run as administrator) into HeMiMizer.exe.
fn main() {
    println!("cargo:rerun-if-changed=assets");
    println!("cargo:rerun-if-changed=src");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    // the release always runs as administrator; a Debug build runs as the user (for `--check` and the look)
    let level = if std::env::var("PROFILE").as_deref() == Ok("release") { "requireAdministrator" } else { "asInvoker" };
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/HeMiMizer.ico")
        .set("FileDescription", "HeMiMizer")
        .set("ProductName", "HeMiMizer")
        .set("CompanyName", "HeMi")
        .set("LegalCopyright", "© Richard Hemala")
        .set_manifest(
            &r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="LEVEL" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
      <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
    </windowsSettings>
  </application>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
        processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
</assembly>"#
                .replace("LEVEL", level),
        );
    res.compile().expect("resources");
}

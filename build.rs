fn main() {
    // Embed assets/logo.ico as icon resource ID 1 (RT_GROUP_ICON),
    // loaded at runtime via LoadIconW(instance, PCWSTR(1)) for the tray icon.
    if cfg!(target_os = "windows") {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/logo.ico");
        res.compile().unwrap();
    }
    println!("cargo:rerun-if-changed=assets/logo.ico");
}
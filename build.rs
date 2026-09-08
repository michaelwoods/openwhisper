fn main() {
    if let (Err(_), Ok(home)) = (std::env::var("PKG_CONFIG_PATH"), std::env::var("HOME")) {
        let user_pkg = format!("{}/.local/lib/pkgconfig", home);
        if std::path::Path::new(&user_pkg).exists() {
            unsafe {
                std::env::set_var("PKG_CONFIG_PATH", user_pkg);
            }
        }
    }
    slint_build::compile("ui/settings.slint").expect("Slint compilation failed");
}

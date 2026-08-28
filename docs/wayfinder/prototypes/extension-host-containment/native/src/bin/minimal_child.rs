#![windows_subsystem = "windows"]

fn main() {
    let Some(path) = std::env::var_os("KOMOREBI_PROTOTYPE_ERROR_FILE") else {
        std::process::exit(2);
    };
    if std::fs::write(path, "minimal Rust main reached").is_err() {
        std::process::exit(3);
    }
}

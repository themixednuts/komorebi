#![windows_subsystem = "windows"]

fn main() {
    std::panic::set_hook(Box::new(|info| write_error(&format!("panic: {info}"))));
    if let Err(error) = wayfinder_extension_containment_prototype::fault_child::run() {
        write_error(&format!("{error:#}"));
        std::process::exit(1);
    }
}

fn write_error(message: &str) {
    if let Some(path) = std::env::var_os("KOMOREBI_PROTOTYPE_ERROR_FILE") {
        // The child has no console; this best-effort file is secondary to its process exit code.
        let _ = std::fs::write(path, message);
    }
}

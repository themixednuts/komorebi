#![windows_subsystem = "windows"]

use wayfinder_extension_containment_prototype::child;
use wayfinder_extension_containment_prototype::protocol::RuntimeKind;

fn main() {
    std::panic::set_hook(Box::new(|info| write_error(&format!("panic: {info}"))));
    if let Err(error) = child::run(RuntimeKind::Rust) {
        write_error(&format!("{error:#}"));
        std::process::exit(1);
    }
}

fn write_error(message: &str) {
    if let Ok(path) = std::env::var("KOMOREBI_PROTOTYPE_ERROR_FILE") {
        let _ = std::fs::write(path, message);
    }
}

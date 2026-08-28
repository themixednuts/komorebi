use drizzle_migrations::build::{Config, Output, run};
use drizzle_types::Dialect;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The Rust schema is the sole migration source; build.rs writes only generated artifacts.
    let configuration = Config::new(Dialect::SQLite)
        .file("./src/schema.rs")
        .out("./drizzle");
    configuration.watch();
    if let Output::Generated { tag, .. } = run(&configuration)? {
        println!("cargo:warning=generated Drizzle migration {tag}");
    }
    Ok(())
}

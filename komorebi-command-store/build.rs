use drizzle_migrations::build::Config;
use drizzle_migrations::build::Output;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_toml("drizzle.config.toml")?;
    config.watch();

    if let Output::Generated { tag, .. } = drizzle_migrations::build::run(&config)? {
        println!("cargo:warning=generated invocation-ledger migration {tag}");
    }

    Ok(())
}

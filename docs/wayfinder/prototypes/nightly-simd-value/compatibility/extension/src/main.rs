fn main() -> anyhow::Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(&toolchain_extension_compatibility::run()?)?
    );
    Ok(())
}

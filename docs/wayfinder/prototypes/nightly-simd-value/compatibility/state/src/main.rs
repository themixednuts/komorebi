fn main() -> anyhow::Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(&toolchain_state_compatibility::run()?)?
    );
    Ok(())
}

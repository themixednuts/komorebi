fn main() -> Result<(), serde_json::Error> {
    println!(
        "{}",
        serde_json::to_string_pretty(&toolchain_shell_compatibility::inspect())?
    );
    Ok(())
}

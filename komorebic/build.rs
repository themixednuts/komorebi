use shadow_rs::ShadowBuilder;

fn main() {
    if std::env::var_os("CARGO_CFG_TARGET_ENV").is_some_and(|environment| environment == "msvc") {
        // clap's complete command tree plus the root async dispatch exceeds the
        // Windows PE default 1 MiB stack reserve in debug builds. 4 MiB is the
        // smallest tested power-of-two reserve with enough audit headroom.
        println!("cargo:rustc-link-arg-bin=komorebic=/STACK:4194304");
    }

    if std::fs::metadata("applications.json").is_err() {
        let applications_json = reqwest::blocking::get(
        "https://raw.githubusercontent.com/LGUG2Z/komorebi-application-specific-configuration/master/applications.json"
    ).unwrap().text().unwrap();
        std::fs::write("applications.json", applications_json).unwrap();
    }

    ShadowBuilder::builder().build().unwrap();
}

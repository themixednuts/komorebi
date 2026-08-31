const DIRECTION_NAMES: [&str; 4] = ["left", "right", "up", "down"];

#[must_use]
pub fn generated_typescript_declarations() -> String {
    let directions = DIRECTION_NAMES
        .iter()
        .map(|name| format!("\"{name}\""))
        .collect::<Vec<_>>()
        .join(" | ");
    [
        "declare module \"komorebi:host\" {\n".to_owned(),
        format!("  export type Direction = {directions};\n"),
        "  export function focus(direction: Direction): Promise<void>;\n}\n".to_owned(),
    ]
    .concat()
}

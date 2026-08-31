use quickjs_plugin_spike::generated_typescript_declarations;

#[test]
fn generated_host_types_describe_the_runtime_module_without_project_configuration() {
    assert_eq!(
        generated_typescript_declarations(),
        concat!(
            "declare module \"komorebi:host\" {\n",
            "  export type Direction = \"left\" | \"right\" | \"up\" | \"down\";\n",
            "  export function focus(direction: Direction): Promise<void>;\n",
            "}\n",
        )
    );
}

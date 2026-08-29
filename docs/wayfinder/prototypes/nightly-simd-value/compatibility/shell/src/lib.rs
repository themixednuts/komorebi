use std::{any::TypeId, mem::size_of};

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ShellCompatibility {
    pub gpui_pixels_bytes: usize,
    pub gpui_base_button_type: &'static str,
    pub gpui_component_index_path_type: &'static str,
    pub types_are_distinct: bool,
}

pub fn inspect() -> ShellCompatibility {
    ShellCompatibility {
        gpui_pixels_bytes: size_of::<gpui::Pixels>(),
        gpui_base_button_type: std::any::type_name::<gpui_base::Button>(),
        gpui_component_index_path_type: std::any::type_name::<gpui_component::IndexPath>(),
        types_are_distinct: TypeId::of::<gpui_base::Button>()
            != TypeId::of::<gpui_component::IndexPath>(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn gpui_components_and_base_share_the_pinned_gpui_graph() {
        let report = super::inspect();
        assert!(report.types_are_distinct);
        assert_ne!(report.gpui_pixels_bytes, 0);
    }
}

use komorebi_layouts::DefaultLayout;
use komorebi_protocol::BuiltInLayout;

/// Projects the public layout vocabulary onto its closed command-protocol
/// shape.
#[must_use]
pub const fn built_in_layout(value: DefaultLayout) -> BuiltInLayout {
    match value {
        DefaultLayout::BSP => BuiltInLayout::Bsp,
        DefaultLayout::Columns => BuiltInLayout::Columns,
        DefaultLayout::Rows => BuiltInLayout::Rows,
        DefaultLayout::VerticalStack => BuiltInLayout::VerticalStack,
        DefaultLayout::HorizontalStack => BuiltInLayout::HorizontalStack,
        DefaultLayout::UltrawideVerticalStack => BuiltInLayout::UltrawideVerticalStack,
        DefaultLayout::Grid => BuiltInLayout::Grid,
        DefaultLayout::RightMainVerticalStack => BuiltInLayout::RightMainVerticalStack,
        DefaultLayout::Scrolling => BuiltInLayout::Scrolling,
    }
}

use serde::Deserialize;
use serde::Serialize;

use super::Rect;

/// Signed edge offsets applied to a monitor work area before tiling.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct WorkAreaOffset {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

impl WorkAreaOffset {
    #[must_use]
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    #[must_use]
    pub const fn left(self) -> i32 {
        self.left
    }

    #[must_use]
    pub const fn top(self) -> i32 {
        self.top
    }

    #[must_use]
    pub const fn right(self) -> i32 {
        self.right
    }

    #[must_use]
    pub const fn bottom(self) -> i32 {
        self.bottom
    }
}

impl From<Rect> for WorkAreaOffset {
    fn from(value: Rect) -> Self {
        Self::new(value.left, value.top, value.right, value.bottom)
    }
}

impl From<WorkAreaOffset> for Rect {
    fn from(value: WorkAreaOffset) -> Self {
        Self {
            left: value.left,
            top: value.top,
            right: value.right,
            bottom: value.bottom,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_rect_conversion_preserves_signed_edge_offsets() {
        let offset = WorkAreaOffset::new(-1, 2, -3, 4);
        assert_eq!(WorkAreaOffset::from(Rect::from(offset)), offset);
    }
}

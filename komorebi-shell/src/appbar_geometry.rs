use std::num::NonZeroU32;

use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppBarEdge {
    Left,
    Top,
    Right,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LogicalAppBarThickness(f32);

impl LogicalAppBarThickness {
    pub fn new(value: f32) -> Result<Self, LogicalAppBarThicknessError> {
        if !value.is_finite() {
            Err(LogicalAppBarThicknessError::NonFinite)
        } else if value <= 0.0 {
            Err(LogicalAppBarThicknessError::NotPositive)
        } else {
            Ok(Self(value))
        }
    }

    pub fn to_physical(
        self,
        dpi: NonZeroU32,
    ) -> Result<PhysicalThickness, LogicalAppBarThicknessError> {
        const DEFAULT_DPI: f64 = 96.0;
        let scaled = (f64::from(self.0) * f64::from(dpi.get()) / DEFAULT_DPI).round();
        if scaled > f64::from(u32::MAX) {
            return Err(LogicalAppBarThicknessError::PhysicalOverflow);
        }
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the finite positive rounded value is range-checked before conversion"
        )]
        let pixels = scaled.max(1.0) as u32;
        PhysicalThickness::new(pixels).ok_or(LogicalAppBarThicknessError::PhysicalOverflow)
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum LogicalAppBarThicknessError {
    #[error("logical AppBar thickness must be finite")]
    NonFinite,
    #[error("logical AppBar thickness must be positive")]
    NotPositive,
    #[error("logical AppBar thickness exceeds the physical coordinate range")]
    PhysicalOverflow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    width: NonZeroU32,
    height: NonZeroU32,
}

impl PhysicalRect {
    pub fn new(left: i32, top: i32, right: i32, bottom: i32) -> Result<Self, PhysicalRectError> {
        let width = u32::try_from(i64::from(right) - i64::from(left))
            .ok()
            .and_then(NonZeroU32::new)
            .ok_or(PhysicalRectError::EmptyWidth { left, right })?;
        let height = u32::try_from(i64::from(bottom) - i64::from(top))
            .ok()
            .and_then(NonZeroU32::new)
            .ok_or(PhysicalRectError::EmptyHeight { top, bottom })?;
        Ok(Self {
            left,
            top,
            right,
            bottom,
            width,
            height,
        })
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

    #[must_use]
    pub const fn width(self) -> NonZeroU32 {
        self.width
    }

    #[must_use]
    pub const fn height(self) -> NonZeroU32 {
        self.height
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum PhysicalRectError {
    #[error("physical rectangle has no width: left {left}, right {right}")]
    EmptyWidth { left: i32, right: i32 },
    #[error("physical rectangle has no height: top {top}, bottom {bottom}")]
    EmptyHeight { top: i32, bottom: i32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalThickness(NonZeroU32);

impl PhysicalThickness {
    #[must_use]
    pub const fn new(value: u32) -> Option<Self> {
        match NonZeroU32::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    const fn get(self) -> u32 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AppBarGeometry {
    monitor: PhysicalRect,
    edge: AppBarEdge,
    thickness: PhysicalThickness,
}

impl AppBarGeometry {
    #[must_use]
    pub const fn new(
        monitor: PhysicalRect,
        edge: AppBarEdge,
        thickness: PhysicalThickness,
    ) -> Self {
        Self {
            monitor,
            edge,
            thickness,
        }
    }

    #[must_use]
    pub const fn edge(self) -> AppBarEdge {
        self.edge
    }

    pub fn proposed_rect(self) -> Result<PhysicalRect, PhysicalRectError> {
        self.apply_thickness(self.monitor)
    }

    pub fn apply_thickness(
        self,
        negotiated: PhysicalRect,
    ) -> Result<PhysicalRect, PhysicalRectError> {
        let axis_span = match self.edge {
            AppBarEdge::Left | AppBarEdge::Right => negotiated.width().get(),
            AppBarEdge::Top | AppBarEdge::Bottom => negotiated.height().get(),
        };
        let thickness = self.thickness.get().min(axis_span);
        let thickness = i64::from(thickness);
        match self.edge {
            AppBarEdge::Left => PhysicalRect::new(
                negotiated.left,
                negotiated.top,
                coordinate(i64::from(negotiated.left) + thickness),
                negotiated.bottom,
            ),
            AppBarEdge::Top => PhysicalRect::new(
                negotiated.left,
                negotiated.top,
                negotiated.right,
                coordinate(i64::from(negotiated.top) + thickness),
            ),
            AppBarEdge::Right => PhysicalRect::new(
                coordinate(i64::from(negotiated.right) - thickness),
                negotiated.top,
                negotiated.right,
                negotiated.bottom,
            ),
            AppBarEdge::Bottom => PhysicalRect::new(
                negotiated.left,
                coordinate(i64::from(negotiated.bottom) - thickness),
                negotiated.right,
                negotiated.bottom,
            ),
        }
    }
}

fn coordinate(value: i64) -> i32 {
    match i32::try_from(value) {
        Ok(value) => value,
        Err(_) if value.is_negative() => i32::MIN,
        Err(_) => i32::MAX,
    }
}

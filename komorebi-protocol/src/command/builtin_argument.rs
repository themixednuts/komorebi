use std::collections::BTreeMap;
use std::num::NonZeroI32;
use std::num::NonZeroU64;

use thiserror::Error;

use super::ActionArgument;
use super::ActionArguments;
use super::ArgumentError;
use super::ArgumentScalar;
use super::ArgumentScalars;
use super::BoundedText;
use super::ChoiceId;
use super::FixedDecimal;
use super::ParameterId;
use super::SelectorId;
use super::WindowsPathInput;

macro_rules! known_ids {
    ($name:ident, $target:ident, $($variant:ident => $value:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $value),+
                }
            }

            fn into_wire(self) -> $target {
                $target::from_known(self.as_str())
            }
        }
    };
}

known_ids! {
    BuiltInParameterId, ParameterId,
    Direction => "direction",
    Axis => "axis",
    Delta => "delta",
    Workspace => "workspace",
    Layout => "layout",
    Window => "window",
    Cycle => "cycle",
    Index => "index",
    Monitor => "monitor",
    Sizing => "sizing",
    Adjustment => "adjustment",
    Enabled => "enabled",
    Size => "size",
    Count => "count",
    Container => "container",
    Columns => "columns",
    Name => "name",
    Path => "path",
    Behaviour => "behaviour",
    Implementation => "implementation",
    Exe => "exe",
    Identifier => "identifier",
    Names => "names",
    ColumnRatios => "column-ratios",
    RowRatios => "row-ratios",
    AtCount => "at-count",
    ResizeStep => "resize-step",
    Alpha => "alpha",
    WindowKind => "window-kind",
    Red => "red",
    Green => "green",
    Blue => "blue",
    Width => "width",
    Offset => "offset",
    Style => "style",
    Mode => "mode",
    Label => "label",
    Height => "height",
    TabWidth => "tab-width",
    FontSize => "font-size",
    FontFamily => "font-family",
    Prefix => "prefix",
    Duration => "duration",
    Fps => "fps",
    Left => "left",
    Top => "top",
    Right => "right",
    Bottom => "bottom",
}

known_ids! {
    BuiltInDirection, ChoiceId,
    Left => "left",
    Right => "right",
    Up => "up",
    Down => "down",
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuiltInResizeStep(NonZeroI32);

impl BuiltInResizeStep {
    /// Creates a positive built-in resize step.
    ///
    /// # Errors
    ///
    /// Returns [`BuiltInResizeStepError`] for zero or negative values.
    pub const fn new(value: i32) -> Result<Self, BuiltInResizeStepError> {
        match NonZeroI32::new(value) {
            Some(value) if value.is_positive() => Ok(Self(value)),
            _ => Err(BuiltInResizeStepError(value)),
        }
    }

    #[must_use]
    pub const fn get(self) -> i32 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("built-in resize step must be positive; received {0}")]
pub struct BuiltInResizeStepError(i32);

known_ids! {
    BuiltInAxis, ChoiceId,
    Horizontal => "horizontal",
    Vertical => "vertical",
    HorizontalAndVertical => "horizontal-and-vertical",
}

known_ids! {
    BuiltInCycle, ChoiceId,
    Previous => "previous",
    Next => "next",
}

known_ids! {
    BuiltInLayout, ChoiceId,
    Bsp => "bsp",
    Columns => "columns",
    Rows => "rows",
    VerticalStack => "vertical-stack",
    HorizontalStack => "horizontal-stack",
    UltrawideVerticalStack => "ultrawide-vertical-stack",
    Grid => "grid",
    RightMainVerticalStack => "right-main-vertical-stack",
    Scrolling => "scrolling",
}

known_ids! {
    BuiltInSizing, ChoiceId,
    Increase => "increase",
    Decrease => "decrease",
}

known_ids! {
    BuiltInHidingBehaviour, ChoiceId,
    Hide => "hide",
    Minimize => "minimize",
    Cloak => "cloak",
}

known_ids! {
    BuiltInMoveBehaviour, ChoiceId,
    Swap => "swap",
    Insert => "insert",
    NoOp => "no-op",
}

known_ids! {
    BuiltInMonocleBehaviour, ChoiceId,
    Cycle => "cycle",
    NoOp => "no-op",
}

known_ids! {
    BuiltInOperationBehaviour, ChoiceId,
    Op => "op",
    NoOp => "no-op",
}

known_ids! {
    BuiltInImplementation, ChoiceId,
    Komorebi => "komorebi",
    Windows => "windows",
}

known_ids! {
    BuiltInWindowKind, ChoiceId,
    Single => "single",
    Stack => "stack",
    Monocle => "monocle",
    Unfocused => "unfocused",
    UnfocusedLocked => "unfocused-locked",
    Floating => "floating",
}

known_ids! {
    BuiltInBorderStyle, ChoiceId,
    System => "system",
    Rounded => "rounded",
    Square => "square",
}

known_ids! {
    BuiltInBorderImplementation, ChoiceId,
    Komorebi => "komorebi",
    Windows => "windows",
}

known_ids! {
    BuiltInStackbarMode, ChoiceId,
    Always => "always",
    Never => "never",
    OnStack => "on-stack",
}

known_ids! {
    BuiltInStackbarLabel, ChoiceId,
    Process => "process",
    Title => "title",
}

known_ids! {
    BuiltInAnimationPrefix, ChoiceId,
    Movement => "movement",
    Transparency => "transparency",
}

known_ids! {
    BuiltInNamedAnimationStyle, ChoiceId,
    Linear => "linear",
    EaseInSine => "ease-in-sine",
    EaseOutSine => "ease-out-sine",
    EaseInOutSine => "ease-in-out-sine",
    EaseInQuad => "ease-in-quad",
    EaseOutQuad => "ease-out-quad",
    EaseInOutQuad => "ease-in-out-quad",
    EaseInCubic => "ease-in-cubic",
    EaseOutCubic => "ease-out-cubic",
    EaseInOutCubic => "ease-in-out-cubic",
    EaseInQuart => "ease-in-quart",
    EaseOutQuart => "ease-out-quart",
    EaseInOutQuart => "ease-in-out-quart",
    EaseInQuint => "ease-in-quint",
    EaseOutQuint => "ease-out-quint",
    EaseInOutQuint => "ease-in-out-quint",
    EaseInExpo => "ease-in-expo",
    EaseOutExpo => "ease-out-expo",
    EaseInOutExpo => "ease-in-out-expo",
    EaseInCirc => "ease-in-circ",
    EaseOutCirc => "ease-out-circ",
    EaseInOutCirc => "ease-in-out-circ",
    EaseInBack => "ease-in-back",
    EaseOutBack => "ease-out-back",
    EaseInOutBack => "ease-in-out-back",
    EaseInElastic => "ease-in-elastic",
    EaseOutElastic => "ease-out-elastic",
    EaseInOutElastic => "ease-in-out-elastic",
    EaseInBounce => "ease-in-bounce",
    EaseOutBounce => "ease-out-bounce",
    EaseInOutBounce => "ease-in-out-bounce",
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuiltInAnimationStyle {
    Named(BuiltInNamedAnimationStyle),
    CubicBezier([FixedDecimal; 4]),
}

impl BuiltInAnimationStyle {
    fn try_into_scalars(self) -> Result<ArgumentScalars, ArgumentError> {
        let values: Box<[ArgumentScalar]> = match self {
            Self::Named(style) => [ArgumentScalar::Choice(style.into_wire())].into(),
            Self::CubicBezier(points) => points.map(ArgumentScalar::Decimal).into_iter().collect(),
        };
        ArgumentScalars::new(values)
    }
}

known_ids! {
    BuiltInIdentifier, ChoiceId,
    Exe => "exe",
    Class => "class",
    Title => "title",
    Path => "path",
}

known_ids! {
    BuiltInSelector, SelectorId,
    FocusedAtExecution => "focused-at-execution",
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuiltInNames(ArgumentScalars);

impl BuiltInNames {
    /// Creates a nonempty bounded list of workspace names.
    ///
    /// # Errors
    ///
    /// Returns [`ArgumentError`] when the list violates protocol bounds.
    pub fn new(values: impl IntoIterator<Item = BoundedText>) -> Result<Self, ArgumentError> {
        let values = values
            .into_iter()
            .map(ArgumentScalar::Text)
            .collect::<Vec<_>>();
        Ok(Self(ArgumentScalars::new(values)?))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuiltInRatios(ArgumentScalars);

impl BuiltInRatios {
    /// Creates a nonempty bounded list of exact decimal ratios.
    ///
    /// # Errors
    ///
    /// Returns [`ArgumentError`] when the list violates protocol bounds.
    pub fn new(values: impl IntoIterator<Item = FixedDecimal>) -> Result<Self, ArgumentError> {
        let values = values
            .into_iter()
            .map(ArgumentScalar::Decimal)
            .collect::<Vec<_>>();
        Ok(Self(ArgumentScalars::new(values)?))
    }
}

/// One manager-owned parameter paired with its only valid wire scalar shape.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BuiltInArgument {
    Direction(BuiltInDirection),
    Axis(BuiltInAxis),
    Delta(NonZeroI32),
    Workspace(BuiltInSelector),
    Layout(BuiltInLayout),
    Window(BuiltInSelector),
    Cycle(BuiltInCycle),
    Index(u64),
    Monitor(u64),
    Sizing(BuiltInSizing),
    Adjustment(i32),
    Enabled(bool),
    Size(i32),
    Count(u64),
    Container(u64),
    Columns(NonZeroU64),
    Name(BoundedText),
    Path(WindowsPathInput),
    HidingBehaviour(BuiltInHidingBehaviour),
    MoveBehaviour(BuiltInMoveBehaviour),
    MonocleBehaviour(BuiltInMonocleBehaviour),
    OperationBehaviour(BuiltInOperationBehaviour),
    Implementation(BuiltInImplementation),
    Exe(BoundedText),
    Identifier(BuiltInIdentifier),
    Names(BuiltInNames),
    ColumnRatios(BuiltInRatios),
    RowRatios(BuiltInRatios),
    AtCount(u64),
    ResizeStep(BuiltInResizeStep),
    Alpha(u8),
    WindowKind(BuiltInWindowKind),
    Red(u8),
    Green(u8),
    Blue(u8),
    Width(i32),
    Offset(i32),
    BorderStyle(BuiltInBorderStyle),
    BorderImplementation(BuiltInBorderImplementation),
    StackbarMode(BuiltInStackbarMode),
    StackbarLabel(BuiltInStackbarLabel),
    Height(i32),
    TabWidth(i32),
    FontSize(i32),
    FontFamily(BoundedText),
    AnimationPrefix(BuiltInAnimationPrefix),
    Duration(u64),
    Fps(NonZeroU64),
    AnimationStyle(BuiltInAnimationStyle),
    Left(i32),
    Top(i32),
    Right(i32),
    Bottom(i32),
}

impl BuiltInArgument {
    fn encode(self) -> Result<(BuiltInParameterId, ActionArgument), ArgumentError> {
        use ActionArgument::Scalar;
        use ArgumentScalar as S;

        Ok(match self {
            Self::Direction(value) => choice(BuiltInParameterId::Direction, value.into_wire()),
            Self::Axis(value) => choice(BuiltInParameterId::Axis, value.into_wire()),
            Self::Delta(value) => signed(BuiltInParameterId::Delta, value.get()),
            Self::Workspace(value) => selector(BuiltInParameterId::Workspace, value.into_wire()),
            Self::Layout(value) => choice(BuiltInParameterId::Layout, value.into_wire()),
            Self::Window(value) => selector(BuiltInParameterId::Window, value.into_wire()),
            Self::Cycle(value) => choice(BuiltInParameterId::Cycle, value.into_wire()),
            Self::Index(value) => (BuiltInParameterId::Index, Scalar(S::Unsigned(value))),
            Self::Monitor(value) => (BuiltInParameterId::Monitor, Scalar(S::Unsigned(value))),
            Self::Sizing(value) => choice(BuiltInParameterId::Sizing, value.into_wire()),
            Self::Adjustment(value) => signed(BuiltInParameterId::Adjustment, value),
            Self::Enabled(value) => (BuiltInParameterId::Enabled, Scalar(S::Bool(value))),
            Self::Size(value) => signed(BuiltInParameterId::Size, value),
            Self::Count(value) => (BuiltInParameterId::Count, Scalar(S::Unsigned(value))),
            Self::Container(value) => (BuiltInParameterId::Container, Scalar(S::Unsigned(value))),
            Self::Columns(value) => (
                BuiltInParameterId::Columns,
                Scalar(S::Unsigned(value.get())),
            ),
            Self::Name(value) => (BuiltInParameterId::Name, Scalar(S::Text(value))),
            Self::Path(value) => (BuiltInParameterId::Path, Scalar(S::WindowsPath(value))),
            Self::HidingBehaviour(value) => {
                choice(BuiltInParameterId::Behaviour, value.into_wire())
            }
            Self::MoveBehaviour(value) => choice(BuiltInParameterId::Behaviour, value.into_wire()),
            Self::MonocleBehaviour(value) => {
                choice(BuiltInParameterId::Behaviour, value.into_wire())
            }
            Self::OperationBehaviour(value) => {
                choice(BuiltInParameterId::Behaviour, value.into_wire())
            }
            Self::Implementation(value) => {
                choice(BuiltInParameterId::Implementation, value.into_wire())
            }
            Self::Exe(value) => (BuiltInParameterId::Exe, Scalar(S::Text(value))),
            Self::Identifier(value) => choice(BuiltInParameterId::Identifier, value.into_wire()),
            Self::Names(value) => (BuiltInParameterId::Names, ActionArgument::Scalars(value.0)),
            Self::ColumnRatios(value) => (
                BuiltInParameterId::ColumnRatios,
                ActionArgument::Scalars(value.0),
            ),
            Self::RowRatios(value) => (
                BuiltInParameterId::RowRatios,
                ActionArgument::Scalars(value.0),
            ),
            Self::AtCount(value) => (BuiltInParameterId::AtCount, Scalar(S::Unsigned(value))),
            Self::ResizeStep(value) => signed(BuiltInParameterId::ResizeStep, value.get()),
            Self::Alpha(value) => (
                BuiltInParameterId::Alpha,
                Scalar(S::Unsigned(u64::from(value))),
            ),
            Self::WindowKind(value) => choice(BuiltInParameterId::WindowKind, value.into_wire()),
            Self::Red(value) => unsigned_u8(BuiltInParameterId::Red, value),
            Self::Green(value) => unsigned_u8(BuiltInParameterId::Green, value),
            Self::Blue(value) => unsigned_u8(BuiltInParameterId::Blue, value),
            Self::Width(value) => signed(BuiltInParameterId::Width, value),
            Self::Offset(value) => signed(BuiltInParameterId::Offset, value),
            Self::BorderStyle(value) => choice(BuiltInParameterId::Style, value.into_wire()),
            Self::BorderImplementation(value) => {
                choice(BuiltInParameterId::Implementation, value.into_wire())
            }
            Self::StackbarMode(value) => choice(BuiltInParameterId::Mode, value.into_wire()),
            Self::StackbarLabel(value) => choice(BuiltInParameterId::Label, value.into_wire()),
            Self::Height(value) => signed(BuiltInParameterId::Height, value),
            Self::TabWidth(value) => signed(BuiltInParameterId::TabWidth, value),
            Self::FontSize(value) => signed(BuiltInParameterId::FontSize, value),
            Self::FontFamily(value) => (BuiltInParameterId::FontFamily, Scalar(S::Text(value))),
            Self::AnimationPrefix(value) => choice(BuiltInParameterId::Prefix, value.into_wire()),
            Self::Duration(value) => (BuiltInParameterId::Duration, Scalar(S::Unsigned(value))),
            Self::Fps(value) => (BuiltInParameterId::Fps, Scalar(S::Unsigned(value.get()))),
            Self::AnimationStyle(value) => (
                BuiltInParameterId::Style,
                ActionArgument::Scalars(value.try_into_scalars()?),
            ),
            Self::Left(value) => signed(BuiltInParameterId::Left, value),
            Self::Top(value) => signed(BuiltInParameterId::Top, value),
            Self::Right(value) => signed(BuiltInParameterId::Right, value),
            Self::Bottom(value) => signed(BuiltInParameterId::Bottom, value),
        })
    }
}

fn unsigned_u8(id: BuiltInParameterId, value: u8) -> (BuiltInParameterId, ActionArgument) {
    (
        id,
        ActionArgument::Scalar(ArgumentScalar::Unsigned(u64::from(value))),
    )
}

fn choice(id: BuiltInParameterId, value: ChoiceId) -> (BuiltInParameterId, ActionArgument) {
    (id, ActionArgument::Scalar(ArgumentScalar::Choice(value)))
}

fn selector(id: BuiltInParameterId, value: SelectorId) -> (BuiltInParameterId, ActionArgument) {
    (id, ActionArgument::Scalar(ArgumentScalar::Selector(value)))
}

fn signed(id: BuiltInParameterId, value: i32) -> (BuiltInParameterId, ActionArgument) {
    (
        id,
        ActionArgument::Scalar(ArgumentScalar::Signed(i64::from(value))),
    )
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BuiltInArguments(ActionArguments);

impl BuiltInArguments {
    /// Creates a canonical built-in argument map without duplicate parameters.
    ///
    /// # Errors
    ///
    /// Returns [`BuiltInArgumentsError`] for duplicate parameters or protocol
    /// bound violations.
    pub fn new(
        arguments: impl IntoIterator<Item = BuiltInArgument>,
    ) -> Result<Self, BuiltInArgumentsError> {
        let mut values = BTreeMap::new();
        for argument in arguments {
            let (id, value) = argument.encode()?;
            if values.insert(id.into_wire(), value).is_some() {
                return Err(BuiltInArgumentsError::Duplicate(id));
            }
        }
        Ok(Self(ActionArguments::new(values)?))
    }

    #[must_use]
    pub fn into_action_arguments(self) -> ActionArguments {
        self.0
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum BuiltInArgumentsError {
    #[error("built-in parameter {0:?} was supplied more than once")]
    Duplicate(BuiltInParameterId),
    #[error("built-in arguments violate protocol bounds: {0}")]
    Protocol(#[from] ArgumentError),
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn every_parameter_identity_is_unique_and_valid() {
        let identities = BuiltInParameterId::ALL
            .iter()
            .map(|id| id.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(identities.len(), BuiltInParameterId::ALL.len());
        for id in BuiltInParameterId::ALL {
            assert_eq!(id.into_wire().as_str(), id.as_str());
        }
    }

    #[test]
    fn argument_identity_and_scalar_shape_are_coupled() -> Result<(), Box<dyn std::error::Error>> {
        let arguments = BuiltInArguments::new([
            BuiltInArgument::Direction(BuiltInDirection::Left),
            BuiltInArgument::Delta(NonZeroI32::new(1).ok_or("nonzero test delta")?),
        ])?
        .into_action_arguments();

        assert_eq!(arguments.values().len(), 2);
        assert!(matches!(
            arguments
                .values()
                .get(&ParameterId::parse("direction")?),
            Some(ActionArgument::Scalar(ArgumentScalar::Choice(value)))
                if value.as_str() == "left"
        ));
        assert!(matches!(
            arguments.values().get(&ParameterId::parse("delta")?),
            Some(ActionArgument::Scalar(ArgumentScalar::Signed(1)))
        ));
        Ok(())
    }

    #[test]
    fn duplicate_parameter_is_rejected() {
        assert_eq!(
            BuiltInArguments::new([BuiltInArgument::Index(1), BuiltInArgument::Index(2),]),
            Err(BuiltInArgumentsError::Duplicate(BuiltInParameterId::Index))
        );
    }

    #[test]
    fn resize_step_is_positive_and_encodes_as_its_own_signed_parameter()
    -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            BuiltInResizeStep::new(37).map(BuiltInResizeStep::get),
            Ok(37)
        );
        assert_eq!(BuiltInResizeStep::new(0), Err(BuiltInResizeStepError(0)));
        assert_eq!(BuiltInResizeStep::new(-1), Err(BuiltInResizeStepError(-1)));

        let arguments =
            BuiltInArguments::new([BuiltInArgument::ResizeStep(BuiltInResizeStep::new(37)?)])?
                .into_action_arguments();
        assert!(matches!(
            arguments.values().get(&ParameterId::parse("resize-step")?),
            Some(ActionArgument::Scalar(ArgumentScalar::Signed(37)))
        ));
        Ok(())
    }

    #[test]
    fn transparency_alpha_encodes_as_a_bounded_unsigned_parameter()
    -> Result<(), Box<dyn std::error::Error>> {
        let arguments =
            BuiltInArguments::new([BuiltInArgument::Alpha(200)])?.into_action_arguments();
        assert!(matches!(
            arguments.values().get(&ParameterId::parse("alpha")?),
            Some(ActionArgument::Scalar(ArgumentScalar::Unsigned(200)))
        ));
        Ok(())
    }

    #[test]
    fn border_arguments_preserve_signed_geometry_and_bounded_channels()
    -> Result<(), Box<dyn std::error::Error>> {
        let arguments = BuiltInArguments::new([
            BuiltInArgument::WindowKind(BuiltInWindowKind::UnfocusedLocked),
            BuiltInArgument::Red(1),
            BuiltInArgument::Green(2),
            BuiltInArgument::Blue(3),
            BuiltInArgument::Width(-50),
            BuiltInArgument::Offset(50),
        ])?
        .into_action_arguments();

        assert!(matches!(
            arguments.values().get(&ParameterId::parse("window-kind")?),
            Some(ActionArgument::Scalar(ArgumentScalar::Choice(value)))
                if value.as_str() == "unfocused-locked"
        ));
        assert!(matches!(
            arguments.values().get(&ParameterId::parse("red")?),
            Some(ActionArgument::Scalar(ArgumentScalar::Unsigned(1)))
        ));
        assert!(matches!(
            arguments.values().get(&ParameterId::parse("width")?),
            Some(ActionArgument::Scalar(ArgumentScalar::Signed(-50)))
        ));
        assert!(matches!(
            arguments.values().get(&ParameterId::parse("offset")?),
            Some(ActionArgument::Scalar(ArgumentScalar::Signed(50)))
        ));
        Ok(())
    }

    #[test]
    fn animation_style_preserves_exact_cubic_bezier_coordinates()
    -> Result<(), Box<dyn std::error::Error>> {
        let points = [
            FixedDecimal::new(25, 2)?,
            FixedDecimal::new(-5, 1)?,
            FixedDecimal::new(75, 2)?,
            FixedDecimal::new(125, 2)?,
        ];
        let arguments = BuiltInArguments::new([BuiltInArgument::AnimationStyle(
            BuiltInAnimationStyle::CubicBezier(points),
        )])?
        .into_action_arguments();
        assert!(matches!(
            arguments.values().get(&ParameterId::parse("style")?),
            Some(ActionArgument::Scalars(values))
                if values.values() == points.map(ArgumentScalar::Decimal)
        ));
        Ok(())
    }
}

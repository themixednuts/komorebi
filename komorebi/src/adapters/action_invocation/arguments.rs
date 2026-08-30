use std::num::NonZeroUsize;

use komorebi_protocol as protocol;
use thiserror::Error;

use crate::action::Pixels;
use crate::action::WindowSelector;
use crate::action::WindowsPath;
use crate::action::WorkspaceName;
use crate::action::WorkspaceSelector;
use crate::action::definition::ActionDefinition;
use crate::action::definition::ArgumentCardinality;
use crate::action::id::ParameterId;
use crate::core::ApplicationIdentifier;
use crate::core::Axis;
use crate::core::BorderImplementation;
use crate::core::BorderStyle;
use crate::core::CycleDirection;
use crate::core::DefaultLayout;
use crate::core::FocusFollowsMouseImplementation;
use crate::core::HidingBehaviour;
use crate::core::MonocleFocusBehaviour;
use crate::core::MoveBehaviour;
use crate::core::OperationBehaviour;
use crate::core::OperationDirection;
use crate::core::ResizeStep;
use crate::core::Sizing;
use crate::core::WindowKind;

pub(super) struct ValidatedArguments<'a> {
    values: &'a protocol::ActionArguments,
}

impl<'a> ValidatedArguments<'a> {
    pub(super) fn new(
        definition: &ActionDefinition,
        values: &'a protocol::ActionArguments,
    ) -> Result<Self, ArgumentBindingError> {
        for parameter in definition.parameters {
            let value = find(values, parameter.id);
            match (parameter.cardinality, value) {
                (ArgumentCardinality::RequiredScalar | ArgumentCardinality::RequiredList, None) => {
                    return Err(ArgumentBindingError::Missing(parameter.id));
                }
                (
                    ArgumentCardinality::RequiredScalar | ArgumentCardinality::OptionalScalar,
                    Some(value),
                ) if !matches!(value, protocol::ActionArgument::Scalar(_)) => {
                    return Err(wrong_cardinality(parameter.id, parameter.cardinality));
                }
                (
                    ArgumentCardinality::RequiredList | ArgumentCardinality::OptionalList,
                    Some(value),
                ) if !matches!(value, protocol::ActionArgument::Scalars(_)) => {
                    return Err(wrong_cardinality(parameter.id, parameter.cardinality));
                }
                _ => {}
            }
        }
        for id in values.values().keys() {
            if !definition
                .parameters
                .iter()
                .any(|parameter| parameter.id.as_str() == id.as_str())
            {
                return Err(ArgumentBindingError::Unexpected(
                    id.as_str().to_owned().into_boxed_str(),
                ));
            }
        }
        Ok(Self { values })
    }

    pub(super) fn boolean(&self, id: ParameterId) -> Result<bool, ArgumentBindingError> {
        match self.scalar(id)? {
            protocol::ArgumentScalar::Bool(value) => Ok(*value),
            _ => Err(wrong_scalar(id, ScalarKind::Boolean)),
        }
    }

    pub(super) fn i32(&self, id: ParameterId) -> Result<i32, ArgumentBindingError> {
        match self.scalar(id)? {
            protocol::ArgumentScalar::Signed(value) => i32::try_from(*value)
                .map_err(|_| ArgumentBindingError::OutsideI32 { parameter: id }),
            _ => Err(wrong_scalar(id, ScalarKind::Signed)),
        }
    }

    pub(super) fn usize(&self, id: ParameterId) -> Result<usize, ArgumentBindingError> {
        match self.scalar(id)? {
            protocol::ArgumentScalar::Unsigned(value) => usize::try_from(*value)
                .map_err(|_| ArgumentBindingError::OutsideUsize { parameter: id }),
            _ => Err(wrong_scalar(id, ScalarKind::Unsigned)),
        }
    }

    pub(super) fn u8(&self, id: ParameterId) -> Result<u8, ArgumentBindingError> {
        match self.scalar(id)? {
            protocol::ArgumentScalar::Unsigned(value) => {
                u8::try_from(*value).map_err(|_| ArgumentBindingError::OutsideU8 { parameter: id })
            }
            _ => Err(wrong_scalar(id, ScalarKind::Unsigned)),
        }
    }

    pub(super) fn nonzero_usize(
        &self,
        id: ParameterId,
    ) -> Result<NonZeroUsize, ArgumentBindingError> {
        NonZeroUsize::new(self.usize(id)?).ok_or(ArgumentBindingError::Zero { parameter: id })
    }

    pub(super) fn text(&self, id: ParameterId) -> Result<&'a str, ArgumentBindingError> {
        match self.scalar(id)? {
            protocol::ArgumentScalar::Text(value) => Ok(value.as_str()),
            _ => Err(wrong_scalar(id, ScalarKind::Text)),
        }
    }

    fn choice(&self, id: ParameterId) -> Result<&'a str, ArgumentBindingError> {
        match self.scalar(id)? {
            protocol::ArgumentScalar::Choice(value) => Ok(value.as_str()),
            _ => Err(wrong_scalar(id, ScalarKind::Choice)),
        }
    }

    fn selector(&self, id: ParameterId) -> Result<&'a str, ArgumentBindingError> {
        match self.scalar(id)? {
            protocol::ArgumentScalar::Selector(value) => Ok(value.as_str()),
            _ => Err(wrong_scalar(id, ScalarKind::Selector)),
        }
    }

    pub(super) fn direction(
        &self,
        id: ParameterId,
    ) -> Result<OperationDirection, ArgumentBindingError> {
        match self.choice(id)? {
            "left" => Ok(OperationDirection::Left),
            "right" => Ok(OperationDirection::Right),
            "up" => Ok(OperationDirection::Up),
            "down" => Ok(OperationDirection::Down),
            value => Err(unknown_choice(id, value)),
        }
    }

    pub(super) fn axis(&self, id: ParameterId) -> Result<Axis, ArgumentBindingError> {
        match self.choice(id)? {
            "horizontal" => Ok(Axis::Horizontal),
            "vertical" => Ok(Axis::Vertical),
            "horizontal-and-vertical" => Ok(Axis::HorizontalAndVertical),
            value => Err(unknown_choice(id, value)),
        }
    }

    pub(super) fn cycle(&self, id: ParameterId) -> Result<CycleDirection, ArgumentBindingError> {
        match self.choice(id)? {
            "previous" => Ok(CycleDirection::Previous),
            "next" => Ok(CycleDirection::Next),
            value => Err(unknown_choice(id, value)),
        }
    }

    pub(super) fn sizing(&self, id: ParameterId) -> Result<Sizing, ArgumentBindingError> {
        match self.choice(id)? {
            "increase" => Ok(Sizing::Increase),
            "decrease" => Ok(Sizing::Decrease),
            value => Err(unknown_choice(id, value)),
        }
    }

    pub(super) fn layout(&self, id: ParameterId) -> Result<DefaultLayout, ArgumentBindingError> {
        match self.choice(id)? {
            "bsp" => Ok(DefaultLayout::BSP),
            "columns" => Ok(DefaultLayout::Columns),
            "rows" => Ok(DefaultLayout::Rows),
            "vertical-stack" => Ok(DefaultLayout::VerticalStack),
            "horizontal-stack" => Ok(DefaultLayout::HorizontalStack),
            "ultrawide-vertical-stack" => Ok(DefaultLayout::UltrawideVerticalStack),
            "grid" => Ok(DefaultLayout::Grid),
            "right-main-vertical-stack" => Ok(DefaultLayout::RightMainVerticalStack),
            "scrolling" => Ok(DefaultLayout::Scrolling),
            value => Err(unknown_choice(id, value)),
        }
    }

    pub(super) fn workspace_selector(
        &self,
        id: ParameterId,
    ) -> Result<WorkspaceSelector, ArgumentBindingError> {
        match self.selector(id)? {
            "focused-at-execution" => Ok(WorkspaceSelector::FocusedAtExecution),
            value => Err(unknown_choice(id, value)),
        }
    }

    pub(super) fn window_selector(
        &self,
        id: ParameterId,
    ) -> Result<WindowSelector, ArgumentBindingError> {
        match self.selector(id)? {
            "focused-at-execution" => Ok(WindowSelector::FocusedAtExecution),
            value => Err(unknown_choice(id, value)),
        }
    }

    pub(super) fn pixels(&self, id: ParameterId) -> Result<Pixels, ArgumentBindingError> {
        Pixels::new(self.i32(id)?).map_err(|_| ArgumentBindingError::Zero { parameter: id })
    }

    pub(super) fn resize_step(&self, id: ParameterId) -> Result<ResizeStep, ArgumentBindingError> {
        ResizeStep::new(self.i32(id)?)
            .map_err(|_| ArgumentBindingError::NonPositive { parameter: id })
    }

    pub(super) fn workspace_name(
        &self,
        id: ParameterId,
    ) -> Result<WorkspaceName, ArgumentBindingError> {
        WorkspaceName::parse(self.text(id)?.to_owned())
            .map_err(|_| ArgumentBindingError::EmptyText { parameter: id })
    }

    pub(super) fn workspace_names(
        &self,
        id: ParameterId,
    ) -> Result<Vec<WorkspaceName>, ArgumentBindingError> {
        self.list(id)?
            .iter()
            .map(|value| match value {
                protocol::ArgumentScalar::Text(value) => WorkspaceName::parse(value.as_str())
                    .map_err(|_| ArgumentBindingError::EmptyText { parameter: id }),
                _ => Err(wrong_scalar(id, ScalarKind::Text)),
            })
            .collect()
    }

    pub(super) fn windows_path(
        &self,
        id: ParameterId,
    ) -> Result<WindowsPath, ArgumentBindingError> {
        match self.scalar(id)? {
            protocol::ArgumentScalar::WindowsPath(value) => WindowsPath::from_wtf16(value.units())
                .map_err(|source| ArgumentBindingError::WindowsPath {
                    parameter: id,
                    source,
                }),
            _ => Err(wrong_scalar(id, ScalarKind::WindowsPath)),
        }
    }

    pub(super) fn ratios(&self, id: ParameterId) -> Result<Option<Vec<f32>>, ArgumentBindingError> {
        self.optional_list(id)?
            .map(|values| bind_ratios(id, values))
            .transpose()
    }

    #[allow(deprecated)]
    pub(super) fn hiding_behaviour(
        &self,
        id: ParameterId,
    ) -> Result<HidingBehaviour, ArgumentBindingError> {
        match self.choice(id)? {
            "hide" => Ok(HidingBehaviour::Hide),
            "minimize" => Ok(HidingBehaviour::Minimize),
            "cloak" => Ok(HidingBehaviour::Cloak),
            value => Err(unknown_choice(id, value)),
        }
    }

    pub(super) fn move_behaviour(
        &self,
        id: ParameterId,
    ) -> Result<MoveBehaviour, ArgumentBindingError> {
        match self.choice(id)? {
            "swap" => Ok(MoveBehaviour::Swap),
            "insert" => Ok(MoveBehaviour::Insert),
            "no-op" => Ok(MoveBehaviour::NoOp),
            value => Err(unknown_choice(id, value)),
        }
    }

    pub(super) fn monocle_behaviour(
        &self,
        id: ParameterId,
    ) -> Result<MonocleFocusBehaviour, ArgumentBindingError> {
        match self.choice(id)? {
            "cycle" => Ok(MonocleFocusBehaviour::Cycle),
            "no-op" => Ok(MonocleFocusBehaviour::NoOp),
            value => Err(unknown_choice(id, value)),
        }
    }

    pub(super) fn operation_behaviour(
        &self,
        id: ParameterId,
    ) -> Result<OperationBehaviour, ArgumentBindingError> {
        match self.choice(id)? {
            "op" => Ok(OperationBehaviour::Op),
            "no-op" => Ok(OperationBehaviour::NoOp),
            value => Err(unknown_choice(id, value)),
        }
    }

    pub(super) fn ffm_implementation(
        &self,
        id: ParameterId,
    ) -> Result<FocusFollowsMouseImplementation, ArgumentBindingError> {
        match self.choice(id)? {
            "komorebi" => Ok(FocusFollowsMouseImplementation::Komorebi),
            "windows" => Ok(FocusFollowsMouseImplementation::Windows),
            value => Err(unknown_choice(id, value)),
        }
    }

    pub(super) fn window_kind(&self, id: ParameterId) -> Result<WindowKind, ArgumentBindingError> {
        match self.choice(id)? {
            "single" => Ok(WindowKind::Single),
            "stack" => Ok(WindowKind::Stack),
            "monocle" => Ok(WindowKind::Monocle),
            "unfocused" => Ok(WindowKind::Unfocused),
            "unfocused-locked" => Ok(WindowKind::UnfocusedLocked),
            "floating" => Ok(WindowKind::Floating),
            value => Err(unknown_choice(id, value)),
        }
    }

    pub(super) fn border_style(
        &self,
        id: ParameterId,
    ) -> Result<BorderStyle, ArgumentBindingError> {
        match self.choice(id)? {
            "system" => Ok(BorderStyle::System),
            "rounded" => Ok(BorderStyle::Rounded),
            "square" => Ok(BorderStyle::Square),
            value => Err(unknown_choice(id, value)),
        }
    }

    pub(super) fn border_implementation(
        &self,
        id: ParameterId,
    ) -> Result<BorderImplementation, ArgumentBindingError> {
        match self.choice(id)? {
            "komorebi" => Ok(BorderImplementation::Komorebi),
            "windows" => Ok(BorderImplementation::Windows),
            value => Err(unknown_choice(id, value)),
        }
    }

    pub(super) fn application_identifier(
        &self,
        id: ParameterId,
    ) -> Result<ApplicationIdentifier, ArgumentBindingError> {
        match self.choice(id)? {
            "exe" => Ok(ApplicationIdentifier::Exe),
            "class" => Ok(ApplicationIdentifier::Class),
            "title" => Ok(ApplicationIdentifier::Title),
            "path" => Ok(ApplicationIdentifier::Path),
            value => Err(unknown_choice(id, value)),
        }
    }

    fn scalar(
        &self,
        id: ParameterId,
    ) -> Result<&'a protocol::ArgumentScalar, ArgumentBindingError> {
        match find(self.values, id) {
            Some(protocol::ActionArgument::Scalar(value)) => Ok(value),
            Some(protocol::ActionArgument::Scalars(_)) => {
                Err(wrong_cardinality(id, ArgumentCardinality::RequiredScalar))
            }
            None => Err(ArgumentBindingError::Missing(id)),
        }
    }

    fn list(
        &self,
        id: ParameterId,
    ) -> Result<&'a [protocol::ArgumentScalar], ArgumentBindingError> {
        match find(self.values, id) {
            Some(protocol::ActionArgument::Scalars(values)) => Ok(values.values()),
            Some(protocol::ActionArgument::Scalar(_)) => {
                Err(wrong_cardinality(id, ArgumentCardinality::RequiredList))
            }
            None => Err(ArgumentBindingError::Missing(id)),
        }
    }

    fn optional_list(
        &self,
        id: ParameterId,
    ) -> Result<Option<&'a [protocol::ArgumentScalar]>, ArgumentBindingError> {
        match find(self.values, id) {
            Some(protocol::ActionArgument::Scalars(values)) => Ok(Some(values.values())),
            Some(protocol::ActionArgument::Scalar(_)) => {
                Err(wrong_cardinality(id, ArgumentCardinality::OptionalList))
            }
            None => Ok(None),
        }
    }
}

fn find(values: &protocol::ActionArguments, id: ParameterId) -> Option<&protocol::ActionArgument> {
    values
        .values()
        .iter()
        .find(|(candidate, _)| candidate.as_str() == id.as_str())
        .map(|(_, value)| value)
}

const RATIO_SCALE: u32 = 18;
const RATIO_DENOMINATOR: i128 = 10_i128.pow(RATIO_SCALE);
const MIN_RATIO_UNITS: i128 = RATIO_DENOMINATOR / 10;
const MAX_RATIO_UNITS: i128 = RATIO_DENOMINATOR * 9 / 10;

fn bind_ratios(
    parameter: ParameterId,
    values: &[protocol::ArgumentScalar],
) -> Result<Vec<f32>, ArgumentBindingError> {
    if values.len() > komorebi_layouts::MAX_RATIOS {
        return Err(ArgumentBindingError::InvalidRatio { parameter });
    }

    let mut total = 0_i128;
    values
        .iter()
        .map(|value| {
            let protocol::ArgumentScalar::Decimal(value) = value else {
                return Err(wrong_scalar(parameter, ScalarKind::Decimal));
            };
            let (ratio, units) = decimal_ratio(parameter, *value)?;
            total += units;
            if total >= RATIO_DENOMINATOR {
                return Err(ArgumentBindingError::InvalidRatio { parameter });
            }
            Ok(ratio)
        })
        .collect()
}

fn decimal_ratio(
    parameter: ParameterId,
    value: protocol::FixedDecimal,
) -> Result<(f32, i128), ArgumentBindingError> {
    let scale_multiplier = 10_i128.pow(RATIO_SCALE - u32::from(value.scale()));
    let units = i128::from(value.coefficient()) * scale_multiplier;
    if !(MIN_RATIO_UNITS..=MAX_RATIO_UNITS).contains(&units) {
        return Err(ArgumentBindingError::InvalidRatio { parameter });
    }
    let ratio = units as f64 / RATIO_DENOMINATOR as f64;
    #[allow(clippy::cast_possible_truncation)]
    let ratio = ratio as f32;
    if !(komorebi_layouts::MIN_RATIO..=komorebi_layouts::MAX_RATIO).contains(&ratio) {
        return Err(ArgumentBindingError::InvalidRatio { parameter });
    }
    Ok((ratio, units))
}

fn unknown_choice(parameter: ParameterId, value: &str) -> ArgumentBindingError {
    ArgumentBindingError::UnknownChoice {
        parameter,
        value: value.to_owned().into_boxed_str(),
    }
}

const fn wrong_scalar(parameter: ParameterId, expected: ScalarKind) -> ArgumentBindingError {
    ArgumentBindingError::WrongScalar {
        parameter,
        expected,
    }
}

const fn wrong_cardinality(
    parameter: ParameterId,
    expected: ArgumentCardinality,
) -> ArgumentBindingError {
    ArgumentBindingError::WrongCardinality {
        parameter,
        expected,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScalarKind {
    Boolean,
    Signed,
    Unsigned,
    Decimal,
    Text,
    Choice,
    Selector,
    WindowsPath,
}

#[derive(Debug, Error)]
pub enum ArgumentBindingError {
    #[error("missing action parameter {0}")]
    Missing(ParameterId),
    #[error("unexpected action parameter {0}")]
    Unexpected(Box<str>),
    #[error("parameter {parameter} has the wrong cardinality; expected {expected:?}")]
    WrongCardinality {
        parameter: ParameterId,
        expected: ArgumentCardinality,
    },
    #[error("parameter {parameter} has the wrong scalar kind; expected {expected:?}")]
    WrongScalar {
        parameter: ParameterId,
        expected: ScalarKind,
    },
    #[error("parameter {parameter} has unknown choice {value}")]
    UnknownChoice {
        parameter: ParameterId,
        value: Box<str>,
    },
    #[error("parameter {parameter} does not fit a signed 32-bit value")]
    OutsideI32 { parameter: ParameterId },
    #[error("parameter {parameter} does not fit this process address space")]
    OutsideUsize { parameter: ParameterId },
    #[error("parameter {parameter} does not fit an unsigned 8-bit value")]
    OutsideU8 { parameter: ParameterId },
    #[error("parameter {parameter} must be nonzero")]
    Zero { parameter: ParameterId },
    #[error("parameter {parameter} must be positive")]
    NonPositive { parameter: ParameterId },
    #[error("parameter {parameter} must not be empty")]
    EmptyText { parameter: ParameterId },
    #[error(
        "parameter {parameter} must contain at most five ratios in [0.1, 0.9] whose sum is below 1"
    )]
    InvalidRatio { parameter: ParameterId },
    #[error("parameter {parameter} is not a valid lossless Windows path: {source}")]
    WindowsPath {
        parameter: ParameterId,
        #[source]
        source: crate::action::WindowsPathError,
    },
}

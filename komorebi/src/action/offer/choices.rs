use crate::core::OperationDirection;

use super::ActionSnapshot;
use crate::action::ParameterId;
use crate::action::WorkspaceName;

use super::policy::DynamicChoiceSource;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DynamicParameterChoice {
    Direction(OperationDirection),
    WorkspaceName(WorkspaceName),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DynamicParameterChoices {
    pub parameter: ParameterId,
    pub choices: Vec<DynamicParameterChoice>,
}

pub(super) fn dynamic_choices(
    snapshot: &ActionSnapshot,
    source: DynamicChoiceSource,
) -> Vec<DynamicParameterChoices> {
    let choices = match source {
        DynamicChoiceSource::DirectionalTarget => Some(DynamicParameterChoices {
            parameter: ParameterId::DIRECTION,
            choices: directional_targets(snapshot),
        }),
        DynamicChoiceSource::ExistingWorkspaceName => Some(DynamicParameterChoices {
            parameter: ParameterId::NAME,
            choices: snapshot
                .named_workspaces
                .iter()
                .map(|target| DynamicParameterChoice::WorkspaceName(target.name.clone()))
                .collect(),
        }),
        DynamicChoiceSource::None => None,
    };
    choices
        .filter(|choices| !choices.choices.is_empty())
        .into_iter()
        .collect()
}

fn directional_targets(snapshot: &ActionSnapshot) -> Vec<DynamicParameterChoice> {
    snapshot
        .directional_targets
        .iter()
        .map(DynamicParameterChoice::Direction)
        .collect()
}

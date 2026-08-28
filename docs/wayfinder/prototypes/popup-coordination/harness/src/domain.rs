use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct WindowId(pub [u8; 32]);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ObservationRevision(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct SurfaceGeneration(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct PhysicalRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl PhysicalRect {
    pub fn new(left: i32, top: i32, right: i32, bottom: i32) -> Result<Self, InvalidRect> {
        if left >= right || top >= bottom {
            return Err(InvalidRect);
        }
        Ok(Self {
            left,
            top,
            right,
            bottom,
        })
    }

    pub fn width(self) -> Result<i32, PlacementUnavailable> {
        self.right
            .checked_sub(self.left)
            .ok_or(PlacementUnavailable::Arithmetic)
    }

    pub fn height(self) -> Result<i32, PlacementUnavailable> {
        self.bottom
            .checked_sub(self.top)
            .ok_or(PlacementUnavailable::Arithmetic)
    }

    pub fn translated(self, left: i32, top: i32) -> Result<Self, PlacementUnavailable> {
        let width = self.width()?;
        let height = self.height()?;
        Ok(Self {
            left,
            top,
            right: left
                .checked_add(width)
                .ok_or(PlacementUnavailable::Arithmetic)?,
            bottom: top
                .checked_add(height)
                .ok_or(PlacementUnavailable::Arithmetic)?,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability<T> {
    Known(T),
    Unavailable(UnavailableFact),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnavailableFact {
    AccessDenied,
    ProviderUnsupported,
    ProviderTimedOut,
    ProviderFailed,
    StaleWindow,
    MissingOwner,
    ContradictoryOwner,
    EventGap,
    SecureDesktop,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerLink {
    Root,
    OwnedBy(WindowId),
    Unresolved(UnavailableFact),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfiguredRole {
    Dialog,
    Utility,
    Menu,
    Tooltip,
    ComboPopup,
    DragVisual,
    System,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct StyleEvidence(u8);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StyleFlag {
    DialogFrame,
    ToolWindow,
    NoActivate,
    AppWindow,
    Topmost,
}

impl StyleEvidence {
    pub const EMPTY: Self = Self(0);

    #[must_use]
    pub const fn with(self, flag: StyleFlag) -> Self {
        Self(self.0 | style_flag_bit(flag))
    }

    pub const fn has(self, flag: StyleFlag) -> bool {
        self.0 & style_flag_bit(flag) != 0
    }
}

const fn style_flag_bit(flag: StyleFlag) -> u8 {
    match flag {
        StyleFlag::DialogFrame => 1 << 0,
        StyleFlag::ToolWindow => 1 << 1,
        StyleFlag::NoActivate => 1 << 2,
        StyleFlag::AppWindow => 1 << 3,
        StyleFlag::Topmost => 1 << 4,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Visible,
    Hidden,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EnabledState {
    Enabled,
    Disabled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceProvenance {
    External,
    Protected,
    ManagerOwned,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerGraphState {
    Complete,
    Incomplete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UiaControlType {
    Window,
    Menu,
    ToolTip,
    Pane,
    Other(i32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct UiaFacts {
    pub control_type: UiaControlType,
    pub is_modal: Availability<bool>,
    pub window_pattern: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum UiaEvidence {
    Known(UiaFacts),
    Unavailable(UnavailableFact),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SurfaceObservation {
    pub window: WindowId,
    pub revision: ObservationRevision,
    pub generation: SurfaceGeneration,
    pub owner: OwnerLink,
    pub root_owner: Availability<WindowId>,
    pub owner_enabled: Availability<bool>,
    pub visibility: Visibility,
    pub enabled: EnabledState,
    pub cloaked: Availability<bool>,
    pub style: StyleEvidence,
    pub frame: Availability<PhysicalRect>,
    pub work_area: Availability<PhysicalRect>,
    pub dpi: Availability<u32>,
    pub uia: UiaEvidence,
    pub configured_role: Option<ConfiguredRole>,
    pub provenance: SurfaceProvenance,
    pub owner_graph: OwnerGraphState,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DialogRole {
    Modal,
    Modeless,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceRole {
    Primary,
    Dialog(DialogRole),
    Utility,
    Menu,
    Tooltip,
    ComboPopup,
    DragVisual,
    System,
    UnknownTransient,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoordinationMode {
    OrdinaryManaged,
    AttachedFloat,
    IndependentFloat,
    ObserveOnly,
    Excluded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementIntent {
    PreserveApplicationPlacement,
    CenterOnOwner,
    RecoverIntoWorkArea,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SurfaceDecision {
    pub window: WindowId,
    pub family: Availability<WindowId>,
    pub generation: SurfaceGeneration,
    pub role: SurfaceRole,
    pub mode: CoordinationMode,
    pub placement: PlacementIntent,
    pub missing: BTreeSet<UnavailableFact>,
    pub reasons: Vec<&'static str>,
}

#[allow(
    clippy::too_many_lines,
    reason = "one exhaustive pure decision table keeps role precedence visible and reviewable"
)]
pub fn classify_surface(observation: &SurfaceObservation) -> SurfaceDecision {
    let mut missing = unavailable_facts(observation);
    let family = family_of(observation, &mut missing);
    let (role, mode, reasons) = if observation.provenance != SurfaceProvenance::External {
        (
            SurfaceRole::System,
            CoordinationMode::Excluded,
            vec!["protected or manager-owned surface"],
        )
    } else if matches!(observation.configured_role, Some(ConfiguredRole::System)) {
        (
            SurfaceRole::System,
            CoordinationMode::Excluded,
            vec!["explicit protected-system fixture"],
        )
    } else if matches!(observation.configured_role, Some(ConfiguredRole::Menu))
        || matches!(uia_control_type(observation), Some(UiaControlType::Menu))
    {
        (
            SurfaceRole::Menu,
            CoordinationMode::ObserveOnly,
            vec!["menu evidence is never tile eligibility"],
        )
    } else if matches!(observation.configured_role, Some(ConfiguredRole::Tooltip))
        || matches!(uia_control_type(observation), Some(UiaControlType::ToolTip))
    {
        (
            SurfaceRole::Tooltip,
            CoordinationMode::ObserveOnly,
            vec!["tooltip evidence is never tile eligibility"],
        )
    } else if matches!(
        observation.configured_role,
        Some(ConfiguredRole::ComboPopup)
    ) {
        (
            SurfaceRole::ComboPopup,
            CoordinationMode::ObserveOnly,
            vec!["combo popup is application-owned transient UI"],
        )
    } else if matches!(
        observation.configured_role,
        Some(ConfiguredRole::DragVisual)
    ) {
        (
            SurfaceRole::DragVisual,
            CoordinationMode::ObserveOnly,
            vec!["drag visual is application-owned transient UI"],
        )
    } else if matches!(observation.configured_role, Some(ConfiguredRole::Utility)) {
        let mode = if matches!(observation.owner, OwnerLink::OwnedBy(_))
            && !observation.style.has(StyleFlag::NoActivate)
        {
            CoordinationMode::AttachedFloat
        } else {
            CoordinationMode::ObserveOnly
        };
        (
            SurfaceRole::Utility,
            mode,
            vec!["tool-window evidence does not create workspace identity"],
        )
    } else if dialog_evidence(observation) {
        let dialog = dialog_role(observation, &mut missing);
        let mode = match observation.owner {
            OwnerLink::OwnedBy(_) => CoordinationMode::AttachedFloat,
            OwnerLink::Root if missing.is_empty() => CoordinationMode::IndependentFloat,
            OwnerLink::Root | OwnerLink::Unresolved(_) => CoordinationMode::ObserveOnly,
        };
        (
            SurfaceRole::Dialog(dialog),
            mode,
            vec!["dialog evidence produces a float, never an ordinary tile"],
        )
    } else if observation.style.has(StyleFlag::ToolWindow) {
        let mode = if matches!(observation.owner, OwnerLink::OwnedBy(_))
            && !observation.style.has(StyleFlag::NoActivate)
        {
            CoordinationMode::AttachedFloat
        } else {
            CoordinationMode::ObserveOnly
        };
        (
            SurfaceRole::Utility,
            mode,
            vec!["tool-window evidence does not create workspace identity"],
        )
    } else if observation.style.has(StyleFlag::NoActivate)
        || observation.owner_graph == OwnerGraphState::Incomplete
        || !missing.is_empty()
    {
        (
            SurfaceRole::UnknownTransient,
            CoordinationMode::ObserveOnly,
            vec!["uncertain or no-activate surface remains visible and untouched"],
        )
    } else {
        match observation.owner {
            OwnerLink::Root => (
                SurfaceRole::Primary,
                CoordinationMode::OrdinaryManaged,
                vec!["complete unowned top-level surface"],
            ),
            OwnerLink::OwnedBy(_) | OwnerLink::Unresolved(_) => (
                SurfaceRole::UnknownTransient,
                CoordinationMode::ObserveOnly,
                vec!["owned surface without positive role evidence stays observe-only"],
            ),
        }
    };

    SurfaceDecision {
        window: observation.window,
        family,
        generation: observation.generation,
        role,
        mode,
        placement: PlacementIntent::PreserveApplicationPlacement,
        missing,
        reasons,
    }
}

fn family_of(
    observation: &SurfaceObservation,
    missing: &mut BTreeSet<UnavailableFact>,
) -> Availability<WindowId> {
    match (observation.owner, observation.root_owner) {
        (OwnerLink::Root, Availability::Known(root)) if root == observation.window => {
            Availability::Known(root)
        }
        (OwnerLink::OwnedBy(_), Availability::Known(root)) if root != observation.window => {
            Availability::Known(root)
        }
        (OwnerLink::Unresolved(reason), _) | (_, Availability::Unavailable(reason)) => {
            missing.insert(reason);
            Availability::Unavailable(reason)
        }
        _ => {
            missing.insert(UnavailableFact::ContradictoryOwner);
            Availability::Unavailable(UnavailableFact::ContradictoryOwner)
        }
    }
}

fn unavailable_facts(observation: &SurfaceObservation) -> BTreeSet<UnavailableFact> {
    let mut missing = BTreeSet::new();
    for availability in [observation.cloaked.map_unit(), observation.dpi.map_unit()] {
        if let Availability::Unavailable(reason) = availability {
            missing.insert(reason);
        }
    }
    if let UiaEvidence::Unavailable(reason) = observation.uia {
        missing.insert(reason);
    }
    if observation.owner_graph == OwnerGraphState::Incomplete {
        missing.insert(UnavailableFact::EventGap);
    }
    missing
}

impl<T> Availability<T> {
    fn map_unit(self) -> Availability<()> {
        match self {
            Self::Known(_) => Availability::Known(()),
            Self::Unavailable(reason) => Availability::Unavailable(reason),
        }
    }
}

fn uia_control_type(observation: &SurfaceObservation) -> Option<UiaControlType> {
    match observation.uia {
        UiaEvidence::Known(facts) => Some(facts.control_type),
        UiaEvidence::Unavailable(_) => None,
    }
}

fn dialog_evidence(observation: &SurfaceObservation) -> bool {
    matches!(observation.configured_role, Some(ConfiguredRole::Dialog))
        || observation.style.has(StyleFlag::DialogFrame)
        || matches!(
            observation.uia,
            UiaEvidence::Known(UiaFacts {
                control_type: UiaControlType::Window,
                is_modal: Availability::Known(true),
                ..
            })
        )
}

fn dialog_role(
    observation: &SurfaceObservation,
    missing: &mut BTreeSet<UnavailableFact>,
) -> DialogRole {
    if matches!(
        observation.uia,
        UiaEvidence::Known(UiaFacts {
            is_modal: Availability::Known(true),
            ..
        })
    ) || matches!(observation.owner_enabled, Availability::Known(false))
        && matches!(observation.owner, OwnerLink::OwnedBy(_))
        && observation.visibility == Visibility::Visible
    {
        return DialogRole::Modal;
    }
    match observation.uia {
        UiaEvidence::Known(UiaFacts {
            is_modal: Availability::Known(false),
            ..
        }) if observation.owner_graph == OwnerGraphState::Complete => DialogRole::Modeless,
        UiaEvidence::Known(UiaFacts {
            is_modal: Availability::Unavailable(reason),
            ..
        })
        | UiaEvidence::Unavailable(reason) => {
            missing.insert(reason);
            DialogRole::Unknown
        }
        UiaEvidence::Known(_) => DialogRole::Unknown,
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModalConstraint {
    NoneObserved { at: ObservationRevision },
    Active { dialog: WindowId },
    Unresolved { reasons: BTreeSet<UnavailableFact> },
}

pub fn evaluate_modal_constraint(
    revision: ObservationRevision,
    decisions: &[SurfaceDecision],
) -> ModalConstraint {
    if let Some(dialog) = decisions.iter().find(|decision| {
        matches!(decision.role, SurfaceRole::Dialog(DialogRole::Modal))
            && decision.mode == CoordinationMode::AttachedFloat
    }) {
        return ModalConstraint::Active {
            dialog: dialog.window,
        };
    }
    let reasons = decisions
        .iter()
        .filter(|decision| {
            matches!(
                decision.role,
                SurfaceRole::Dialog(DialogRole::Unknown) | SurfaceRole::UnknownTransient
            )
        })
        .flat_map(|decision| decision.missing.iter().copied())
        .collect::<BTreeSet<_>>();
    if reasons.is_empty() {
        ModalConstraint::NoneObserved { at: revision }
    } else {
        ModalConstraint::Unresolved { reasons }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FamilyAction {
    Hide,
    MoveWorkspace,
    MoveMonitor,
    AdoptScratchpad,
    TransferDesktop,
    MinimizeRoot,
    CloseRoot,
    FocusActiveDialog,
    Inspect,
}

impl FamilyAction {
    const fn can_strand_dialog(self) -> bool {
        matches!(
            self,
            Self::Hide
                | Self::MoveWorkspace
                | Self::MoveMonitor
                | Self::AdoptScratchpad
                | Self::TransferDesktop
                | Self::MinimizeRoot
                | Self::CloseRoot
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardDecision {
    Allowed,
    ActiveModal { dialog: WindowId },
    Unresolved { reasons: BTreeSet<UnavailableFact> },
}

pub fn guard_family(constraint: &ModalConstraint, action: FamilyAction) -> GuardDecision {
    if !action.can_strand_dialog() {
        return GuardDecision::Allowed;
    }
    match constraint {
        ModalConstraint::NoneObserved { .. } => GuardDecision::Allowed,
        ModalConstraint::Active { dialog } => GuardDecision::ActiveModal { dialog: *dialog },
        ModalConstraint::Unresolved { reasons } => GuardDecision::Unresolved {
            reasons: reasons.clone(),
        },
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct PlacementRequest {
    pub window: WindowId,
    pub generation: SurfaceGeneration,
    pub current: PhysicalRect,
    pub owner: PhysicalRect,
    pub work_area: PhysicalRect,
    pub intent: PlacementIntent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct PlacementPlan {
    pub window: WindowId,
    pub generation: SurfaceGeneration,
    pub original: PhysicalRect,
    pub target: PhysicalRect,
}

pub fn plan_placement(
    request: PlacementRequest,
) -> Result<Option<PlacementPlan>, PlacementUnavailable> {
    let width = request.current.width()?;
    let height = request.current.height()?;
    if width > request.work_area.width()? || height > request.work_area.height()? {
        return Err(PlacementUnavailable::LargerThanWorkArea);
    }
    let target = match request.intent {
        PlacementIntent::PreserveApplicationPlacement => return Ok(None),
        PlacementIntent::CenterOnOwner => {
            let owner_center_x = request
                .owner
                .left
                .checked_add(
                    request
                        .owner
                        .width()?
                        .checked_div(2)
                        .ok_or(PlacementUnavailable::Arithmetic)?,
                )
                .ok_or(PlacementUnavailable::Arithmetic)?;
            let owner_center_y = request
                .owner
                .top
                .checked_add(
                    request
                        .owner
                        .height()?
                        .checked_div(2)
                        .ok_or(PlacementUnavailable::Arithmetic)?,
                )
                .ok_or(PlacementUnavailable::Arithmetic)?;
            let left = owner_center_x
                .checked_sub(
                    width
                        .checked_div(2)
                        .ok_or(PlacementUnavailable::Arithmetic)?,
                )
                .ok_or(PlacementUnavailable::Arithmetic)?;
            let top = owner_center_y
                .checked_sub(
                    height
                        .checked_div(2)
                        .ok_or(PlacementUnavailable::Arithmetic)?,
                )
                .ok_or(PlacementUnavailable::Arithmetic)?;
            clamp_into_work_area(request.current.translated(left, top)?, request.work_area)?
        }
        PlacementIntent::RecoverIntoWorkArea => {
            clamp_into_work_area(request.current, request.work_area)?
        }
    };
    Ok(Some(PlacementPlan {
        window: request.window,
        generation: request.generation,
        original: request.current,
        target,
    }))
}

fn clamp_into_work_area(
    frame: PhysicalRect,
    work_area: PhysicalRect,
) -> Result<PhysicalRect, PlacementUnavailable> {
    let width = frame.width()?;
    let height = frame.height()?;
    let maximum_left = work_area
        .right
        .checked_sub(width)
        .ok_or(PlacementUnavailable::Arithmetic)?;
    let maximum_top = work_area
        .bottom
        .checked_sub(height)
        .ok_or(PlacementUnavailable::Arithmetic)?;
    frame.translated(
        frame.left.clamp(work_area.left, maximum_left),
        frame.top.clamp(work_area.top, maximum_top),
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HintKind {
    Create,
    Show,
    Focus,
    OwnerChanged,
    StateChanged,
    LocationChanged,
    Hide,
    Destroy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ObservationHint {
    pub sequence: u64,
    pub window: WindowId,
    pub generation: SurfaceGeneration,
    pub kind: HintKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FamilyModel {
    revision: ObservationRevision,
    decisions: BTreeMap<WindowId, SurfaceDecision>,
    census_required: bool,
}

impl FamilyModel {
    pub fn empty() -> Self {
        Self {
            revision: ObservationRevision(0),
            decisions: BTreeMap::new(),
            census_required: true,
        }
    }

    pub fn apply_hint(&mut self, _hint: ObservationHint) {
        self.census_required = true;
    }

    pub fn mark_gap(&mut self) {
        self.census_required = true;
    }

    pub fn reconcile(&mut self, observations: impl IntoIterator<Item = SurfaceObservation>) {
        self.revision = ObservationRevision(self.revision.0.saturating_add(1));
        self.decisions = observations
            .into_iter()
            .map(|observation| {
                let decision = classify_surface(&observation);
                (decision.window, decision)
            })
            .collect();
        self.census_required = false;
    }

    pub fn decisions(&self) -> &BTreeMap<WindowId, SurfaceDecision> {
        &self.decisions
    }

    pub const fn census_required(&self) -> bool {
        self.census_required
    }
}

#[derive(Debug, Error)]
#[error("physical rectangle must have positive width and height")]
pub struct InvalidRect;

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementUnavailable {
    #[error("window is larger than the work area")]
    LargerThanWorkArea,
    #[error("placement arithmetic overflow")]
    Arithmetic,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use proptest::prelude::*;

    use super::*;

    fn id(value: u8) -> WindowId {
        WindowId([value; 32])
    }

    fn observation(window: WindowId, owner: OwnerLink) -> SurfaceObservation {
        let root = match owner {
            OwnerLink::OwnedBy(owner) => owner,
            OwnerLink::Root | OwnerLink::Unresolved(_) => window,
        };
        SurfaceObservation {
            window,
            revision: ObservationRevision(1),
            generation: SurfaceGeneration(1),
            owner,
            root_owner: Availability::Known(root),
            owner_enabled: Availability::Known(true),
            visibility: Visibility::Visible,
            enabled: EnabledState::Enabled,
            cloaked: Availability::Known(false),
            style: StyleEvidence::EMPTY.with(StyleFlag::AppWindow),
            frame: Availability::Known(PhysicalRect {
                left: 0,
                top: 0,
                right: 100,
                bottom: 100,
            }),
            work_area: Availability::Known(PhysicalRect {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            }),
            dpi: Availability::Known(96),
            uia: UiaEvidence::Known(UiaFacts {
                control_type: UiaControlType::Window,
                is_modal: Availability::Known(false),
                window_pattern: true,
            }),
            configured_role: None,
            provenance: SurfaceProvenance::External,
            owner_graph: OwnerGraphState::Complete,
        }
    }

    proptest! {
        #[test]
        fn arbitrary_hint_order_cannot_change_census_result(
            hints in prop::collection::vec((0_u8..8, any::<u64>()), 0..256),
        ) {
            let root = observation(id(1), OwnerLink::Root);
            let mut dialog = observation(id(2), OwnerLink::OwnedBy(id(1)));
            dialog.configured_role = Some(ConfiguredRole::Dialog);
            dialog.owner_enabled = Availability::Known(false);
            let census = vec![root, dialog];

            let mut model = FamilyModel::empty();
            for (kind, sequence) in hints {
                model.apply_hint(ObservationHint {
                    sequence,
                    window: if sequence % 2 == 0 { id(1) } else { id(2) },
                    generation: SurfaceGeneration(sequence),
                    kind: match kind {
                        0 => HintKind::Create,
                        1 => HintKind::Show,
                        2 => HintKind::Focus,
                        3 => HintKind::OwnerChanged,
                        4 => HintKind::StateChanged,
                        5 => HintKind::LocationChanged,
                        6 => HintKind::Hide,
                        _ => HintKind::Destroy,
                    },
                });
            }
            model.reconcile(census.clone());

            let mut reference = FamilyModel::empty();
            reference.reconcile(census);
            prop_assert_eq!(model.decisions(), reference.decisions());
            prop_assert!(!model.census_required());
        }

        #[test]
        fn placement_never_resizes_and_stays_in_work_area(
            left in -10_000_i32..10_000,
            top in -10_000_i32..10_000,
            width in 1_i32..1920,
            height in 1_i32..1080,
        ) {
            let current = PhysicalRect::new(
                left,
                top,
                left.saturating_add(width),
                top.saturating_add(height),
            ).map_err(|error| TestCaseError::fail(error.to_string()))?;
            let work = PhysicalRect::new(0, 0, 1920, 1080)
                .map_err(|error| TestCaseError::fail(error.to_string()))?;
            let plan = plan_placement(PlacementRequest {
                window: id(1),
                generation: SurfaceGeneration(1),
                current,
                owner: work,
                work_area: work,
                intent: PlacementIntent::RecoverIntoWorkArea,
            }).map_err(|error| TestCaseError::fail(error.to_string()))?
                .ok_or_else(|| TestCaseError::fail("recovery must produce a plan"))?;
            prop_assert_eq!(plan.target.width(), Ok(width));
            prop_assert_eq!(plan.target.height(), Ok(height));
            prop_assert!(plan.target.left >= work.left);
            prop_assert!(plan.target.top >= work.top);
            prop_assert!(plan.target.right <= work.right);
            prop_assert!(plan.target.bottom <= work.bottom);
        }
    }

    #[test]
    fn every_transient_fixture_is_never_an_ordinary_tile() {
        for (fixture_id, role) in (2_u8..).zip([
            ConfiguredRole::Dialog,
            ConfiguredRole::Utility,
            ConfiguredRole::Menu,
            ConfiguredRole::Tooltip,
            ConfiguredRole::ComboPopup,
            ConfiguredRole::DragVisual,
            ConfiguredRole::System,
        ]) {
            let mut facts = observation(id(fixture_id), OwnerLink::OwnedBy(id(1)));
            facts.configured_role = Some(role);
            assert_ne!(
                classify_surface(&facts).mode,
                CoordinationMode::OrdinaryManaged
            );
        }
    }

    #[test]
    fn modal_guard_blocks_only_family_stranding_actions() {
        let constraint = ModalConstraint::Active { dialog: id(2) };
        for action in [
            FamilyAction::Hide,
            FamilyAction::MoveWorkspace,
            FamilyAction::MoveMonitor,
            FamilyAction::AdoptScratchpad,
            FamilyAction::TransferDesktop,
            FamilyAction::MinimizeRoot,
            FamilyAction::CloseRoot,
        ] {
            assert_eq!(
                guard_family(&constraint, action),
                GuardDecision::ActiveModal { dialog: id(2) }
            );
        }
        assert_eq!(
            guard_family(&constraint, FamilyAction::FocusActiveDialog),
            GuardDecision::Allowed
        );
        assert_eq!(
            guard_family(&constraint, FamilyAction::Inspect),
            GuardDecision::Allowed
        );
    }

    #[test]
    fn unresolved_modal_facts_remain_unresolved() {
        let reasons = BTreeSet::from([UnavailableFact::ProviderTimedOut]);
        assert_eq!(
            guard_family(
                &ModalConstraint::Unresolved {
                    reasons: reasons.clone(),
                },
                FamilyAction::Hide,
            ),
            GuardDecision::Unresolved { reasons }
        );
    }
}

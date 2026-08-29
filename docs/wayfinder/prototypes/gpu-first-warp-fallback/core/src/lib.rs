#![cfg_attr(
    not(test),
    deny(
        clippy::expect_used,
        clippy::panic,
        clippy::unwrap_used,
        unsafe_op_in_unsafe_fn
    )
)]

use decoration_effect_core::Generation;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum HardwarePreference {
    Enabled,
    Disabled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DeviceEvidence {
    pub hardware_adapter: bool,
    pub hardware_compute: bool,
    pub warp_device: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FrameBudget {
    pub max_update_ns: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MeasuredCost {
    pub update_ns: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SceneDevice {
    Hardware,
    Warp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum EffectCompute {
    DeviceCompute,
    CpuUpload,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AdmittedRuntime {
    Live {
        device: SceneDevice,
        compute: EffectCompute,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum RuntimeError {
    #[error("WARP scene device cannot run GPU compute")]
    WarpCannotOwnCompute,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Error, Serialize, Deserialize)]
pub enum UnavailableReason {
    #[error("no hardware or WARP scene device is available")]
    NoSceneDevice,
    #[error("admitted backend exceeded the measured frame budget")]
    OverBudget,
    #[error("the scene device was removed")]
    DeviceRemoved,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScenePlan {
    pub generation: Generation,
    pub particle_count: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum PlanError {
    #[error("plan generation is stale")]
    StaleGeneration,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SceneOwner {
    published: Option<ScenePlan>,
}

impl AdmittedRuntime {
    pub fn live(device: SceneDevice, compute: EffectCompute) -> Result<Self, RuntimeError> {
        match (device, compute) {
            (SceneDevice::Warp, EffectCompute::DeviceCompute) => {
                Err(RuntimeError::WarpCannotOwnCompute)
            }
            (device, compute) => Ok(Self::Live { device, compute }),
        }
    }
}

pub fn admit(
    preference: HardwarePreference,
    evidence: DeviceEvidence,
) -> Result<AdmittedRuntime, UnavailableReason> {
    let hardware = preference == HardwarePreference::Enabled && evidence.hardware_adapter;
    if hardware {
        let compute = if evidence.hardware_compute {
            EffectCompute::DeviceCompute
        } else {
            EffectCompute::CpuUpload
        };
        return AdmittedRuntime::live(SceneDevice::Hardware, compute)
            .map_err(|_| UnavailableReason::NoSceneDevice);
    }
    if evidence.warp_device {
        return AdmittedRuntime::live(SceneDevice::Warp, EffectCompute::CpuUpload)
            .map_err(|_| UnavailableReason::NoSceneDevice);
    }
    Err(UnavailableReason::NoSceneDevice)
}

pub fn enforce_budget(
    runtime: AdmittedRuntime,
    cost: MeasuredCost,
    budget: FrameBudget,
) -> Result<AdmittedRuntime, UnavailableReason> {
    if cost.update_ns > budget.max_update_ns {
        return Err(UnavailableReason::OverBudget);
    }
    Ok(runtime)
}

impl SceneOwner {
    pub fn publish(&mut self, plan: ScenePlan) -> Result<(), PlanError> {
        if self
            .published
            .is_some_and(|current| plan.generation <= current.generation)
        {
            return Err(PlanError::StaleGeneration);
        }
        self.published = Some(plan);
        Ok(())
    }

    pub fn cancel(&mut self, generation: Generation) -> Result<(), PlanError> {
        match self.published {
            Some(plan) if plan.generation == generation => {
                self.published = None;
                Ok(())
            }
            Some(_) => Err(PlanError::StaleGeneration),
            None => Ok(()),
        }
    }

    pub fn published(&self) -> Option<ScenePlan> {
        self.published
    }

    pub fn wants_frame(&self) -> bool {
        self.published.is_some()
    }

    pub fn cpu_plan_after_device_loss(&self) -> Result<ScenePlan, UnavailableReason> {
        self.published.ok_or(UnavailableReason::DeviceRemoved)
    }
}

#[cfg(test)]
mod tests {
    use decoration_effect_core::Generation;

    use super::*;

    fn plan(value: u64, particles: u16) -> ScenePlan {
        ScenePlan {
            generation: Generation::checked(value).unwrap(),
            particle_count: particles,
        }
    }

    #[test]
    fn warp_cannot_own_device_compute() {
        assert_eq!(
            AdmittedRuntime::live(SceneDevice::Warp, EffectCompute::DeviceCompute),
            Err(RuntimeError::WarpCannotOwnCompute)
        );
    }

    #[test]
    fn admit_selects_the_single_live_ladder() {
        let hardware_compute = DeviceEvidence {
            hardware_adapter: true,
            hardware_compute: true,
            warp_device: true,
        };
        let hardware_draw = DeviceEvidence {
            hardware_adapter: true,
            hardware_compute: false,
            warp_device: true,
        };
        let warp_only = DeviceEvidence {
            hardware_adapter: false,
            hardware_compute: false,
            warp_device: true,
        };
        let none = DeviceEvidence {
            hardware_adapter: false,
            hardware_compute: false,
            warp_device: false,
        };

        assert_eq!(
            admit(HardwarePreference::Enabled, hardware_compute),
            Ok(AdmittedRuntime::Live {
                device: SceneDevice::Hardware,
                compute: EffectCompute::DeviceCompute,
            })
        );
        assert_eq!(
            admit(HardwarePreference::Enabled, hardware_draw),
            Ok(AdmittedRuntime::Live {
                device: SceneDevice::Hardware,
                compute: EffectCompute::CpuUpload,
            })
        );
        assert_eq!(
            admit(HardwarePreference::Disabled, hardware_compute),
            Ok(AdmittedRuntime::Live {
                device: SceneDevice::Warp,
                compute: EffectCompute::CpuUpload,
            })
        );
        assert_eq!(
            admit(HardwarePreference::Enabled, warp_only),
            Ok(AdmittedRuntime::Live {
                device: SceneDevice::Warp,
                compute: EffectCompute::CpuUpload,
            })
        );
        assert_eq!(
            admit(HardwarePreference::Enabled, none),
            Err(UnavailableReason::NoSceneDevice)
        );
    }

    #[test]
    fn over_budget_degrades_without_changing_the_runtime_type_space() {
        let runtime =
            AdmittedRuntime::live(SceneDevice::Hardware, EffectCompute::CpuUpload).unwrap();
        assert_eq!(
            enforce_budget(
                runtime,
                MeasuredCost {
                    update_ns: 5_000_000
                },
                FrameBudget {
                    max_update_ns: 4_166_000
                }
            ),
            Err(UnavailableReason::OverBudget)
        );
        assert_eq!(
            enforce_budget(
                runtime,
                MeasuredCost {
                    update_ns: 1_000_000
                },
                FrameBudget {
                    max_update_ns: 4_166_000
                }
            ),
            Ok(runtime)
        );
    }

    #[test]
    fn scene_owner_replaces_latest_generation_and_rejects_stale() {
        let mut owner = SceneOwner::default();
        let first = plan(1, 64);
        let second = plan(2, 128);
        owner.publish(first).unwrap();
        owner.publish(second).unwrap();
        assert_eq!(owner.published(), Some(second));
        assert_eq!(owner.publish(first), Err(PlanError::StaleGeneration));
        assert_eq!(owner.published(), Some(second));
        assert!(owner.wants_frame());
    }

    #[test]
    fn cancel_is_generation_fenced_and_idempotent() {
        let mut owner = SceneOwner::default();
        let first = plan(1, 64);
        owner.publish(first).unwrap();
        assert_eq!(
            owner.cancel(Generation::checked(2).unwrap()),
            Err(PlanError::StaleGeneration)
        );
        assert_eq!(owner.published(), Some(first));
        owner.cancel(Generation::INITIAL).unwrap();
        assert_eq!(owner.published(), None);
        assert!(!owner.wants_frame());
        owner.cancel(Generation::INITIAL).unwrap();
    }

    #[test]
    fn device_loss_keeps_the_cpu_plan_and_empty_loss_is_removed() {
        let mut owner = SceneOwner::default();
        assert_eq!(
            owner.cpu_plan_after_device_loss(),
            Err(UnavailableReason::DeviceRemoved)
        );
        let first = plan(1, 32);
        owner.publish(first).unwrap();
        assert_eq!(owner.cpu_plan_after_device_loss(), Ok(first));
    }
}

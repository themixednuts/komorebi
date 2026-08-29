#![cfg_attr(
    not(test),
    deny(
        clippy::expect_used,
        clippy::panic,
        clippy::unwrap_used,
        unsafe_op_in_unsafe_fn
    )
)]

use std::mem::{size_of, size_of_val};

use decoration_compute_core::{AdmittedRuntime, DeviceEvidence, EffectCompute, SceneDevice};
use particle_kernel::{InterleavedParticle, ParticleBatch, ParticleStep, step_scalar};
use thiserror::Error;
use windows::{
    Win32::{
        Foundation::HMODULE,
        Graphics::{
            Direct3D::{
                D3D_DRIVER_TYPE_UNKNOWN, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL,
                D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
            },
            Direct3D11::{
                D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_UNORDERED_ACCESS, D3D11_BUFFER_DESC,
                D3D11_BUFFER_UAV, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                D3D11_FEATURE_D3D10_X_HARDWARE_OPTIONS,
                D3D11_FEATURE_DATA_D3D10_X_HARDWARE_OPTIONS, D3D11_MAP_READ,
                D3D11_MAPPED_SUBRESOURCE, D3D11_RESOURCE_MISC_BUFFER_STRUCTURED, D3D11_SDK_VERSION,
                D3D11_SUBRESOURCE_DATA, D3D11_UAV_DIMENSION_BUFFER,
                D3D11_UNORDERED_ACCESS_VIEW_DESC, D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING,
                D3D11CreateDevice, ID3D11Buffer, ID3D11ComputeShader, ID3D11Device,
                ID3D11DeviceContext, ID3D11UnorderedAccessView,
            },
            Dxgi::{CreateDXGIFactory1, IDXGIAdapter, IDXGIAdapter1, IDXGIFactory1},
        },
    },
    core::Interface as _,
};

const COMPUTE_SHADER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/particle.cs.bin"));
const THREADS: u32 = 64;

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("D3D11 rejected scene device creation")]
    DeviceCreate(#[source] windows::core::Error),
    #[error("required compute buffers are unsupported")]
    ComputeUnsupported,
    #[error("particle buffer construction failed")]
    Resource(#[source] windows::core::Error),
    #[error("the scene device was removed")]
    DeviceRemoved,
    #[error(transparent)]
    Kernel(#[from] particle_kernel::KernelError),
}

#[derive(Clone)]
pub struct GpuDevice {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    kind: SceneDevice,
}

pub struct ParticleScene {
    gpu: GpuDevice,
    compute: Option<ID3D11ComputeShader>,
    particles: ID3D11Buffer,
    uav: ID3D11UnorderedAccessView,
    constants: ID3D11Buffer,
    count: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct StepConstants {
    delta_seconds: f32,
    drag: f32,
    acceleration_x: f32,
    acceleration_y: f32,
}

pub fn probe() -> Result<DeviceEvidence, AdapterError> {
    Ok(DeviceEvidence {
        hardware_adapter: open_kind(SceneDevice::Hardware).is_ok(),
        hardware_compute: open_kind(SceneDevice::Hardware)
            .ok()
            .and_then(|gpu| gpu.supports_compute().ok())
            .unwrap_or(false),
        warp_device: open_kind(SceneDevice::Warp).is_ok(),
    })
}

pub fn open(runtime: AdmittedRuntime) -> Result<GpuDevice, AdapterError> {
    let AdmittedRuntime::Live { device, .. } = runtime;
    open_kind(device)
}

fn open_kind(kind: SceneDevice) -> Result<GpuDevice, AdapterError> {
    match kind {
        SceneDevice::Hardware => open_hardware(),
        SceneDevice::Warp => open_warp(),
    }
}

fn open_hardware() -> Result<GpuDevice, AdapterError> {
    let factory: IDXGIFactory1 =
        unsafe { CreateDXGIFactory1() }.map_err(AdapterError::DeviceCreate)?;
    for index in 0.. {
        let adapter = match unsafe { factory.EnumAdapters(index) } {
            Ok(adapter) => adapter,
            Err(_) => break,
        };
        let adapter: IDXGIAdapter1 = adapter.cast().map_err(AdapterError::DeviceCreate)?;
        if let Ok(gpu) = create_on_adapter(
            Some(&adapter),
            D3D_DRIVER_TYPE_UNKNOWN,
            SceneDevice::Hardware,
        ) {
            return Ok(gpu);
        }
    }
    Err(AdapterError::DeviceCreate(windows::core::Error::empty()))
}

fn open_warp() -> Result<GpuDevice, AdapterError> {
    create_on_adapter(None, D3D_DRIVER_TYPE_WARP, SceneDevice::Warp)
}

fn create_on_adapter(
    adapter: Option<&IDXGIAdapter1>,
    driver: windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE,
    kind: SceneDevice,
) -> Result<GpuDevice, AdapterError> {
    let mut device = None;
    let mut context = None;
    let mut feature_level = D3D_FEATURE_LEVEL::default();
    unsafe {
        D3D11CreateDevice(
            adapter
                .map(|adapter| adapter.cast::<IDXGIAdapter>())
                .transpose()
                .map_err(AdapterError::DeviceCreate)?
                .as_ref(),
            driver,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            Some(&mut feature_level),
            Some(&mut context),
        )
    }
    .map_err(AdapterError::DeviceCreate)?;
    let device = device.ok_or_else(|| AdapterError::DeviceCreate(windows::core::Error::empty()))?;
    let context =
        context.ok_or_else(|| AdapterError::DeviceCreate(windows::core::Error::empty()))?;
    Ok(GpuDevice {
        device,
        context,
        kind,
    })
}

impl GpuDevice {
    pub fn kind(&self) -> SceneDevice {
        self.kind
    }

    pub fn supports_compute(&self) -> Result<bool, AdapterError> {
        let mut data = D3D11_FEATURE_DATA_D3D10_X_HARDWARE_OPTIONS::default();
        unsafe {
            self.device
                .CheckFeatureSupport(
                    D3D11_FEATURE_D3D10_X_HARDWARE_OPTIONS,
                    std::ptr::from_mut(&mut data).cast(),
                    size_of::<D3D11_FEATURE_DATA_D3D10_X_HARDWARE_OPTIONS>() as u32,
                )
                .map_err(AdapterError::DeviceCreate)?;
        }
        Ok(data
            .ComputeShaders_Plus_RawAndStructuredBuffers_Via_Shader_4_x
            .as_bool())
    }

    pub fn removed(&self) -> bool {
        unsafe { self.device.GetDeviceRemovedReason() }.is_err()
    }
}

impl ParticleScene {
    pub fn attach(
        gpu: GpuDevice,
        runtime: AdmittedRuntime,
        batch: &ParticleBatch,
    ) -> Result<Self, AdapterError> {
        let AdmittedRuntime::Live { compute, .. } = runtime;
        if gpu.removed() {
            return Err(AdapterError::DeviceRemoved);
        }
        if compute == EffectCompute::DeviceCompute && !gpu.supports_compute()? {
            return Err(AdapterError::ComputeUnsupported);
        }
        let mut packed = vec![
            InterleavedParticle {
                position: [0.0, 0.0],
                velocity: [0.0, 0.0],
            };
            batch.len()
        ];
        batch.write_interleaved(&mut packed)?;
        let particles = structured_buffer(&gpu.device, &packed)?;
        let uav = uav(&gpu.device, &particles, batch.len() as u32)?;
        let constants = constant_buffer(&gpu.device)?;
        let shader = match compute {
            EffectCompute::DeviceCompute => Some(compute_shader(&gpu.device)?),
            EffectCompute::CpuUpload => None,
        };
        Ok(Self {
            gpu,
            compute: shader,
            particles,
            uav,
            constants,
            count: batch.len() as u32,
        })
    }

    pub fn dispatch(&self, step: ParticleStep) -> Result<(), AdapterError> {
        let Some(shader) = &self.compute else {
            return Err(AdapterError::ComputeUnsupported);
        };
        self.write_constants(step)?;
        unsafe {
            self.gpu.context.CSSetShader(shader, None);
            let constants = [Some(self.constants.clone())];
            self.gpu.context.CSSetConstantBuffers(0, Some(&constants));
            let views = [Some(self.uav.clone())];
            self.gpu
                .context
                .CSSetUnorderedAccessViews(0, 1, Some(views.as_ptr()), None);
            self.gpu
                .context
                .Dispatch(self.count.div_ceil(THREADS), 1, 1);
            self.gpu.context.CSSetShader(None, None);
            let empty = [None];
            self.gpu
                .context
                .CSSetUnorderedAccessViews(0, 1, Some(empty.as_ptr()), None);
        }
        self.check_removed()
    }

    pub fn upload(&self, batch: &ParticleBatch) -> Result<(), AdapterError> {
        let mut packed = vec![
            InterleavedParticle {
                position: [0.0, 0.0],
                velocity: [0.0, 0.0],
            };
            batch.len()
        ];
        batch.write_interleaved(&mut packed)?;
        unsafe {
            self.gpu.context.UpdateSubresource(
                &self.particles,
                0,
                None,
                packed.as_ptr().cast(),
                0,
                0,
            );
        }
        self.check_removed()
    }

    pub fn step_cpu(
        &self,
        batch: &mut ParticleBatch,
        step: ParticleStep,
    ) -> Result<(), AdapterError> {
        step_scalar(batch, step);
        self.upload(batch)
    }

    pub fn debug_readback(&self) -> Result<ParticleBatch, AdapterError> {
        let staging = staging_buffer(&self.gpu.device, self.count)?;
        unsafe {
            self.gpu.context.CopyResource(&staging, &self.particles);
        }
        self.check_removed()?;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            self.gpu
                .context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(AdapterError::Resource)?;
        }
        let particles = unsafe {
            std::slice::from_raw_parts(
                mapped.pData.cast::<InterleavedParticle>(),
                self.count as usize,
            )
        }
        .to_vec();
        unsafe {
            self.gpu.context.Unmap(&staging, 0);
        }
        Ok(ParticleBatch::from_interleaved(&particles))
    }

    fn write_constants(&self, step: ParticleStep) -> Result<(), AdapterError> {
        let constants = StepConstants {
            delta_seconds: step.delta_seconds,
            drag: step.drag,
            acceleration_x: step.acceleration_x,
            acceleration_y: step.acceleration_y,
        };
        unsafe {
            self.gpu.context.UpdateSubresource(
                &self.constants,
                0,
                None,
                std::ptr::from_ref(&constants).cast(),
                0,
                0,
            );
        }
        self.check_removed()
    }

    fn check_removed(&self) -> Result<(), AdapterError> {
        if self.gpu.removed() {
            Err(AdapterError::DeviceRemoved)
        } else {
            Ok(())
        }
    }
}

fn compute_shader(device: &ID3D11Device) -> Result<ID3D11ComputeShader, AdapterError> {
    let mut shader = None;
    unsafe {
        device
            .CreateComputeShader(COMPUTE_SHADER, None, Some(&mut shader))
            .map_err(AdapterError::Resource)?;
    }
    shader.ok_or_else(|| AdapterError::Resource(windows::core::Error::empty()))
}

fn structured_buffer(
    device: &ID3D11Device,
    particles: &[InterleavedParticle],
) -> Result<ID3D11Buffer, AdapterError> {
    let mut buffer = None;
    unsafe {
        device
            .CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: size_of_val(particles) as u32,
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_UNORDERED_ACCESS.0 as u32,
                    CPUAccessFlags: 0,
                    MiscFlags: D3D11_RESOURCE_MISC_BUFFER_STRUCTURED.0 as u32,
                    StructureByteStride: size_of::<InterleavedParticle>() as u32,
                },
                Some(&D3D11_SUBRESOURCE_DATA {
                    pSysMem: particles.as_ptr().cast(),
                    ..Default::default()
                }),
                Some(&mut buffer),
            )
            .map_err(AdapterError::Resource)?;
    }
    buffer.ok_or_else(|| AdapterError::Resource(windows::core::Error::empty()))
}

fn uav(
    device: &ID3D11Device,
    buffer: &ID3D11Buffer,
    count: u32,
) -> Result<ID3D11UnorderedAccessView, AdapterError> {
    let mut view = None;
    let desc = D3D11_UNORDERED_ACCESS_VIEW_DESC {
        Format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_UNKNOWN,
        ViewDimension: D3D11_UAV_DIMENSION_BUFFER,
        Anonymous: windows::Win32::Graphics::Direct3D11::D3D11_UNORDERED_ACCESS_VIEW_DESC_0 {
            Buffer: D3D11_BUFFER_UAV {
                FirstElement: 0,
                NumElements: count,
                Flags: 0,
            },
        },
    };
    unsafe {
        device
            .CreateUnorderedAccessView(buffer, Some(&desc), Some(&mut view))
            .map_err(AdapterError::Resource)?;
    }
    view.ok_or_else(|| AdapterError::Resource(windows::core::Error::empty()))
}

fn constant_buffer(device: &ID3D11Device) -> Result<ID3D11Buffer, AdapterError> {
    let initial = StepConstants {
        delta_seconds: 0.0,
        drag: 1.0,
        acceleration_x: 0.0,
        acceleration_y: 0.0,
    };
    let mut buffer = None;
    unsafe {
        device
            .CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: size_of::<StepConstants>() as u32,
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                    StructureByteStride: 0,
                },
                Some(&D3D11_SUBRESOURCE_DATA {
                    pSysMem: std::ptr::from_ref(&initial).cast(),
                    ..Default::default()
                }),
                Some(&mut buffer),
            )
            .map_err(AdapterError::Resource)?;
    }
    buffer.ok_or_else(|| AdapterError::Resource(windows::core::Error::empty()))
}

fn staging_buffer(device: &ID3D11Device, count: u32) -> Result<ID3D11Buffer, AdapterError> {
    let mut buffer = None;
    unsafe {
        device
            .CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: size_of::<InterleavedParticle>() as u32 * count,
                    Usage: D3D11_USAGE_STAGING,
                    BindFlags: 0,
                    CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                    MiscFlags: D3D11_RESOURCE_MISC_BUFFER_STRUCTURED.0 as u32,
                    StructureByteStride: size_of::<InterleavedParticle>() as u32,
                },
                None,
                Some(&mut buffer),
            )
            .map_err(AdapterError::Resource)?;
    }
    buffer.ok_or_else(|| AdapterError::Resource(windows::core::Error::empty()))
}

#[cfg(test)]
mod tests {
    use decoration_compute_core::{AdmittedRuntime, EffectCompute, SceneDevice};
    use particle_kernel::{ParticleBatch, ParticleStep};

    use super::*;

    fn equivalent(left: &ParticleBatch, right: &ParticleBatch) -> bool {
        (left.checksum() - right.checksum()).abs() < 1.0e-2
    }

    #[test]
    fn hardware_and_warp_devices_open() {
        let evidence = probe().expect("probe the attached GPU");
        assert!(evidence.warp_device || evidence.hardware_adapter);
        if evidence.hardware_adapter {
            open_kind(SceneDevice::Hardware).expect("open hardware");
        }
        if evidence.warp_device {
            open_kind(SceneDevice::Warp).expect("open WARP");
        }
    }

    #[test]
    fn compute_matches_scalar_on_hardware_when_available() {
        let Ok(gpu) = open_kind(SceneDevice::Hardware) else {
            return;
        };
        if !gpu.supports_compute().unwrap_or(false) {
            return;
        }
        let runtime = AdmittedRuntime::live(SceneDevice::Hardware, EffectCompute::DeviceCompute)
            .expect("hardware compute is legal");
        let mut expected = ParticleBatch::seeded(128, 11);
        let scene = ParticleScene::attach(gpu, runtime, &expected).expect("attach");
        let step = ParticleStep::checked(0.016, 0.98, 0.0, -9.8).expect("step");
        scene.dispatch(step).expect("dispatch");
        step_scalar(&mut expected, step);
        let actual = scene.debug_readback().expect("test-only staging copy");
        assert!(equivalent(&expected, &actual));
    }

    #[test]
    fn cpu_upload_on_warp_matches_scalar() {
        let Ok(gpu) = open_kind(SceneDevice::Warp) else {
            return;
        };
        let runtime = AdmittedRuntime::live(SceneDevice::Warp, EffectCompute::CpuUpload)
            .expect("warp cpu upload is legal");
        let mut batch = ParticleBatch::seeded(64, 3);
        let scene = ParticleScene::attach(gpu, runtime, &batch).expect("attach");
        let step = ParticleStep::checked(0.016, 0.9, 1.0, 0.0).expect("step");
        scene.step_cpu(&mut batch, step).expect("cpu upload");
        let actual = scene.debug_readback().expect("test-only staging copy");
        assert!(equivalent(&batch, &actual));
    }
}

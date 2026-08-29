#![cfg_attr(feature = "portable-simd", feature(portable_simd))]
#![cfg_attr(
    test,
    allow(
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::unwrap_used
    )
)]

use serde::{Deserialize, Serialize};
use thiserror::Error;

const LANES: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParticleStep {
    pub delta_seconds: f32,
    pub drag: f32,
    pub acceleration_x: f32,
    pub acceleration_y: f32,
}

impl ParticleStep {
    pub fn checked(
        delta_seconds: f32,
        drag: f32,
        acceleration_x: f32,
        acceleration_y: f32,
    ) -> Result<Self, KernelError> {
        if !(0.0..=0.1).contains(&delta_seconds)
            || !(0.0..=1.0).contains(&drag)
            || ![delta_seconds, drag, acceleration_x, acceleration_y]
                .into_iter()
                .all(f32::is_finite)
        {
            return Err(KernelError::InvalidStep);
        }
        Ok(Self {
            delta_seconds,
            drag,
            acceleration_x,
            acceleration_y,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParticleBatch {
    position_x: Vec<f32>,
    position_y: Vec<f32>,
    velocity_x: Vec<f32>,
    velocity_y: Vec<f32>,
}

impl ParticleBatch {
    pub fn from_components(
        position_x: Vec<f32>,
        position_y: Vec<f32>,
        velocity_x: Vec<f32>,
        velocity_y: Vec<f32>,
    ) -> Result<Self, KernelError> {
        let length = position_x.len();
        if [position_y.len(), velocity_x.len(), velocity_y.len()]
            .into_iter()
            .any(|candidate| candidate != length)
        {
            return Err(KernelError::MismatchedLengths);
        }
        Ok(Self {
            position_x,
            position_y,
            velocity_x,
            velocity_y,
        })
    }

    pub fn seeded(length: usize, seed: u64) -> Self {
        let mut random = XorShift64::new(seed);
        let mut generate = || {
            (0..length)
                .map(|_| random.next_f32() * 2.0 - 1.0)
                .collect::<Vec<_>>()
        };
        Self {
            position_x: generate(),
            position_y: generate(),
            velocity_x: generate(),
            velocity_y: generate(),
        }
    }

    pub fn len(&self) -> usize {
        self.position_x.len()
    }

    pub fn is_empty(&self) -> bool {
        self.position_x.is_empty()
    }

    pub fn checksum(&self) -> f64 {
        self.position_x
            .iter()
            .chain(&self.position_y)
            .chain(&self.velocity_x)
            .chain(&self.velocity_y)
            .map(|value| f64::from(*value))
            .sum()
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum KernelError {
    #[error("particle component lengths differ")]
    MismatchedLengths,
    #[error("particle step is non-finite or outside its admitted range")]
    InvalidStep,
    #[error("AVX2 is unavailable on this processor")]
    Avx2Unavailable,
}

#[inline(never)]
pub fn step_scalar(batch: &mut ParticleBatch, step: ParticleStep) {
    step_scalar_range(batch, step, 0);
}

fn step_scalar_range(batch: &mut ParticleBatch, step: ParticleStep, start: usize) {
    let positions = batch.position_x[start..]
        .iter_mut()
        .zip(&mut batch.position_y[start..]);
    let velocities = batch.velocity_x[start..]
        .iter_mut()
        .zip(&mut batch.velocity_y[start..]);
    for ((position_x, position_y), (velocity_x, velocity_y)) in positions.zip(velocities) {
        *velocity_x = *velocity_x * step.drag + step.acceleration_x * step.delta_seconds;
        *velocity_y = *velocity_y * step.drag + step.acceleration_y * step.delta_seconds;
        *position_x += *velocity_x * step.delta_seconds;
        *position_y += *velocity_y * step.delta_seconds;
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Avx2Kernel(());

impl Avx2Kernel {
    pub fn detect() -> Result<Self, KernelError> {
        std::arch::is_x86_feature_detected!("avx2")
            .then_some(Self(()))
            .ok_or(KernelError::Avx2Unavailable)
    }

    pub fn step(self, batch: &mut ParticleBatch, step: ParticleStep) {
        // SAFETY: construction proves AVX2 is available. ParticleBatch construction proves all
        // component lengths match, and the inner function handles the scalar tail.
        unsafe { step_avx2_inner(batch, step) };
    }
}

pub fn step_avx2(batch: &mut ParticleBatch, step: ParticleStep) -> Result<(), KernelError> {
    Avx2Kernel::detect()?.step(batch, step);
    Ok(())
}

#[target_feature(enable = "avx2")]
unsafe fn step_avx2_inner(batch: &mut ParticleBatch, step: ParticleStep) {
    use std::arch::x86_64::{
        _mm256_add_ps, _mm256_loadu_ps, _mm256_mul_ps, _mm256_set1_ps, _mm256_storeu_ps,
    };

    let vectorized = batch.len() - batch.len() % LANES;
    let delta = _mm256_set1_ps(step.delta_seconds);
    let drag = _mm256_set1_ps(step.drag);
    let acceleration_x = _mm256_set1_ps(step.acceleration_x);
    let acceleration_y = _mm256_set1_ps(step.acceleration_y);
    let mut index = 0;
    while index < vectorized {
        // SAFETY: index advances in LANES-sized blocks below vectorized. All four allocations have
        // the same length, are separately owned, and loadu/storeu impose no alignment requirement.
        unsafe {
            let mut velocity_x = _mm256_loadu_ps(batch.velocity_x.as_ptr().add(index));
            let mut velocity_y = _mm256_loadu_ps(batch.velocity_y.as_ptr().add(index));
            let mut position_x = _mm256_loadu_ps(batch.position_x.as_ptr().add(index));
            let mut position_y = _mm256_loadu_ps(batch.position_y.as_ptr().add(index));
            velocity_x = _mm256_add_ps(
                _mm256_mul_ps(velocity_x, drag),
                _mm256_mul_ps(acceleration_x, delta),
            );
            velocity_y = _mm256_add_ps(
                _mm256_mul_ps(velocity_y, drag),
                _mm256_mul_ps(acceleration_y, delta),
            );
            position_x = _mm256_add_ps(position_x, _mm256_mul_ps(velocity_x, delta));
            position_y = _mm256_add_ps(position_y, _mm256_mul_ps(velocity_y, delta));
            _mm256_storeu_ps(batch.velocity_x.as_mut_ptr().add(index), velocity_x);
            _mm256_storeu_ps(batch.velocity_y.as_mut_ptr().add(index), velocity_y);
            _mm256_storeu_ps(batch.position_x.as_mut_ptr().add(index), position_x);
            _mm256_storeu_ps(batch.position_y.as_mut_ptr().add(index), position_y);
        }
        index += LANES;
    }
    step_scalar_range(batch, step, vectorized);
}

#[cfg(feature = "portable-simd")]
#[inline(never)]
pub fn step_portable_simd(batch: &mut ParticleBatch, step: ParticleStep) {
    use std::simd::f32x8;

    let vectorized = batch.len() - batch.len() % LANES;
    let delta = f32x8::splat(step.delta_seconds);
    let drag = f32x8::splat(step.drag);
    let acceleration_x = f32x8::splat(step.acceleration_x);
    let acceleration_y = f32x8::splat(step.acceleration_y);
    let mut index = 0;
    while index < vectorized {
        let range = index..index + LANES;
        let mut velocity_x = f32x8::from_slice(&batch.velocity_x[range.clone()]);
        let mut velocity_y = f32x8::from_slice(&batch.velocity_y[range.clone()]);
        let mut position_x = f32x8::from_slice(&batch.position_x[range.clone()]);
        let mut position_y = f32x8::from_slice(&batch.position_y[range.clone()]);
        velocity_x = velocity_x * drag + acceleration_x * delta;
        velocity_y = velocity_y * drag + acceleration_y * delta;
        position_x += velocity_x * delta;
        position_y += velocity_y * delta;
        velocity_x.copy_to_slice(&mut batch.velocity_x[range.clone()]);
        velocity_y.copy_to_slice(&mut batch.velocity_y[range.clone()]);
        position_x.copy_to_slice(&mut batch.position_x[range.clone()]);
        position_y.copy_to_slice(&mut batch.position_y[range]);
        index += LANES;
    }
    step_scalar_range(batch, step, vectorized);
}

struct XorShift64(u64);

impl XorShift64 {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    fn next_f32(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        let shifted = (self.0 >> 41).to_le_bytes();
        let mantissa = u32::from_le_bytes([shifted[0], shifted[1], shifted[2], shifted[3]]);
        f32::from_bits(0x3f80_0000 | mantissa) - 1.0
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn equivalent(left: &ParticleBatch, right: &ParticleBatch) -> bool {
        let components = [
            (&left.position_x, &right.position_x),
            (&left.position_y, &right.position_y),
            (&left.velocity_x, &right.velocity_x),
            (&left.velocity_y, &right.velocity_y),
        ];
        components.into_iter().all(|(left, right)| {
            left.iter().zip(right).all(|(left, right)| {
                (left.is_nan() && right.is_nan())
                    || (*left - *right).abs() <= 1.0e-5 * left.abs().max(right.abs()).max(1.0)
            })
        })
    }

    proptest! {
        #[test]
        fn avx2_matches_scalar_for_lengths_and_finite_values(
            length in 0_usize..257,
            seed in 1_u64..u64::MAX,
            delta in 0.0_f32..0.1,
            drag in 0.0_f32..1.0,
            acceleration_x in -100.0_f32..100.0,
            acceleration_y in -100.0_f32..100.0,
        ) {
            let step = ParticleStep::checked(delta, drag, acceleration_x, acceleration_y).unwrap();
            let mut expected = ParticleBatch::seeded(length, seed);
            let mut actual = expected.clone();
            step_scalar(&mut expected, step);
            if std::arch::is_x86_feature_detected!("avx2") {
                step_avx2(&mut actual, step).unwrap();
                prop_assert!(equivalent(&expected, &actual));
            }
        }

        #[cfg(feature = "portable-simd")]
        #[test]
        fn portable_simd_matches_scalar_for_lengths_and_finite_values(
            length in 0_usize..257,
            seed in 1_u64..u64::MAX,
            delta in 0.0_f32..0.1,
            drag in 0.0_f32..1.0,
            acceleration_x in -100.0_f32..100.0,
            acceleration_y in -100.0_f32..100.0,
        ) {
            let step = ParticleStep::checked(delta, drag, acceleration_x, acceleration_y).unwrap();
            let mut expected = ParticleBatch::seeded(length, seed);
            let mut actual = expected.clone();
            step_scalar(&mut expected, step);
            step_portable_simd(&mut actual, step);
            prop_assert!(equivalent(&expected, &actual));
        }
    }

    #[test]
    fn rejects_mismatched_components_and_invalid_steps() {
        assert_eq!(
            ParticleBatch::from_components(vec![0.0], Vec::new(), vec![0.0], vec![0.0]),
            Err(KernelError::MismatchedLengths)
        );
        assert_eq!(
            ParticleStep::checked(f32::NAN, 1.0, 0.0, 0.0),
            Err(KernelError::InvalidStep)
        );
    }
}

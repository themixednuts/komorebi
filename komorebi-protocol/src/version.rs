use std::num::NonZeroU16;

use thiserror::Error;

const MAX_VERSION_RANGES: usize = 32;

macro_rules! version_type {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
        pub struct $name(NonZeroU16);

        impl $name {
            #[must_use]
            pub const fn new(value: NonZeroU16) -> Self {
                Self(value)
            }

            #[must_use]
            pub const fn get(self) -> u16 {
                self.0.get()
            }
        }

        impl TryFrom<u16> for $name {
            type Error = VersionSetError;

            fn try_from(value: u16) -> Result<Self, Self::Error> {
                NonZeroU16::new(value)
                    .map(Self)
                    .ok_or(VersionSetError::ZeroVersion)
            }
        }
    };
}

version_type!(ProtocolVersion);
version_type!(CatalogSchemaVersion);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VersionRange<V> {
    first: V,
    last: V,
}

impl<V: Copy + Ord> VersionRange<V> {
    /// Creates an inclusive range whose first version is not after its last.
    ///
    /// # Errors
    ///
    /// Returns [`VersionSetError::ReversedRange`] when `first > last`.
    pub fn new(first: V, last: V) -> Result<Self, VersionSetError> {
        if first > last {
            Err(VersionSetError::ReversedRange)
        } else {
            Ok(Self { first, last })
        }
    }

    #[must_use]
    pub const fn first(self) -> V {
        self.first
    }

    #[must_use]
    pub const fn last(self) -> V {
        self.last
    }

    #[must_use]
    pub fn contains(self, version: V) -> bool {
        self.first <= version && version <= self.last
    }

    fn highest_overlap(self, other: Self) -> Option<V> {
        let first = self.first.max(other.first);
        let last = self.last.min(other.last);
        (first <= last).then_some(last)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VersionRanges<V>(Box<[VersionRange<V>]>);

impl<V: Copy + Ord> VersionRanges<V> {
    /// Creates a bounded, nonempty set of ascending, nonoverlapping ranges.
    ///
    /// # Errors
    ///
    /// Returns a [`VersionSetError`] when the set is empty, exceeds the protocol
    /// bound, or is not already in canonical ascending order.
    pub fn new(ranges: Vec<VersionRange<V>>) -> Result<Self, VersionSetError> {
        if ranges.is_empty() {
            return Err(VersionSetError::Empty);
        }
        if ranges.len() > MAX_VERSION_RANGES {
            return Err(VersionSetError::TooMany(ranges.len()));
        }
        if ranges.windows(2).any(|pair| pair[0].last >= pair[1].first) {
            return Err(VersionSetError::NonCanonical);
        }
        Ok(Self(ranges.into_boxed_slice()))
    }

    #[must_use]
    pub fn as_slice(&self) -> &[VersionRange<V>] {
        &self.0
    }

    #[must_use]
    pub fn contains(&self, version: V) -> bool {
        self.0.iter().any(|range| range.contains(version))
    }

    #[must_use]
    pub fn highest_common(&self, other: &Self) -> Option<V> {
        self.0
            .iter()
            .flat_map(|left| {
                other
                    .0
                    .iter()
                    .filter_map(|right| left.highest_overlap(*right))
            })
            .max()
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum VersionSetError {
    #[error("protocol versions begin at one")]
    ZeroVersion,
    #[error("version range begins after it ends")]
    ReversedRange,
    #[error("version range set must not be empty")]
    Empty,
    #[error("version range set has {0} ranges; maximum is {MAX_VERSION_RANGES}")]
    TooMany(usize),
    #[error("version ranges must be ascending and nonoverlapping")]
    NonCanonical,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn protocol(value: u16) -> Result<ProtocolVersion, VersionSetError> {
        value.try_into()
    }

    fn range(first: u16, last: u16) -> Result<VersionRange<ProtocolVersion>, VersionSetError> {
        VersionRange::new(protocol(first)?, protocol(last)?)
    }

    #[test]
    fn versions_reject_zero_and_reversed_ranges() {
        assert_eq!(
            ProtocolVersion::try_from(0),
            Err(VersionSetError::ZeroVersion)
        );
        assert_eq!(range(2, 1), Err(VersionSetError::ReversedRange));
    }

    #[test]
    fn range_sets_reject_empty_overlapping_and_unordered_inputs() -> Result<(), VersionSetError> {
        assert_eq!(
            VersionRanges::<ProtocolVersion>::new(Vec::new()),
            Err(VersionSetError::Empty)
        );
        assert_eq!(
            VersionRanges::new(vec![range(1, 3)?, range(3, 4)?]),
            Err(VersionSetError::NonCanonical)
        );
        assert_eq!(
            VersionRanges::new(vec![range(5, 6)?, range(1, 2)?]),
            Err(VersionSetError::NonCanonical)
        );
        Ok(())
    }

    #[test]
    fn negotiation_selects_the_highest_common_version() -> Result<(), VersionSetError> {
        let server = VersionRanges::new(vec![range(1, 2)?, range(5, 8)?])?;
        let client = VersionRanges::new(vec![range(2, 6)?, range(9, 10)?])?;

        assert_eq!(server.highest_common(&client), Some(protocol(6)?));
        assert!(server.contains(protocol(6)?));
        assert!(client.contains(protocol(6)?));
        Ok(())
    }
}

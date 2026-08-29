use super::OperationDirection;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct DirectionSet(u8);

impl DirectionSet {
    const LEFT: u8 = 1 << 0;
    const RIGHT: u8 = 1 << 1;
    const UP: u8 = 1 << 2;
    const DOWN: u8 = 1 << 3;

    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    #[must_use]
    pub const fn contains(self, direction: OperationDirection) -> bool {
        self.0 & bit(direction) != 0
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn iter(self) -> impl Iterator<Item = OperationDirection> {
        [
            OperationDirection::Left,
            OperationDirection::Right,
            OperationDirection::Up,
            OperationDirection::Down,
        ]
        .into_iter()
        .filter(move |direction| self.contains(*direction))
    }
}

impl FromIterator<OperationDirection> for DirectionSet {
    fn from_iter<T: IntoIterator<Item = OperationDirection>>(directions: T) -> Self {
        let mut bits = 0;
        for direction in directions {
            bits |= bit(direction);
        }
        Self(bits)
    }
}

impl<const N: usize> From<[OperationDirection; N]> for DirectionSet {
    fn from(directions: [OperationDirection; N]) -> Self {
        directions.into_iter().collect()
    }
}

const fn bit(direction: OperationDirection) -> u8 {
    match direction {
        OperationDirection::Left => DirectionSet::LEFT,
        OperationDirection::Right => DirectionSet::RIGHT,
        OperationDirection::Up => DirectionSet::UP,
        OperationDirection::Down => DirectionSet::DOWN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_deduplicates_and_iterates_in_spatial_order() {
        let targets = [
            OperationDirection::Down,
            OperationDirection::Left,
            OperationDirection::Down,
        ]
        .into_iter()
        .collect::<DirectionSet>();

        assert_eq!(
            targets.iter().collect::<Vec<_>>(),
            vec![OperationDirection::Left, OperationDirection::Down]
        );
        assert!(targets.contains(OperationDirection::Left));
        assert!(!targets.contains(OperationDirection::Up));
    }
}

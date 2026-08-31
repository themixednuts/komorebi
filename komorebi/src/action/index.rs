use serde::Deserialize;
use serde::Serialize;

macro_rules! semantic_index {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(usize);

        impl $name {
            #[must_use]
            pub const fn new(value: usize) -> Self {
                Self(value)
            }

            #[must_use]
            pub const fn get(self) -> usize {
                self.0
            }
        }
    };
}

semantic_index!(MonitorIndex);
semantic_index!(WorkspaceIndex);
semantic_index!(ContainerIndex);
semantic_index!(StackIndex);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct WorkspaceLocation {
    monitor: MonitorIndex,
    workspace: WorkspaceIndex,
}

impl WorkspaceLocation {
    #[must_use]
    pub const fn new(monitor: MonitorIndex, workspace: WorkspaceIndex) -> Self {
        Self { monitor, workspace }
    }

    #[must_use]
    pub const fn monitor(self) -> MonitorIndex {
        self.monitor
    }

    #[must_use]
    pub const fn workspace(self) -> WorkspaceIndex {
        self.workspace
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_indices_round_trip_as_wire_integers() {
        let encoded = serde_json::to_string(&MonitorIndex::new(7)).unwrap();
        assert_eq!(encoded, "7");
        assert_eq!(
            serde_json::from_str::<MonitorIndex>(&encoded).unwrap(),
            MonitorIndex::new(7)
        );
    }
}

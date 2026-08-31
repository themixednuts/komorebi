use crate::PluginId;

const IDENTITY_PREFIX: &str = "Komorebi.Extension.";
const IDENTITY_SUFFIX: &str = ".v1";

/// Stable `AppContainer` profile identity owned by exactly one plugin.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SandboxIdentity(Box<str>);

impl SandboxIdentity {
    #[must_use]
    pub fn for_plugin(plugin: &PluginId) -> Self {
        Self(format!("{IDENTITY_PREFIX}{}{IDENTITY_SUFFIX}", plugin.as_str()).into_boxed_str())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::SandboxIdentity;
    use crate::PluginId;

    #[test]
    fn profile_identity_is_stable_and_plugin_scoped() -> Result<(), Box<dyn std::error::Error>> {
        let first = SandboxIdentity::for_plugin(&PluginId::parse("first")?);
        let second = SandboxIdentity::for_plugin(&PluginId::parse("second")?);

        assert_eq!(first.as_str(), "Komorebi.Extension.first.v1");
        assert_ne!(first, second);
        Ok(())
    }
}

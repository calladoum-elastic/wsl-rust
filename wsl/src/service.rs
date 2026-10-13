use crate::prelude::*;
use crate::raw;

/// A component that must be installed for the SDK to work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Component {
    VirtualMachinePlatform,
    WslPackage,
    /// This application is incompatible with the installed WSL runtime.
    SdkNeedsUpdate,
    Unknown(i32),
}

impl From<raw::Component> for Component {
    fn from(c: raw::Component) -> Self {
        match c {
            raw::Component::VirtualMachinePlatform => Self::VirtualMachinePlatform,
            raw::Component::WslPackage => Self::WslPackage,
            raw::Component::SdkNeedsUpdate => Self::SdkNeedsUpdate,
            raw::Component(other) => Self::Unknown(other),
        }
    }
}

/// Version of the installed WSL service.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub revision: u32,
}

/// Components that are missing from this machine; empty when everything is ready.
pub fn missing_components() -> Result<Vec<Component>> {
    wsl_sys::init()?;
    Ok(raw::WslcService::GetMissingComponents()?
        .into_iter()
        .map(Component::from)
        .collect())
}

/// Version of the installed WSL service.
pub fn version() -> Result<Version> {
    wsl_sys::init()?;
    let v = raw::WslcService::GetVersion()?;
    Ok(Version {
        major: v.Major()?,
        minor: v.Minor()?,
        revision: v.Revision()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_from_raw() {
        assert_eq!(
            Component::from(raw::Component::VirtualMachinePlatform),
            Component::VirtualMachinePlatform
        );
        assert_eq!(Component::from(raw::Component::WslPackage), Component::WslPackage);
        assert_eq!(
            Component::from(raw::Component::SdkNeedsUpdate),
            Component::SdkNeedsUpdate
        );
    }

    #[test]
    fn unknown_component_keeps_value() {
        assert_eq!(Component::from(raw::Component(12345)), Component::Unknown(12345));
    }

    /// Needs the WSL runtime installed.
    #[test]
    #[ignore]
    fn queries_installed_service() {
        missing_components().unwrap();
        let v = version().unwrap();
        assert!(v.major > 0 || v.minor > 0 || v.revision > 0);
    }
}

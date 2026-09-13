//! Main-window section identifiers (no GPUI imports).
#![forbid(unsafe_code)]

/// Main-window sections (primary tabs + menu secondary surfaces).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Section {
    Events,
    Daemon,
    Rules,
    Hosts,
    Applications,
    Addresses,
    Ports,
    Users,
    Traffic,
    Network,
    Profiling,
    Preferences,
}

impl Section {
    /// Horizontal primary tabs (statistics shell).
    pub const PRIMARY: [Self; 8] = [
        Self::Events,
        Self::Daemon,
        Self::Rules,
        Self::Hosts,
        Self::Applications,
        Self::Addresses,
        Self::Ports,
        Self::Users,
    ];

    /// All navigable sections including menu secondary surfaces.
    #[cfg(test)]
    pub const ALL: [Self; 12] = [
        Self::Events,
        Self::Daemon,
        Self::Rules,
        Self::Hosts,
        Self::Applications,
        Self::Addresses,
        Self::Ports,
        Self::Users,
        Self::Traffic,
        Self::Network,
        Self::Profiling,
        Self::Preferences,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Events => "Events",
            Self::Daemon => "Daemon",
            Self::Rules => "Rules",
            Self::Hosts => "Hosts",
            Self::Applications => "Applications",
            Self::Addresses => "Addresses",
            Self::Ports => "Ports",
            Self::Users => "Users",
            Self::Traffic => "Traffic",
            Self::Network => "Network",
            Self::Profiling => "Profiling",
            Self::Preferences => "Preferences",
        }
    }

    /// Whether the shared filter strip applies to this section.
    #[cfg(test)]
    #[must_use]
    pub const fn uses_filter(self) -> bool {
        matches!(
            self,
            Self::Events
                | Self::Rules
                | Self::Hosts
                | Self::Applications
                | Self::Addresses
                | Self::Ports
                | Self::Users
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_tabs_and_labels_are_stable() {
        assert_eq!(Section::PRIMARY.len(), 8);
        assert_eq!(Section::PRIMARY[0], Section::Events);
        assert_eq!(Section::PRIMARY[2], Section::Rules);
        assert_eq!(Section::Events.label(), "Events");
        assert_eq!(Section::Preferences.label(), "Preferences");
        assert!(Section::Events.uses_filter());
        assert!(!Section::Daemon.uses_filter());
        assert_eq!(Section::ALL.len(), 12);
    }
}

//! Main-window section identifiers (no GPUI imports).
#![forbid(unsafe_code)]

/// Main-window left-nav sections.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Section {
    Status,
    Applications,
    Rules,
    Log,
    Network,
    Settings,
}

impl Section {
    pub const ALL: [Self; 6] = [
        Self::Status,
        Self::Applications,
        Self::Rules,
        Self::Log,
        Self::Network,
        Self::Settings,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Status => "Status",
            Self::Applications => "Applications",
            Self::Rules => "Rules",
            Self::Log => "Log",
            Self::Network => "Network",
            Self::Settings => "Settings",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_labels_are_stable() {
        assert_eq!(Section::Rules.label(), "Rules");
        assert_eq!(Section::ALL.len(), 6);
        assert_eq!(Section::ALL[2], Section::Rules);
    }
}

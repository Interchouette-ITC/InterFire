//! Shared list-tab filter strip state (text, verdict, limit).
#![forbid(unsafe_code)]

/// Hard cap for All / Custom result limits (matches audit subscriber cap).
pub const FILTER_HARD_CAP: usize = 2_000;

/// Verdict filter for list tabs (no reject).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum VerdictFilter {
    #[default]
    All,
    Allow,
    Deny,
    Prompt,
}

impl VerdictFilter {
    pub const ALL: [Self; 4] = [Self::All, Self::Allow, Self::Deny, Self::Prompt];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Allow => "Allow",
            Self::Deny => "Deny",
            Self::Prompt => "Prompt",
        }
    }

    #[must_use]
    pub fn matches(self, verdict: &str) -> bool {
        match self {
            Self::All => true,
            Self::Allow => verdict.eq_ignore_ascii_case("allow"),
            Self::Deny => verdict.eq_ignore_ascii_case("deny"),
            Self::Prompt => verdict.eq_ignore_ascii_case("prompt"),
        }
    }
}

/// Result limit presets plus custom.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResultLimit {
    Preset(usize),
    All,
    Custom(usize),
}

impl Default for ResultLimit {
    fn default() -> Self {
        Self::Preset(100)
    }
}

impl ResultLimit {
    /// Preset limit values offered in the filter Select.
    pub const PRESETS: [usize; 4] = [50, 100, 200, 300];

    #[must_use]
    pub fn effective(self) -> usize {
        match self {
            Self::Preset(n) | Self::Custom(n) => n.min(FILTER_HARD_CAP),
            Self::All => FILTER_HARD_CAP,
        }
    }

    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Preset(n) => n.to_string(),
            Self::All => "All".into(),
            Self::Custom(n) => format!("Custom({n})"),
        }
    }
}

/// Shared filter strip values.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ListFilter {
    pub text: String,
    pub verdict: VerdictFilter,
    pub limit: ResultLimit,
}

impl ListFilter {
    /// Reset to defaults (empty text, All verdict, limit 100).
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Whether `haystack` passes the text filter (case-insensitive substring).
    #[must_use]
    pub fn text_matches(&self, haystack: &str) -> bool {
        if self.text.is_empty() {
            return true;
        }
        haystack
            .to_ascii_lowercase()
            .contains(&self.text.to_ascii_lowercase())
    }

    /// Apply text + verdict + limit to owned rows; returns (`shown`, `total_before_limit`).
    #[must_use]
    pub fn apply_rows<T, F>(&self, rows: Vec<T>, mut predicate: F) -> (Vec<T>, usize)
    where
        F: FnMut(&T) -> (String, String),
    {
        let filtered: Vec<T> = rows
            .into_iter()
            .filter(|row| {
                let (text, verdict) = predicate(row);
                self.text_matches(&text) && self.verdict.matches(&verdict)
            })
            .collect();
        let total = filtered.len();
        let limit = self.limit.effective();
        let shown: Vec<T> = filtered.into_iter().take(limit).collect();
        (shown, total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_applies_text_verdict_and_limit() {
        let mut filter = ListFilter {
            text: "curl".into(),
            verdict: VerdictFilter::Allow,
            limit: ResultLimit::Preset(50),
        };
        let rows = vec![
            ("/usr/bin/curl".to_owned(), "allow".to_owned()),
            ("/usr/bin/wget".to_owned(), "allow".to_owned()),
            ("/usr/bin/curl".to_owned(), "deny".to_owned()),
        ];
        let (shown, total) = filter.apply_rows(rows, Clone::clone);
        assert_eq!(total, 1);
        assert_eq!(shown.len(), 1);
        filter.clear();
        assert_eq!(filter, ListFilter::default());
        assert_eq!(ResultLimit::All.effective(), FILTER_HARD_CAP);
        assert_eq!(ResultLimit::Custom(500).effective(), 500);
        assert_eq!(ResultLimit::Custom(9_999).effective(), FILTER_HARD_CAP);
        assert_eq!(ResultLimit::Preset(100).label(), "100");
        assert_eq!(ResultLimit::PRESETS, [50, 100, 200, 300]);
        assert!(VerdictFilter::Deny.matches("deny"));
        assert!(!VerdictFilter::Prompt.matches("deny"));
    }
}

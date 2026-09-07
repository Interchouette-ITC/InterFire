//! Rules form validation helpers (no GPUI).
#![forbid(unsafe_code)]

use interfire_proto::RuleRow;

/// Verdict chip for the add-rule form.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuleVerdict {
    Allow,
    Deny,
    Prompt,
}

impl RuleVerdict {
    pub const ALL: [Self; 3] = [Self::Allow, Self::Deny, Self::Prompt];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
            Self::Prompt => "prompt",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
            Self::Prompt => "prompt",
        }
    }
}

/// Parsed add-rule fields ready for IPC.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewRule {
    pub id: u64,
    pub executable: String,
    pub verdict: String,
    pub port: u16,
}

/// Suggest the next unused rule id (max existing + 1, or 1).
#[must_use]
pub fn next_rule_id(rules: &[RuleRow]) -> u64 {
    rules.iter().map(|row| row.id).max().map_or(1, |id| id + 1)
}

/// Validate add-rule form text fields.
///
/// # Errors
///
/// Returns a short operator-facing message when a field is invalid.
pub fn validate_new_rule(
    id: &str,
    executable: &str,
    verdict: RuleVerdict,
    port: &str,
) -> Result<NewRule, String> {
    let id = id
        .trim()
        .parse::<u64>()
        .map_err(|_| "id must be an integer".to_owned())?;
    let executable = executable.trim().to_owned();
    if executable.is_empty() || !executable.starts_with('/') {
        return Err("executable must be an absolute path".into());
    }
    let port = port
        .trim()
        .parse::<u16>()
        .map_err(|_| "port must be 0..65535".to_owned())?;
    Ok(NewRule {
        id,
        executable,
        verdict: verdict.as_str().to_owned(),
        port,
    })
}

#[cfg(test)]
mod tests {
    use super::{RuleVerdict, next_rule_id, validate_new_rule};
    use interfire_proto::RuleRow;

    fn rule(id: u64) -> RuleRow {
        RuleRow {
            id,
            executable: "/bin/true".into(),
            verdict: "Allow".into(),
            port: 80,
        }
    }

    #[test]
    fn next_id_from_empty_and_max() {
        assert_eq!(next_rule_id(&[]), 1);
        assert_eq!(next_rule_id(&[rule(3), rule(7)]), 8);
    }

    #[test]
    fn rejects_relative_executable() {
        assert!(validate_new_rule("1", "curl", RuleVerdict::Deny, "443").is_err());
    }

    #[test]
    fn accepts_absolute_path() {
        let rule = validate_new_rule("2", "/usr/bin/curl", RuleVerdict::Allow, "443").unwrap();
        assert_eq!(rule.id, 2);
        assert_eq!(rule.verdict, "allow");
        assert_eq!(rule.port, 443);
    }
}

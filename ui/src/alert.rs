//! Connection-alert state for the desktop prompt dialog.
#![forbid(unsafe_code)]

use interfire_proto::PromptRow;

/// Allow or Deny for a pending prompt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlertVerdict {
    Allow,
    Deny,
}

impl AlertVerdict {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Allow => "Allow",
            Self::Deny => "Deny",
        }
    }
}

/// Rule lifetime for a prompt answer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlertScope {
    Once,
    Session,
    Permanent,
}

impl AlertScope {
    pub const ALL: [Self; 3] = [Self::Once, Self::Session, Self::Permanent];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Session => "session",
            Self::Permanent => "permanent",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Session => "session",
            Self::Permanent => "permanent",
        }
    }

    /// Whether this answer creates or updates a durable / session rule.
    #[must_use]
    pub const fn rule_note(self) -> &'static str {
        match self {
            Self::Once => "Applies to this connection only (no durable rule).",
            Self::Session => "Creates a session rule until the application exits.",
            Self::Permanent => "Creates or updates a durable rule.",
        }
    }
}

/// Active connection alert dialog state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectionAlert {
    pub prompt: PromptRow,
    pub verdict: AlertVerdict,
    pub scope: AlertScope,
    pub details_open: bool,
    pub stale: bool,
    pub status_message: Option<String>,
}

impl ConnectionAlert {
    /// Open an alert for a live prompt (default focus: Deny / once).
    #[must_use]
    pub const fn from_prompt(prompt: PromptRow) -> Self {
        let stale = !prompt.can_answer();
        Self {
            prompt,
            verdict: AlertVerdict::Deny,
            scope: AlertScope::Once,
            details_open: false,
            stale,
            status_message: None,
        }
    }

    /// Refresh from the latest prompt list; mark stale when missing or expired.
    pub fn sync_from_list(&mut self, prompts: &[PromptRow]) {
        match prompts.iter().find(|row| row.id == self.prompt.id) {
            Some(row) => {
                self.prompt = row.clone();
                self.stale = !row.can_answer();
            }
            None => {
                self.stale = true;
            }
        }
    }

    /// Whether Allow/Deny controls may submit.
    #[must_use]
    pub const fn can_submit(&self) -> bool {
        !self.stale && self.prompt.can_answer()
    }

    /// Encode `v1 prompt-answer …` when the alert may still be answered.
    ///
    /// Returns `None` when the prompt is stale or expired so Allow/Deny stay disabled.
    #[must_use]
    pub fn answer_frame(&self, verdict: AlertVerdict) -> Option<String> {
        if !self.can_submit() {
            return None;
        }
        Some(format!(
            "v1 prompt-answer {} {} {}\n",
            self.prompt.id,
            verdict.as_str(),
            self.scope.as_str()
        ))
    }

    /// Pick the next answerable prompt, or keep a stale card when the queue is empty.
    pub fn advance(current: Option<Self>, prompts: &[PromptRow]) -> Option<Self> {
        let answerable = prompts.iter().find(|row| row.can_answer()).cloned();
        match (current, answerable) {
            (None, Some(prompt)) => Some(Self::from_prompt(prompt)),
            (None, None) => None,
            (Some(mut alert), Some(prompt)) => {
                alert.sync_from_list(prompts);
                if alert.can_submit() {
                    Some(alert)
                } else {
                    Some(Self::from_prompt(prompt))
                }
            }
            (Some(mut alert), None) => {
                alert.sync_from_list(prompts);
                Some(alert)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AlertScope, AlertVerdict, ConnectionAlert};
    use interfire_proto::PromptRow;

    fn prompt(id: u64, remaining_secs: u64) -> PromptRow {
        PromptRow {
            id,
            executable: "/usr/bin/curl".into(),
            destination: "1.2.3.4".into(),
            port: 443,
            protocol: "tcp".into(),
            remaining_secs,
        }
    }

    #[test]
    fn defaults_to_deny_once_and_shows_full_path() {
        let alert = ConnectionAlert::from_prompt(prompt(7, 12));
        assert_eq!(alert.verdict, AlertVerdict::Deny);
        assert_eq!(alert.scope, AlertScope::Once);
        assert!(alert.can_submit());
        assert!(alert.prompt.executable.starts_with('/'));
    }

    #[test]
    fn expired_prompt_disables_submit() {
        let alert = ConnectionAlert::from_prompt(prompt(1, 0));
        assert!(alert.stale);
        assert!(!alert.can_submit());
    }

    #[test]
    fn missing_prompt_marks_stale() {
        let mut alert = ConnectionAlert::from_prompt(prompt(1, 20));
        alert.sync_from_list(&[prompt(2, 20)]);
        assert!(alert.stale);
        assert!(!alert.can_submit());
    }

    #[test]
    fn advance_replaces_stale_with_next_answerable() {
        let stale = ConnectionAlert::from_prompt(prompt(1, 0));
        let next = ConnectionAlert::advance(Some(stale), &[prompt(1, 0), prompt(2, 9)]).unwrap();
        assert_eq!(next.prompt.id, 2);
        assert!(next.can_submit());
    }

    #[test]
    fn permanent_scope_notes_durable_rule() {
        assert!(AlertScope::Permanent.rule_note().contains("durable"));
        assert!(AlertScope::Once.rule_note().contains("no durable"));
    }

    #[test]
    fn allow_and_deny_frames_encode_with_scope() {
        let mut alert = ConnectionAlert::from_prompt(prompt(9, 15));
        assert_eq!(
            alert.answer_frame(AlertVerdict::Allow).as_deref(),
            Some("v1 prompt-answer 9 allow once\n")
        );
        alert.scope = AlertScope::Session;
        assert_eq!(
            alert.answer_frame(AlertVerdict::Deny).as_deref(),
            Some("v1 prompt-answer 9 deny session\n")
        );
        alert.scope = AlertScope::Permanent;
        assert_eq!(
            alert.answer_frame(AlertVerdict::Allow).as_deref(),
            Some("v1 prompt-answer 9 allow permanent\n")
        );
    }

    #[test]
    fn stale_and_expired_disable_allow_deny_frames() {
        let expired = ConnectionAlert::from_prompt(prompt(3, 0));
        assert!(expired.answer_frame(AlertVerdict::Allow).is_none());
        assert!(expired.answer_frame(AlertVerdict::Deny).is_none());

        let mut missing = ConnectionAlert::from_prompt(prompt(4, 20));
        missing.sync_from_list(&[]);
        assert!(missing.stale);
        assert!(missing.answer_frame(AlertVerdict::Allow).is_none());
        assert!(missing.answer_frame(AlertVerdict::Deny).is_none());
    }
}

//! Synthetic RSS probe load for release memory gates (no GPUI imports).
#![forbid(unsafe_code)]

use interfire_proto::{MAX_LOG_RECORDS_PER_SUBSCRIBER, MAX_PENDING_PROMPTS, PromptRow};

use crate::alert::ConnectionAlert;
use crate::log_buf::LogBuffer;
use crate::section::Section;
use crate::tray::TrayState;

/// Which memory scenario `interfire-ui` should stage for `make memcheck-ui`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RssProbeMode {
    Idle,
    PromptLoad,
}

impl RssProbeMode {
    /// Parse `--rss-probe=idle|prompt-load`.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "idle" => Some(Self::Idle),
            "prompt-load" => Some(Self::PromptLoad),
            _ => None,
        }
    }
}

/// Staged prompt-load state applied before the first paint settles.
#[derive(Clone, Debug)]
pub struct PromptLoadFixture {
    pub prompts: Vec<PromptRow>,
    pub alert: ConnectionAlert,
    pub log: LogBuffer,
    pub section: Section,
    pub tray_state: TrayState,
}

/// Build a full pending-prompt queue plus a capped audit buffer.
#[must_use]
#[hotpath::measure]
pub fn prompt_load_fixture() -> PromptLoadFixture {
    let prompts: Vec<PromptRow> = (1..=MAX_PENDING_PROMPTS as u64)
        .map(|id| PromptRow {
            id,
            executable: format!("/usr/bin/rss-probe-{id}"),
            destination: "203.0.113.10".into(),
            port: 443,
            protocol: "tcp".into(),
            remaining_secs: 30,
        })
        .collect();
    let alert = ConnectionAlert::from_prompt(prompts[0].clone());
    let mut log = LogBuffer::new();
    for sequence in 1..=MAX_LOG_RECORDS_PER_SUBSCRIBER as u64 {
        log.push_line(sequence, "rss-probe-load");
    }
    PromptLoadFixture {
        prompts,
        alert,
        log,
        section: Section::Log,
        tray_state: TrayState::Prompting,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_probe_modes() {
        assert_eq!(RssProbeMode::parse("idle"), Some(RssProbeMode::Idle));
        assert_eq!(
            RssProbeMode::parse("prompt-load"),
            Some(RssProbeMode::PromptLoad)
        );
        assert_eq!(RssProbeMode::parse("nope"), None);
    }

    #[test]
    fn prompt_load_fills_contract_caps() {
        let fixture = prompt_load_fixture();
        assert_eq!(fixture.prompts.len(), MAX_PENDING_PROMPTS);
        assert_eq!(
            fixture.log.visible_window(10).total,
            MAX_LOG_RECORDS_PER_SUBSCRIBER
        );
        assert!(fixture.alert.can_submit());
        assert_eq!(fixture.tray_state, TrayState::Prompting);
        assert_eq!(fixture.section, Section::Log);
    }
}

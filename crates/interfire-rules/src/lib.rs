//! Deterministic application-firewall rule matching.
#![forbid(unsafe_code)]

use std::fs;
use std::io::Write;
use std::net::IpAddr;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Tcp,
    Udp,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Outbound,
    Inbound,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Allow,
    Deny,
    Prompt,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Once,
    Session,
    Permanent,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Connection {
    pub executable: String,
    pub protocol: Protocol,
    pub direction: Direction,
    pub address: IpAddr,
    pub hostname: Option<String>,
    pub port: u16,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Rule {
    pub id: u64,
    pub executable: String,
    pub protocol: Option<Protocol>,
    pub direction: Option<Direction>,
    pub address: Option<IpAddr>,
    pub hostname: Option<String>,
    pub port: Option<u16>,
    pub verdict: Verdict,
    pub scope: Scope,
}

impl Rule {
    /// Return whether this rule applies to `connection`.
    #[must_use]
    pub fn matches(&self, connection: &Connection) -> bool {
        self.executable == connection.executable
            && self
                .protocol
                .is_none_or(|value| value == connection.protocol)
            && self
                .direction
                .is_none_or(|value| value == connection.direction)
            && self.address.is_none_or(|value| value == connection.address)
            && self.port.is_none_or(|value| value == connection.port)
            && self.hostname.as_ref().is_none_or(|name| {
                connection
                    .hostname
                    .as_ref()
                    .is_some_and(|host| host == name)
            })
    }

    /// More constrained rules win; ID makes otherwise equal rules stable.
    #[must_use]
    pub fn precedence(&self) -> (u8, u64) {
        let constrained = u8::try_from(
            [
                self.protocol.is_some(),
                self.direction.is_some(),
                self.address.is_some(),
                self.hostname.is_some(),
                self.port.is_some(),
            ]
            .into_iter()
            .filter(|value| *value)
            .count(),
        )
        .unwrap_or(u8::MAX);
        (constrained, self.id)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct RuleSet {
    rules: Vec<Rule>,
}

impl RuleSet {
    /// Insert a validated rule.
    ///
    /// # Errors
    ///
    /// Returns [`RuleError::ExecutableMustBeAbsolute`] when the executable is
    /// empty or relative, or [`RuleError::DuplicateId`] when `rule.id` is already
    /// present.
    pub fn insert(&mut self, rule: Rule) -> Result<(), RuleError> {
        if rule.executable.is_empty() || !rule.executable.starts_with('/') {
            return Err(RuleError::ExecutableMustBeAbsolute);
        }
        if self.rules.iter().any(|existing| existing.id == rule.id) {
            return Err(RuleError::DuplicateId(rule.id));
        }
        self.rules.push(rule);
        Ok(())
    }

    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.rules.len();
        self.rules.retain(|rule| rule.id != id);
        before != self.rules.len()
    }

    #[must_use]
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// Highest-precedence matching rule wins; otherwise [`Verdict::Prompt`].
    #[must_use]
    pub fn verdict_for(&self, connection: &Connection) -> Verdict {
        self.rules
            .iter()
            .filter(|rule| rule.matches(connection))
            .max_by_key(|rule| rule.precedence())
            .map_or(Verdict::Prompt, |rule| rule.verdict)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RuleError {
    #[error("duplicate rule id {0}")]
    DuplicateId(u64),
    #[error("executable path must be absolute")]
    ExecutableMustBeAbsolute,
}

#[derive(Debug, thiserror::Error)]
pub enum PersistenceError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid TOML: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("cannot serialize rules: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("unsupported schema version {0}")]
    UnsupportedSchema(u32),
    #[error("invalid rule: {0}")]
    InvalidRule(#[from] RuleError),
}

#[derive(Deserialize, Serialize)]
struct RulesDocument {
    schema_version: u32,
    #[serde(default)]
    rules: Vec<Rule>,
}

pub struct RulesStore {
    path: PathBuf,
}

impl RulesStore {
    pub const SCHEMA_VERSION: u32 = 1;

    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Load rules from disk, or an empty set when the file is missing.
    ///
    /// # Errors
    ///
    /// Returns I/O, TOML parse, schema, or rule-validation failures.
    pub fn load(&self) -> Result<RuleSet, PersistenceError> {
        if !self.path.exists() {
            return Ok(RuleSet::default());
        }
        let document: RulesDocument =
            toml::from_str(&fs::read_to_string(&self.path).map_err(PersistenceError::Io)?)
                .map_err(PersistenceError::Parse)?;
        if document.schema_version != Self::SCHEMA_VERSION {
            return Err(PersistenceError::UnsupportedSchema(document.schema_version));
        }
        let mut rules = RuleSet::default();
        for rule in document.rules {
            rules.insert(rule).map_err(PersistenceError::InvalidRule)?;
        }
        Ok(rules)
    }

    /// Atomically replace the on-disk rules file (mode `0600`).
    ///
    /// # Errors
    ///
    /// Returns serialize or filesystem failures while writing the temporary
    /// file, syncing, renaming, or setting permissions.
    pub fn save(&self, rules: &RuleSet) -> Result<(), PersistenceError> {
        let document = RulesDocument {
            schema_version: Self::SCHEMA_VERSION,
            rules: rules.rules.clone(),
        };
        let data = toml::to_string_pretty(&document).map_err(PersistenceError::Serialize)?;
        let parent = self.path.parent().ok_or_else(|| {
            PersistenceError::Io(std::io::Error::other("rules path has no parent"))
        })?;
        fs::create_dir_all(parent).map_err(PersistenceError::Io)?;
        let temporary = parent.join(format!(".rules-{}.tmp", std::process::id()));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(PersistenceError::Io)?;
        file.write_all(data.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(PersistenceError::Io)?;
        fs::rename(&temporary, &self.path).map_err(PersistenceError::Io)?;
        fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600))
            .map_err(PersistenceError::Io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection() -> Connection {
        Connection {
            executable: "/usr/bin/curl".into(),
            protocol: Protocol::Tcp,
            direction: Direction::Outbound,
            address: "203.0.113.42".parse().unwrap(),
            hostname: Some("example.test".into()),
            port: 443,
        }
    }

    fn rule(id: u64, verdict: Verdict) -> Rule {
        Rule {
            id,
            executable: "/usr/bin/curl".into(),
            protocol: None,
            direction: Some(Direction::Outbound),
            address: None,
            hostname: None,
            port: None,
            verdict,
            scope: Scope::Permanent,
        }
    }

    #[test]
    fn defaults_to_prompt() {
        assert_eq!(
            RuleSet::default().verdict_for(&connection()),
            Verdict::Prompt
        );
    }

    #[test]
    fn specific_rule_beats_generic_rule() {
        let mut rules = RuleSet::default();
        rules.insert(rule(1, Verdict::Deny)).unwrap();
        let mut allow = rule(2, Verdict::Allow);
        allow.port = Some(443);
        rules.insert(allow).unwrap();
        assert_eq!(rules.verdict_for(&connection()), Verdict::Allow);
    }

    #[test]
    fn equal_specificity_uses_newer_id_deterministically() {
        let mut rules = RuleSet::default();
        rules.insert(rule(1, Verdict::Allow)).unwrap();
        rules.insert(rule(2, Verdict::Deny)).unwrap();
        assert_eq!(rules.verdict_for(&connection()), Verdict::Deny);
    }

    #[test]
    fn hostname_never_matches_when_missing() {
        let mut rules = RuleSet::default();
        let mut host_rule = rule(1, Verdict::Allow);
        host_rule.hostname = Some("example.test".into());
        rules.insert(host_rule).unwrap();
        let mut event = connection();
        event.hostname = None;
        assert_eq!(rules.verdict_for(&event), Verdict::Prompt);
    }

    #[test]
    fn store_round_trip_is_private_and_atomic() {
        let path =
            std::env::temp_dir().join(format!("interfire-rules-{}.toml", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let store = RulesStore::new(&path);
        let mut rules = RuleSet::default();
        rules.insert(rule(7, Verdict::Allow)).unwrap();
        store.save(&rules).unwrap();
        assert_eq!(store.load().unwrap().rules(), rules.rules());
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn remove_rule_by_id() {
        let mut rules = RuleSet::default();
        rules.insert(rule(1, Verdict::Allow)).unwrap();
        assert!(rules.remove(1));
        assert!(!rules.remove(1));
        assert!(rules.rules().is_empty());
    }

    #[test]
    fn insert_rejects_relative_executable_and_duplicate_id() {
        let mut rules = RuleSet::default();
        let mut bad = rule(1, Verdict::Allow);
        bad.executable = "curl".into();
        assert_eq!(rules.insert(bad), Err(RuleError::ExecutableMustBeAbsolute));
        rules.insert(rule(2, Verdict::Allow)).unwrap();
        assert_eq!(
            rules.insert(rule(2, Verdict::Deny)),
            Err(RuleError::DuplicateId(2))
        );
    }

    #[test]
    fn load_missing_file_returns_empty_set() {
        let path = std::env::temp_dir().join(format!(
            "interfire-rules-missing-{}.toml",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let store = RulesStore::new(&path);
        assert!(store.load().unwrap().rules().is_empty());
    }

    #[test]
    fn store_path_accessor_and_rootless_save_error() {
        let path =
            std::env::temp_dir().join(format!("interfire-rules-path-{}.toml", std::process::id()));
        let store = RulesStore::new(&path);
        assert_eq!(store.path(), path.as_path());
        let rootless = RulesStore::new(PathBuf::from(""));
        assert!(matches!(
            rootless.save(&RuleSet::default()),
            Err(PersistenceError::Io(_))
        ));
    }

    #[test]
    fn load_rejects_unsupported_schema() {
        let path = std::env::temp_dir().join(format!(
            "interfire-rules-schema-{}.toml",
            std::process::id()
        ));
        std::fs::write(&path, "schema_version = 99\nrules = []\n").unwrap();
        let store = RulesStore::new(&path);
        assert!(matches!(
            store.load(),
            Err(PersistenceError::UnsupportedSchema(99))
        ));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rule_matches_protocol_and_address() {
        let mut host_rule = rule(1, Verdict::Deny);
        host_rule.protocol = Some(Protocol::Udp);
        host_rule.address = Some("203.0.113.42".parse().unwrap());
        let mut event = connection();
        event.protocol = Protocol::Udp;
        assert!(host_rule.matches(&event));
        event.protocol = Protocol::Tcp;
        assert!(!host_rule.matches(&event));
    }
}

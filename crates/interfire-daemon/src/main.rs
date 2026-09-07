#![forbid(unsafe_code)]

mod audit;
mod dns;
mod ipc;
mod nfqueue;
mod observe;
mod packet;
mod pending;
mod policy;
mod process;
mod prompts;
mod shared;

use std::env;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use interfire_ebpf::Observer;
use interfire_proto::{DEFAULT_AUDIT_PATH, DEFAULT_RULES_PATH, DEFAULT_SOCKET_PATH};
use interfire_rules::RulesStore;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use crate::shared::Shared;

fn main() -> io::Result<()> {
    init_tracing();
    let options = Options::from_env();
    let store = RulesStore::new(&options.rules_path);
    let rules = store.load().map_err(io::Error::other)?;
    let rule_count = rules.rules().len();

    let (observer, observation) = start_observation(options.skip_ebpf);
    let shared = Arc::new(Shared::new(
        rules,
        store,
        observation,
        4_096,
        Duration::from_secs(5),
        1_024,
        options.audit_path.clone(),
    )?);

    if let Some(observer) = observer {
        let shared_observe = Arc::clone(&shared);
        thread::spawn(move || observe::run(observer, &shared_observe));
    }

    if options.skip_nfqueue {
        info!("NFQUEUE skipped (--no-nfqueue); enforcement=none");
    } else {
        let shared_queue = Arc::clone(&shared);
        thread::spawn(move || nfqueue::run_or_degrade(&shared_queue));
    }

    let path = Path::new(&options.socket);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if path.exists() {
        fs::remove_file(path)?;
    }

    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    info!(socket = %path.display(), rules = rule_count, "interfired listening");
    info!(
        observation = shared.observation(),
        enforcement = shared.enforcement(),
        audit = %options.audit_path.display(),
        "pipeline status"
    );

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let shared = Arc::clone(&shared);
                thread::spawn(move || {
                    if let Err(error) = ipc::handle(stream, &shared) {
                        error!(%error, "request failed");
                    }
                });
            }
            Err(error) => error!(%error, "accept failed"),
        }
    }
    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(io::stderr)
        .try_init();
}

struct Options {
    socket: String,
    rules_path: String,
    audit_path: PathBuf,
    skip_ebpf: bool,
    skip_nfqueue: bool,
}

impl Options {
    fn from_env() -> Self {
        let mut socket = DEFAULT_SOCKET_PATH.to_owned();
        let mut rules_path = DEFAULT_RULES_PATH.to_owned();
        let mut audit_path = PathBuf::from(DEFAULT_AUDIT_PATH);
        let mut skip_ebpf = false;
        let mut skip_nfqueue = false;
        for argument in env::args().skip(1) {
            if let Some(value) = argument.strip_prefix("--socket=") {
                value.clone_into(&mut socket);
            } else if let Some(value) = argument.strip_prefix("--rules=") {
                value.clone_into(&mut rules_path);
            } else if let Some(value) = argument.strip_prefix("--audit=") {
                audit_path = PathBuf::from(value);
            } else if argument == "--no-ebpf" {
                skip_ebpf = true;
            } else if argument == "--no-nfqueue" {
                skip_nfqueue = true;
            }
        }
        Self {
            socket,
            rules_path,
            audit_path,
            skip_ebpf,
            skip_nfqueue,
        }
    }
}

fn start_observation(skip_ebpf: bool) -> (Option<Observer>, &'static str) {
    if skip_ebpf {
        info!("eBPF skipped (--no-ebpf); observation=degraded");
        return (None, "degraded");
    }
    match Observer::load_embedded_and_attach() {
        Ok(observer) => {
            info!("eBPF attached to tcp_v4_connect");
            (Some(observer), "attached")
        }
        Err(error) => {
            info!(%error, "eBPF unavailable; observation=degraded");
            (None, "degraded")
        }
    }
}

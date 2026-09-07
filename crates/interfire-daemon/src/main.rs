#![forbid(unsafe_code)]

mod process;

use std::env;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;

use interfire_ebpf::Observer;
use interfire_proto::{IPC_VERSION, MAX_FRAME_BYTES, Request, Response};
use interfire_rules::{Direction, Protocol, Rule, RuleSet, RulesStore, Scope, Verdict};

fn main() -> io::Result<()> {
    let self_pid = std::process::id();
    let self_start = process::parse_start_ticks(&fs::read_to_string("/proc/self/stat")?)
        .map_err(io::Error::other)?;
    let self_identity = process::resolve(self_pid, self_start).map_err(io::Error::other)?;
    let mut process_cache = process::ProcessCache::new(1_024);
    process_cache.insert(self_identity);
    debug_assert!(process_cache.get(self_pid, self_start).is_some());
    let socket = env::args()
        .skip(1)
        .find_map(|argument| argument.strip_prefix("--socket=").map(str::to_owned))
        .unwrap_or_else(|| "/run/interfire/interfired.sock".into());
    let rules_path = env::args()
        .skip(1)
        .find_map(|argument| argument.strip_prefix("--rules=").map(str::to_owned))
        .unwrap_or_else(|| "/etc/interfire/rules.toml".into());
    let skip_ebpf = env::args().skip(1).any(|argument| argument == "--no-ebpf");
    let store = RulesStore::new(rules_path);
    let mut rules = store.load().map_err(io::Error::other)?;

    let (_observer, observation) = start_observation(skip_ebpf);

    let path = Path::new(&socket);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if path.exists() {
        fs::remove_file(path)?;
    }

    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    eprintln!("interfired: listening on {}", path.display());
    eprintln!("interfired: loaded {} rule(s)", rules.rules().len());
    eprintln!("interfired: process cache capacity {}", 1_024);
    eprintln!("interfired: observation={observation} enforcement=none");
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if let Err(error) = respond(stream, &mut rules, &store, observation) {
                    eprintln!("interfired: request failed: {error}");
                }
            }
            Err(error) => eprintln!("interfired: accept failed: {error}"),
        }
    }
    Ok(())
}

fn start_observation(skip_ebpf: bool) -> (Option<Observer>, &'static str) {
    if skip_ebpf {
        eprintln!("interfired: eBPF skipped (--no-ebpf); observation=degraded");
        return (None, "degraded");
    }
    match Observer::load_embedded_and_attach() {
        Ok(observer) => {
            eprintln!("interfired: eBPF attached to tcp_v4_connect");
            (Some(observer), "attached")
        }
        Err(error) => {
            eprintln!("interfired: eBPF unavailable ({error}); observation=degraded");
            (None, "degraded")
        }
    }
}

fn respond(
    mut stream: UnixStream,
    rules: &mut RuleSet,
    store: &RulesStore,
    observation: &'static str,
) -> io::Result<()> {
    let mut frame = String::new();
    let bytes = BufReader::new(stream.try_clone()?).read_line(&mut frame)?;
    let response = if bytes > MAX_FRAME_BYTES {
        Response::Error("frame_too_large")
    } else {
        match Request::parse(&frame) {
            Ok(Request::Ping) => Response::Pong,
            Ok(Request::Status) => Response::Status {
                enforcement: "none",
                observation,
                ipc_version: IPC_VERSION,
            },
            Ok(Request::RuleList) => Response::Rules(
                rules
                    .rules()
                    .iter()
                    .map(|rule| {
                        format!(
                            "{}|{}|{:?}|{}",
                            rule.id,
                            rule.executable,
                            rule.verdict,
                            rule.port.unwrap_or(0)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            Ok(Request::RuleDelete { id }) => {
                if !rules.remove(id) {
                    Response::Error("rule_not_found")
                } else if store.save(rules).is_err() {
                    Response::Error("persistence_failed")
                } else {
                    Response::Pong
                }
            }
            Ok(Request::RuleAdd {
                id,
                executable,
                verdict,
                port,
            }) => {
                let verdict = match verdict.as_str() {
                    "allow" => Verdict::Allow,
                    "deny" => Verdict::Deny,
                    "prompt" => Verdict::Prompt,
                    _ => return stream.write_all(b"v1 error invalid_verdict\n"),
                };
                let rule = Rule {
                    id,
                    executable,
                    protocol: Some(Protocol::Tcp),
                    direction: Some(Direction::Outbound),
                    address: None,
                    hostname: None,
                    port: Some(port),
                    verdict,
                    scope: Scope::Permanent,
                };
                match rules.insert(rule) {
                    Ok(()) if store.save(rules).is_ok() => Response::Pong,
                    Ok(()) => Response::Error("persistence_failed"),
                    Err(_) => Response::Error("invalid_rule"),
                }
            }
            Err(_) => Response::Error("malformed_request"),
        }
    };
    stream.write_all(response.encode().as_bytes())
}

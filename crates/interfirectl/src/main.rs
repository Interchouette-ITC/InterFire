#![forbid(unsafe_code)]

use std::env;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

use interfire_proto::DEFAULT_SOCKET_PATH;

fn main() -> io::Result<()> {
    let mut socket = DEFAULT_SOCKET_PATH.to_owned();
    let mut arguments = Vec::new();
    for argument in env::args().skip(1) {
        if let Some(value) = argument.strip_prefix("--socket=") {
            value.clone_into(&mut socket);
        } else {
            arguments.push(argument);
        }
    }

    let Some(command) = parse_command(&arguments) else {
        return usage();
    };

    let mut stream = UnixStream::connect(socket)?;
    stream.write_all(format!("v1 {command}\n").as_bytes())?;
    let mut response = String::new();
    BufReader::new(stream).read_line(&mut response)?;
    print!("{response}");
    Ok(())
}

fn parse_command(arguments: &[String]) -> Option<String> {
    match arguments {
        [] => Some("status".to_owned()),
        [command] if matches!(command.as_str(), "ping" | "status" | "pause" | "resume") => {
            Some(command.clone())
        }
        [traffic, status] if traffic == "traffic" && status == "status" => {
            Some("status".to_owned())
        }
        [traffic, action, rest @ ..] if traffic == "traffic" => parse_traffic_command(action, rest),
        [rules, list] if rules == "rules" && list == "list" => Some("rule-list".to_owned()),
        [rules, add, id, executable, verdict, port]
            if rules == "rules"
                && add == "add"
                && id.parse::<u64>().is_ok()
                && port.parse::<u16>().is_ok() =>
        {
            Some(format!("rule-add {id} {executable} {verdict} {port}"))
        }
        [rules, delete, id]
            if rules == "rules" && delete == "delete" && id.parse::<u64>().is_ok() =>
        {
            Some(format!("rule-delete {id}"))
        }
        [prompts, list] if prompts == "prompts" && list == "list" => Some("prompt-list".to_owned()),
        [prompts, answer, id, verdict, scope]
            if prompts == "prompts"
                && answer == "answer"
                && id.parse::<u64>().is_ok()
                && matches!(verdict.as_str(), "allow" | "deny")
                && matches!(scope.as_str(), "once" | "session" | "permanent") =>
        {
            Some(format!("prompt-answer {id} {verdict} {scope}"))
        }
        [dns, list] if dns == "dns" && list == "list" => Some("dns-list".to_owned()),
        [dns, note, hostname, ipv4] if dns == "dns" && note == "note" && !hostname.is_empty() => {
            Some(format!("dns-note {hostname} {ipv4}"))
        }
        [dns, note, hostname, ipv4, ttl]
            if dns == "dns"
                && note == "note"
                && !hostname.is_empty()
                && ttl.parse::<u64>().is_ok() =>
        {
            Some(format!("dns-note {hostname} {ipv4} {ttl}"))
        }
        [audit, tail] if audit == "audit" && tail == "tail" => Some("audit-tail".to_owned()),
        [audit, tail, limit]
            if audit == "audit" && tail == "tail" && limit.parse::<usize>().is_ok() =>
        {
            Some(format!("audit-tail {limit}"))
        }
        [audit, subscribe, id]
            if audit == "audit" && subscribe == "subscribe" && !id.is_empty() =>
        {
            Some(format!("audit-subscribe {id}"))
        }
        [audit, subscribe, id, since]
            if audit == "audit"
                && subscribe == "subscribe"
                && !id.is_empty()
                && since.starts_with("since=") =>
        {
            Some(format!("audit-subscribe {id} {since}"))
        }
        [network, status] if network == "network" && status == "status" => {
            Some("network-status".to_owned())
        }
        [network, install] if network == "network" && install == "install" => {
            Some("network-install".to_owned())
        }
        [network, remove] if network == "network" && remove == "remove" => {
            Some("network-remove".to_owned())
        }
        _ => None,
    }
}

fn parse_traffic_command(action: &str, rest: &[String]) -> Option<String> {
    let mut scope = "user";
    let mut direction = "out";
    for argument in rest {
        if let Some(value) = argument.strip_prefix("--scope=") {
            scope = value;
        } else {
            let value = argument.strip_prefix("--direction=")?;
            direction = value;
        }
    }
    if !matches!(scope, "user" | "machine") {
        return None;
    }
    match action {
        "block" if matches!(direction, "out" | "in" | "all") => {
            Some(format!("traffic-block scope={scope} direction={direction}"))
        }
        "unblock" => Some(format!("traffic-unblock scope={scope}")),
        _ => None,
    }
}

fn usage() -> io::Result<()> {
    eprintln!(
        "usage: interfirectl [--socket=PATH] <ping|status|pause|resume|traffic status|traffic block [--scope=user|machine] [--direction=out|in|all]|traffic unblock [--scope=user|machine]|rules …|prompts …|dns …|audit tail [N]|audit subscribe ID [since=N]|network status|network install|network remove>"
    );
    eprintln!(
        "machine traffic requires root (pkexec interfirectl traffic block --scope=machine …)"
    );
    eprintln!("one-shot CLI only; use interfire-tui for interactive browse/answer");
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "invalid command",
    ))
}

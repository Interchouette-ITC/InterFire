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

    let command = match arguments.as_slice() {
        [] => "status".to_owned(),
        [command] if matches!(command.as_str(), "ping" | "status") => command.clone(),
        [rules, list] if rules == "rules" && list == "list" => "rule-list".to_owned(),
        [rules, add, id, executable, verdict, port]
            if rules == "rules"
                && add == "add"
                && id.parse::<u64>().is_ok()
                && port.parse::<u16>().is_ok() =>
        {
            format!("rule-add {id} {executable} {verdict} {port}")
        }
        [rules, delete, id]
            if rules == "rules" && delete == "delete" && id.parse::<u64>().is_ok() =>
        {
            format!("rule-delete {id}")
        }
        [prompts, list] if prompts == "prompts" && list == "list" => "prompt-list".to_owned(),
        [prompts, answer, id, verdict, scope]
            if prompts == "prompts"
                && answer == "answer"
                && id.parse::<u64>().is_ok()
                && matches!(verdict.as_str(), "allow" | "deny")
                && matches!(scope.as_str(), "once" | "session" | "permanent") =>
        {
            format!("prompt-answer {id} {verdict} {scope}")
        }
        [dns, list] if dns == "dns" && list == "list" => "dns-list".to_owned(),
        [dns, note, hostname, ipv4] if dns == "dns" && note == "note" && !hostname.is_empty() => {
            format!("dns-note {hostname} {ipv4}")
        }
        [dns, note, hostname, ipv4, ttl]
            if dns == "dns"
                && note == "note"
                && !hostname.is_empty()
                && ttl.parse::<u64>().is_ok() =>
        {
            format!("dns-note {hostname} {ipv4} {ttl}")
        }
        [audit, tail] if audit == "audit" && tail == "tail" => "audit-tail".to_owned(),
        [audit, tail, limit]
            if audit == "audit" && tail == "tail" && limit.parse::<usize>().is_ok() =>
        {
            format!("audit-tail {limit}")
        }
        [audit, subscribe, id]
            if audit == "audit" && subscribe == "subscribe" && !id.is_empty() =>
        {
            format!("audit-subscribe {id}")
        }
        [audit, subscribe, id, since]
            if audit == "audit"
                && subscribe == "subscribe"
                && !id.is_empty()
                && since.starts_with("since=") =>
        {
            format!("audit-subscribe {id} {since}")
        }
        [network, status] if network == "network" && status == "status" => {
            "network-status".to_owned()
        }
        [network, install] if network == "network" && install == "install" => {
            "network-install".to_owned()
        }
        [network, remove] if network == "network" && remove == "remove" => {
            "network-remove".to_owned()
        }
        _ => return usage(),
    };

    let mut stream = UnixStream::connect(socket)?;
    stream.write_all(format!("v1 {command}\n").as_bytes())?;
    let mut response = String::new();
    BufReader::new(stream).read_line(&mut response)?;
    print!("{response}");
    Ok(())
}

fn usage() -> io::Result<()> {
    eprintln!(
        "usage: interfirectl [--socket=PATH] <ping|status|rules …|prompts …|dns …|audit tail [N]|audit subscribe ID [since=N]|network status|network install|network remove>"
    );
    eprintln!("one-shot CLI only; use interfire-tui for interactive browse/answer");
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "invalid command",
    ))
}

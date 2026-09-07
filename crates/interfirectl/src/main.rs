#![forbid(unsafe_code)]

use std::env;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

fn main() -> io::Result<()> {
    let mut socket = "/run/interfire/interfired.sock".to_owned();
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
        "usage: interfirectl [--socket=PATH] <ping|status|rules list|rules add ID PATH allow|deny|prompt PORT|rules delete ID|prompts list|prompts answer ID allow|deny once|session|permanent>"
    );
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "invalid command",
    ))
}

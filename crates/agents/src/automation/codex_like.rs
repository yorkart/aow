pub(super) fn codex_arguments(yolo: bool) -> Vec<String> {
    let mut args = Vec::new();
    if yolo {
        args.push("--yolo");
    } else {
        args.extend(["--ask-for-approval", "never"]);
    }
    args.extend(["exec", "--color", "never"]);
    if !yolo {
        args.extend(["--sandbox", "workspace-write"]);
    }
    args.push("-");
    args.into_iter().map(str::to_owned).collect()
}
pub(super) fn codex_session_from_line(line: &[u8]) -> Option<String> {
    if let Ok(event) = serde_json::from_slice::<serde_json::Value>(line)
        && event.get("type")?.as_str()? == "thread.started"
    {
        return session_id(event.get("thread_id")?.as_str()?);
    }

    let line = std::str::from_utf8(line).ok()?.trim();
    let id = line.strip_prefix("session id:")?.trim();
    session_id(id)
}

fn session_id(id: &str) -> Option<String> {
    (!id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control)).then(|| id.to_owned())
}

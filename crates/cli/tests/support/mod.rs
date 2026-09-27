use serde_json::Value;
use std::{
    io::{Read, Write},
    os::unix::net::UnixListener,
    process::{Command, Output, Stdio},
    thread,
    time::Duration,
};

pub fn run(
    args: &[&str],
    input: Option<&str>,
    status: u16,
    reply: Value,
) -> (Output, String, Value) {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("cli")).unwrap();
    let listener = UnixListener::bind(directory.path().join("cli/cli.sock")).unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut chunk = [0; 4096];
        let (header, body) = loop {
            let n = stream.read(&mut chunk).unwrap();
            assert_ne!(n, 0);
            bytes.extend_from_slice(&chunk[..n]);
            if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                let header = String::from_utf8(bytes[..end].to_vec()).unwrap();
                let length: usize = header
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse().unwrap())
                    })
                    .unwrap_or(0);
                if bytes.len() >= end + 4 + length {
                    let body = if length == 0 {
                        Value::Null
                    } else {
                        serde_json::from_slice(&bytes[end + 4..end + 4 + length]).unwrap()
                    };
                    break (header, body);
                }
            }
        };
        let reply = reply.to_string();
        write!(stream, "HTTP/1.1 {status} Reply\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}", reply.len()).unwrap();
        (header, body)
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_aow-cli"))
        .arg("--state-dir")
        .arg(directory.path())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(input) = input {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
    }
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();
    let (header, body) = server.join().unwrap();
    (output, header, body)
}

//! Regression checks using the installed Hermes APIs and a disposable CLI.
//! AOW_HERMES_TEST_PYTHON selects its venv Python; AOW_HERMES_TEST_SOURCE may override its checkout.
#![cfg(feature = "sessions")]

use aow_agents::{
    Agent,
    sessions::{
        self, AgentSessionProvider, SessionRoots, snapshot,
        tail::SessionTail,
        tracking::{AgentSessionTracker, LiveSessionContext, SessionResolution},
    },
};
use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Command, Stdio},
};

fn python() -> std::path::PathBuf {
    std::env::var_os("AOW_HERMES_TEST_PYTHON")
        .expect("installed Hermes venv Python")
        .into()
}

fn source() -> std::path::PathBuf {
    std::env::var_os("AOW_HERMES_TEST_SOURCE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| python().ancestors().nth(3).unwrap().to_path_buf())
}

fn native(root: &Path, code: &str) -> String {
    let output = Command::new(python())
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONPATH", source())
        .env("HERMES_HOME", root)
        .env("HOME", root)
        .current_dir(root)
        .args(["-c", code])
        .arg(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn prepare(root: &Path) {
    native(
        root,
        r#"
import sys
from pathlib import Path
from hermes_state import SessionDB
db = SessionDB(Path(sys.argv[1]) / 'state.db')
db.create_session('review_compaction', 'cli', cwd=str(Path(sys.argv[1]).resolve()))
for n in range(1, 4):
    db.append_message('review_compaction', 'user', f'Question {n}')
    db.append_message('review_compaction', 'assistant', f'Answer {n}', finish_reason='stop')
db.append_message('review_compaction', 'user', 'Question 4')
db.close()
"#,
    );
}

fn compact(root: &Path) {
    native(
        root,
        r#"
import sys
from pathlib import Path
from hermes_state import SessionDB
db = SessionDB(Path(sys.argv[1]) / 'state.db')
history = db.get_messages_as_conversation('review_compaction')
db.archive_and_compact('review_compaction', [
    {'role': 'user', 'content': '[CONTEXT SUMMARY]: Questions 1 and 2 summarized'},
    *history[-3:]
])
print('native counts:', len(db.get_messages('review_compaction', include_inactive=True)), len(db.get_messages('review_compaction')))
db.close()
"#,
    );
}

fn locator(root: &Path) -> sessions::AgentSessionLocator {
    let env = [("HERMES_HOME".to_owned(), root.to_path_buf())].into();
    sessions::find_session(
        "hermes",
        "review_compaction",
        SessionRoots::from_configuration(root, &env),
    )
    .unwrap()
    .locator()
}

#[test]
#[ignore = "requires AOW_HERMES_TEST_PYTHON; uses only temporary Hermes profiles"]
fn native_compaction_should_not_replay_completed_turns() {
    let dir = tempfile::tempdir().unwrap();
    prepare(dir.path());
    let mut tail = SessionTail::from_eof(locator(dir.path())).unwrap();
    assert!(tail.poll().unwrap().is_empty());
    compact(dir.path());
    let events = tail.poll().unwrap();
    eprintln!(
        "events caused only by native compaction: {}",
        serde_json::to_string(&events).unwrap()
    );
    assert!(
        events.is_empty(),
        "native compaction replayed an old completion"
    );
}

#[test]
#[ignore = "requires AOW_HERMES_TEST_PYTHON; uses only temporary Hermes profiles"]
fn native_compaction_should_keep_original_user_history() {
    let dir = tempfile::tempdir().unwrap();
    prepare(dir.path());
    let loc = locator(dir.path());
    let before = serde_json::to_value(snapshot::read(loc.clone()).unwrap()).unwrap();
    compact(dir.path());
    let after = serde_json::to_value(snapshot::read(loc).unwrap()).unwrap();
    let users = |value: &serde_json::Value| {
        value["turns"]
            .as_array()
            .unwrap()
            .iter()
            .map(|turn| turn["user"]["text"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    eprintln!(
        "before compaction: {:?}; after compaction: {:?}",
        users(&before),
        users(&after)
    );
    assert_eq!(before["turns"], after["turns"]);
}

#[test]
#[ignore = "requires AOW_HERMES_TEST_PYTHON; uses only temporary Hermes profiles"]
fn native_repeated_compaction_covers_live_resumed_and_mixed_timestamps() {
    for mode in ["keep", "fresh", "mixed"] {
        for merged in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            prepare(dir.path());
            native(
                dir.path(),
                r#"
import sys
from pathlib import Path
from hermes_state import SessionDB
db = SessionDB(Path(sys.argv[1]) / 'state.db')
for _ in range(12):
    db.append_message('review_compaction', 'user', 'Repeated question')
    db.append_message('review_compaction', 'assistant', 'Repeated answer', finish_reason='stop')
db.close()
"#,
            );
            let loc = locator(dir.path());
            let before = serde_json::to_value(snapshot::read(loc.clone()).unwrap()).unwrap();
            let mut tail = SessionTail::from_eof(loc.clone()).unwrap();
            for iteration in 0..3 {
                let code = r#"
import sys
from pathlib import Path
from hermes_state import SessionDB
db = SessionDB(Path(sys.argv[1]) / 'state.db')
history = db.get_messages_as_conversation('review_compaction')
for index, message in enumerate(history):
    if '__MODE__' == 'fresh' or ('__MODE__' == 'mixed' and index % 2):
        message.pop('timestamp', None)
head, tail = history[:2], history[-6:]
summary = '[CONTEXT SUMMARY]: Previous turns'
if __MERGED__:
    tail[0]['content'] = '[PRIOR CONTEXT — for reference only; not a new message]\n' + tail[0]['content'] + '\n[END OF PRIOR CONTEXT — COMPACTION SUMMARY BELOW]\n' + summary
    compressed = head + tail
else:
    compressed = head + [{'role': 'user', 'content': summary}] + tail
db.archive_and_compact('review_compaction', compressed)
db.close()
"#.replace("__MODE__", mode).replace("__MERGED__", if merged { "True" } else { "False" });
                native(dir.path(), &code);
                let after = serde_json::to_value(snapshot::read(loc.clone()).unwrap()).unwrap();
                assert_eq!(
                    before["turns"], after["turns"],
                    "{mode} merged={merged} iteration={iteration}"
                );
                assert!(
                    tail.poll().unwrap().is_empty(),
                    "{mode} merged={merged} iteration={iteration}"
                );
            }
        }
    }
}

#[test]
#[ignore = "requires AOW_HERMES_TEST_PYTHON; uses only temporary Hermes profiles"]
fn native_custom_root_should_follow_native_active_profile() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("profiles/work")).unwrap();
    std::fs::write(dir.path().join("active_profile"), "work\n").unwrap();
    let actual = native(
        dir.path(),
        r#"
import ast, os, sys
from pathlib import Path
source = (Path(os.environ['PYTHONPATH']) / 'hermes_cli/main.py').read_text()
node = next(n for n in ast.parse(source).body if isinstance(n, ast.FunctionDef) and n.name == '_apply_profile_override')
sys.argv = ['hermes', '--cli']
exec(compile(ast.Module(body=[node], type_ignores=[]), 'native_profile_override', 'exec'))
_apply_profile_override()
print(os.environ['HERMES_HOME'])
"#,
    );
    let env = [("HERMES_HOME".to_owned(), dir.path().to_path_buf())].into();
    let aow = Agent::Hermes
        .sessions()
        .unwrap()
        .session_root(dir.path(), &env);
    eprintln!("native home: {actual}; AoW home: {}", aow.display());
    assert_eq!(aow, Path::new(&actual));
}

#[tokio::test]
#[ignore = "requires AOW_HERMES_TEST_PYTHON and PTY access; starts an isolated Hermes CLI"]
async fn native_new_unpersisted_process_should_not_attach_another_session() {
    let dir = tempfile::tempdir().unwrap();
    prepare(dir.path());
    let mut child = Command::new(python())
        .env("PYTHONDONTWRITEBYTECODE", "1").env("HERMES_HOME", dir.path())
        .env("HOME", dir.path()).current_dir(dir.path())
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .arg("-c").arg(r#"
import fcntl, json, os, pty, select, sqlite3, struct, subprocess, sys, termios, time
from pathlib import Path
root = Path.cwd()
(root / 'config.yaml').write_text(json.dumps({
    'model': {'provider': 'custom', 'default': 'aow-review', 'base_url': 'http://127.0.0.1:1/v1', 'api_key': 'local-review', 'context_length': 131072},
    'display': {'interface': 'cli'}, 'memory': {'memory_enabled': False, 'user_profile_enabled': False},
    'terminal': {'backend': 'local'}
}))
env = {key: os.environ[key] for key in ('PATH', 'HOME', 'HERMES_HOME', 'SHELL', 'LANG', 'TMPDIR', 'USER', 'LOGNAME') if key in os.environ}
env.update(PYTHONDONTWRITEBYTECODE='1', TERM='xterm-256color', HERMES_SKIP_UPDATE_CHECK='1',
    XDG_CACHE_HOME=str(root/'cache'), OPENAI_API_KEY='local-review', OPENAI_BASE_URL='http://127.0.0.1:1/v1')
master, slave = pty.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 48, 160, 0, 0))
cli = subprocess.Popen([str(Path(sys.executable).with_name('hermes')), '--cli'],
    cwd=root, env=env, stdin=slave, stdout=slave, stderr=slave, start_new_session=True)
os.close(slave)
screen = bytearray()
try:
    deadline = time.monotonic() + 35
    ready = False
    while time.monotonic() < deadline and cli.poll() is None:
        if not select.select([master], [], [], 0.2)[0]:
            continue
        chunk = os.read(master, 65536)
        screen.extend(chunk)
        if b'\x1b[6n' in chunk:
            os.write(master, b'\x1b[1;1R')
        if '❯'.encode() in screen:
            ready = True
            break
    db = sqlite3.connect(f'file:{root / "state.db"}?mode=ro', uri=True)
    ids = [row[0] for row in db.execute('SELECT id FROM sessions')]
    print(json.dumps({'pid': cli.pid, 'ready': ready, 'native_session_ids': ids}), flush=True)
    if ready:
        sys.stdin.readline()
        os.write(master, b'/exit\r')
        cli.wait(timeout=10)
    else:
        print(screen.decode(errors='replace')[-2000:], file=sys.stderr)
finally:
    if cli.poll() is None:
        cli.terminate()
        cli.wait(timeout=5)
    os.close(master)
"#).spawn().unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let info: serde_json::Value = serde_json::from_str(&line).unwrap();
    eprintln!("native new CLI state: {info}");
    let env = [
        ("HERMES_HOME".to_owned(), dir.path().to_path_buf()),
        ("HOME".to_owned(), dir.path().to_path_buf()),
    ]
    .into();
    let cwd = dir.path().canonicalize().unwrap();
    let result = Agent::Hermes
        .session_tracking()
        .unwrap()
        .resolve_live_session(LiveSessionContext {
            pid: info["pid"].as_i64().map(|pid| pid as i32),
            cwd: cwd.to_str().unwrap(),
            title: "hermes",
            environment: &env,
        })
        .await;
    child.stdin.take().unwrap().write_all(b"done\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(info["ready"], true);
    assert_eq!(
        info["native_session_ids"],
        serde_json::json!(["review_compaction"])
    );
    eprintln!("new CLI before first persisted turn resolves to: {result:?}");
    assert_eq!(
        result,
        SessionResolution::NotFound,
        "an unrelated PID was associated using only cwd"
    );
}

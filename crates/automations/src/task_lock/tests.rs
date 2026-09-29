use std::{
    os::unix::process::CommandExt,
    process::{Child, Command, Stdio},
};

use super::{Process, process::group_alive};

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn process_groups_distinguish_live_reused_zombie_and_reaped_owners() {
    let mut child = ChildGuard(
        Command::new("/bin/cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .unwrap(),
    );
    let owner = Process {
        process_group: child.0.id() as i32,
        started_before: u64::MAX,
    };
    assert!(group_alive(&owner).unwrap());
    assert!(
        !group_alive(&Process {
            process_group: owner.process_group,
            started_before: 0,
        })
        .unwrap()
    );

    // EOF lets cat exit, but WNOWAIT leaves the zombie under our ownership.
    // macOS can return EPERM for kill(-pgid, 0) in this exact state.
    drop(child.0.stdin.take());
    let mut status = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
    loop {
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                child.0.id() as libc::id_t,
                &mut status,
                libc::WEXITED | libc::WNOWAIT,
            )
        };
        if result == 0 {
            break;
        }
        let error = std::io::Error::last_os_error();
        assert_eq!(error.kind(), std::io::ErrorKind::Interrupted, "{error}");
    }
    assert!(!group_alive(&owner).unwrap());

    assert!(child.0.wait().unwrap().success());
    assert!(!group_alive(&owner).unwrap());
}

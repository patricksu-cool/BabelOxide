use anyhow::{Context, Result, ensure};

/// A zombie has already released its pipes and cannot execute. Waiting for its
/// PID to disappear would deadlock before Tokio gets to reap the direct child.
fn snapshot_has_live_group(snapshot: &str, group: i32) -> Result<bool> {
    ensure!(group > 1, "Invalid owned process group");
    for line in snapshot.lines().filter(|line| !line.trim().is_empty()) {
        let mut fields = line.split_whitespace();
        let actual: i32 = fields
            .next()
            .context("Missing process group")?
            .parse()
            .context("Invalid process group snapshot")?;
        let state = fields.next().context("Missing process state")?;
        if actual == group && !state.starts_with(['Z', 'X']) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(unix)]
mod system {
    use super::*;
    use std::{
        sync::atomic::{AtomicBool, Ordering},
        time::Duration,
    };
    use tokio::process::{Child, Command};

    pub struct ProcessScope {
        group: libc::pid_t,
        active: AtomicBool,
    }

    impl ProcessScope {
        fn signal_group(&self) -> Result<bool> {
            // SAFETY: group is a child PID > 1 whose process group was created
            // by process_group(0) before exec. Never signal our parent's group.
            if unsafe { libc::kill(-self.group, libc::SIGKILL) } == 0 {
                return Ok(true);
            }
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                Ok(false)
            } else {
                Err(error).context("Terminate owned process group")
            }
        }

        pub async fn terminate_and_wait(&self) -> Result<()> {
            if !self.active.load(Ordering::Acquire) {
                return Ok(());
            }
            if self.signal_group()? {
                tokio::time::timeout(Duration::from_secs(5), async {
                    loop {
                        // Both macOS and Linux provide this system ps. Request
                        // only group IDs and states, never command lines. A
                        // signal-0 probe alone also counts unreaped zombies.
                        let snapshot = Command::new("/bin/ps")
                            .args(["-A", "-o", "pgid=,state="])
                            .kill_on_drop(true)
                            .output()
                            .await
                            .context("Observe owned process group")?;
                        ensure!(
                            snapshot.status.success(),
                            "Cannot observe owned process group"
                        );
                        if !snapshot_has_live_group(
                            std::str::from_utf8(&snapshot.stdout)?,
                            self.group,
                        )? {
                            return Ok::<_, anyhow::Error>(());
                        }
                        // A member may have forked while the first kill was
                        // delivered. Its children still inherit this group.
                        self.signal_group()?;
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await
                .context("Owned process group cleanup timed out")??;
            }
            self.active.store(false, Ordering::Release);
            Ok(())
        }
    }

    impl Drop for ProcessScope {
        fn drop(&mut self) {
            if self.active.load(Ordering::Acquire) {
                let _ = self.signal_group();
            }
        }
    }

    pub fn prepare(command: &mut Command) {
        command.process_group(0);
    }

    pub fn contain_and_resume(child: &mut Child) -> Result<ProcessScope> {
        let group: libc::pid_t = child
            .id()
            .context("Worker PID unavailable")?
            .try_into()
            .context("Invalid worker PID")?;
        ensure!(group > 1, "Invalid owned process group");
        Ok(ProcessScope {
            group,
            active: AtomicBool::new(true),
        })
    }
}

#[cfg(unix)]
pub use system::{ProcessScope, contain_and_resume, prepare};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observes_only_live_members_of_the_owned_group() {
        let snapshot = " 12 S\n42 Z\n42 R+\n";
        assert!(snapshot_has_live_group(snapshot, 42).unwrap());
        assert!(!snapshot_has_live_group(snapshot, 99).unwrap());
    }

    #[test]
    fn zombies_and_dead_processes_do_not_block_reaping() {
        assert!(!snapshot_has_live_group("42 Z+\n42 X\n7 S\n", 42).unwrap());
        assert!(!snapshot_has_live_group("\n", 42).unwrap());
    }

    #[test]
    fn malformed_snapshots_and_unowned_groups_are_errors() {
        for snapshot in ["not-a-group S", "42"] {
            assert!(snapshot_has_live_group(snapshot, 42).is_err());
        }
        for group in [-1, 0, 1] {
            assert!(snapshot_has_live_group("", group).is_err());
        }
    }
}

use anyhow::{Context, Result, ensure};

/// Observe only a controlled fixture PID. Zombies have released their pipes;
/// the start timestamp distinguishes ordinary PID reuse during the test.
pub struct Process {
    pid: u32,
    started: String,
}

fn snapshot(pid: u32) -> Result<Option<(String, bool)>> {
    let output = std::process::Command::new("/bin/ps")
        .args(["-p", &pid.to_string(), "-o", "lstart=,state="])
        .output()?;
    if output.status.code() == Some(1) && output.stdout.is_empty() {
        return Ok(None);
    }
    ensure!(output.status.success(), "Cannot inspect fixture process");
    let text = std::str::from_utf8(&output.stdout)?;
    let mut fields: Vec<_> = text.split_whitespace().collect();
    let state = fields.pop().context("Missing fixture process state")?;
    ensure!(!fields.is_empty(), "Missing fixture process start time");
    Ok(Some((fields.join(" "), !state.starts_with(['Z', 'X']))))
}

impl Process {
    pub fn observe(pid: u32) -> Result<Self> {
        let (started, running) = snapshot(pid)?.context("Fixture process already exited")?;
        ensure!(running, "Fixture process was not alive for observation");
        Ok(Self { pid, started })
    }

    pub fn running(&self) -> Result<bool> {
        Ok(snapshot(self.pid)?
            .is_some_and(|(started, running)| started == self.started && running))
    }
}

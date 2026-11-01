use super::*;
use std::io;
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

const MAX_PROCESS_BYTES: usize = 4 * 1024 * 1024;
const DISALLOWED_TOOLS: &str = "Task Bash Glob Grep LS exit_plan_mode Read Edit MultiEdit Write NotebookRead NotebookEdit TodoRead TodoWrite";

// Python shlex.split(comments=False, posix=True). Unlike the Rust shlex crate,
// '#' is literal, CR is whitespace, backslash-newline is not removed, and
// backslashes in double quotes only escape backslash and double quote.
fn split_command(command: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quote = None;
    let mut chars = command.chars();
    while let Some(ch) = chars.next() {
        match (quote, ch) {
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (Some('\''), ch) => word.push(ch),
            (Some('"'), '\\') => {
                let escaped = chars.next()?;
                if escaped != '\\' && escaped != '"' {
                    word.push('\\');
                }
                word.push(escaped);
            }
            (Some('"'), ch) => word.push(ch),
            (None, '\'' | '"') => {
                quote = Some(ch);
                started = true;
            }
            (None, '\\') => {
                word.push(chars.next()?);
                started = true;
            }
            (None, ' ' | '\t' | '\r' | '\n') => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            (None, ch) => {
                word.push(ch);
                started = true;
            }
            _ => unreachable!("only single and double quote states"),
        }
    }
    if quote.is_some() {
        return None;
    }
    if started {
        words.push(word);
    }
    Some(words)
}

fn parse_command(ctx: &ProviderContext<'_>, key: &str) -> ProviderResult<Vec<String>> {
    split_command(option_string(ctx, key).unwrap_or(""))
        .filter(|parts| !parts.is_empty())
        .ok_or_else(|| {
            ctx.error(
                ErrorKind::InvalidConfiguration,
                "CLI command is empty or has invalid quoting",
            )
        })
}

fn is_executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|meta| meta.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    true
}

fn executable_exists(command: &str) -> bool {
    let command_path = Path::new(command);
    if command_path.is_absolute() || command_path.components().count() > 1 {
        return is_executable_file(command_path);
    }
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    #[cfg(windows)]
    let extensions: Vec<String> = std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
        .split(';')
        .filter(|ext| !ext.is_empty())
        .map(str::to_owned)
        .collect();
    std::env::split_paths(&paths).any(|dir| {
        if is_executable_file(&dir.join(command)) {
            return true;
        }
        #[cfg(windows)]
        if extensions
            .iter()
            .any(|ext| is_executable_file(&dir.join(format!("{command}{ext}"))))
        {
            return true;
        }
        false
    })
}

fn validate_parts(ctx: &ProviderContext<'_>, parts: &[String]) -> ProviderResult<()> {
    if !executable_exists(&parts[0]) {
        return Err(ctx.error(
            ErrorKind::InvalidConfiguration,
            "Configured CLI executable was not found",
        ));
    }
    Ok(())
}

pub(super) fn validate_cli(ctx: &ProviderContext<'_>) -> ProviderResult<()> {
    validate_parts(ctx, &parse_command(ctx, "clitranslator_command")?)?;
    if option_string(ctx, "clitranslator_postprocess_command")
        .is_some_and(|value| !value.is_empty())
    {
        validate_parts(
            ctx,
            &parse_command(ctx, "clitranslator_postprocess_command")?,
        )?;
    }
    Ok(())
}

fn process_policy(layer: &'static str) -> RetryPolicy {
    RetryPolicy {
        layer,
        max_attempts: 3,
        selector: RetrySelector::Process,
        wait: WaitPolicy::P,
        wrap_exhaustion: true,
    }
}

async fn read_bounded(mut stream: impl AsyncRead + Unpin) -> io::Result<Vec<u8>> {
    let mut output = Vec::new();
    (&mut stream)
        .take(MAX_PROCESS_BYTES as u64 + 1)
        .read_to_end(&mut output)
        .await?;
    if output.len() > MAX_PROCESS_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "CLI output exceeded size limit",
        ));
    }
    Ok(output)
}

async fn kill_and_reap(
    ctx: &ProviderContext<'_>,
    child: &mut tokio::process::Child,
    scope: crate::process::ProcessScope,
) -> ProviderResult<()> {
    // Wait for the entire job/process group, including descendants owning a pipe.
    // Cleanup is independent of the already-cancelled request token.
    let tree_result = scope.terminate_and_wait().await;
    let _ = child.start_kill();
    let child_result = child.wait().await;
    if tree_result.is_err() || child_result.is_err() {
        return Err(ctx.error(ErrorKind::Runtime, "Configured CLI process cleanup failed"));
    }
    Ok(())
}

struct CapturedOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
}

async fn run_process(
    ctx: &ProviderContext<'_>,
    args: &[String],
    input: &[u8],
    timeout: Duration,
    claude: bool,
) -> ProviderResult<CapturedOutput> {
    if ctx.cancel.is_cancelled() {
        return Err(ctx.error(ErrorKind::Cancelled, "translation cancelled"));
    }
    let mut command = Command::new(&args[0]);
    command
        .args(&args[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if claude {
        command.env_remove("ANTHROPIC_API_KEY");
    }
    crate::process::prepare(&mut command);
    let mut child = command
        .spawn()
        .map_err(|_| ctx.error(ErrorKind::Value, "Unable to start configured CLI process"))?;
    let scope = match crate::process::contain_and_resume(&mut child) {
        Ok(scope) => scope,
        Err(_) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Err(ctx.error(ErrorKind::Value, "Unable to contain configured CLI process"));
        }
    };
    let mut stdin = child.stdin.take().expect("piped child stdin");
    let stdout = child.stdout.take().expect("piped child stdout");
    let stderr = child.stderr.take().expect("piped child stderr");
    // Popen(text=True, newline=None) translates LF while writing on Windows.
    // Existing CR characters remain literal, as in TextIOWrapper.write.
    #[cfg(windows)]
    let translated_input = String::from_utf8_lossy(input)
        .replace('\n', "\r\n")
        .into_bytes();
    #[cfg(windows)]
    let input = translated_input.as_slice();
    let communicate = async {
        let write = async {
            if let Err(error) = stdin.write_all(input).await
                && error.kind() != io::ErrorKind::BrokenPipe
            {
                return Err(error);
            }
            drop(stdin);
            Ok(())
        };
        let (_, stdout, _stderr, status) = tokio::try_join!(
            write,
            read_bounded(stdout),
            read_bounded(stderr),
            child.wait()
        )?;
        Ok::<_, io::Error>(CapturedOutput { status, stdout })
    };
    enum Completion<T> {
        Done(T),
        Cancelled,
        TimedOut,
    }
    let completion = {
        tokio::pin!(communicate);
        tokio::select! {
            biased;
            _ = ctx.cancel.cancelled() => Completion::Cancelled,
            _ = tokio::time::sleep(timeout) => Completion::TimedOut,
            result = &mut communicate => Completion::Done(result),
        }
    };
    match completion {
        Completion::Cancelled => {
            kill_and_reap(ctx, &mut child, scope).await?;
            Err(ctx.error(ErrorKind::Cancelled, "translation cancelled"))
        }
        Completion::TimedOut => {
            kill_and_reap(ctx, &mut child, scope).await?;
            Err(ctx.error(
                if claude {
                    ErrorKind::Value
                } else {
                    ErrorKind::ProcessTimeout
                },
                if claude {
                    "Claude Code translation timed out"
                } else {
                    "CLI translation timed out"
                },
            ))
        }
        Completion::Done(Err(_)) => {
            kill_and_reap(ctx, &mut child, scope).await?;
            Err(ctx.error(ErrorKind::Value, "Unable to read configured CLI output"))
        }
        Completion::Done(Ok(output)) => {
            kill_and_reap(ctx, &mut child, scope).await?;
            Ok(output)
        }
    }
}

fn check_process(ctx: &ProviderContext<'_>, result: CapturedOutput) -> ProviderResult<String> {
    if !result.status.success() {
        return Err(ctx.error(
            ErrorKind::ProcessExit,
            format!(
                "Configured CLI process exited unsuccessfully (exit {})",
                result
                    .status
                    .code()
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "signal".into()),
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&result.stdout)
        .replace("\r\n", "\n")
        .replace('\r', "\n"))
}

pub(super) async fn translate(ctx: &ProviderContext<'_>) -> ProviderResult<ProviderOutput> {
    let command = parse_command(ctx, "clitranslator_command")?;
    let postprocess = match option_string(ctx, "clitranslator_postprocess_command") {
        Some(value) if !value.is_empty() => {
            Some(parse_command(ctx, "clitranslator_postprocess_command")?)
        }
        _ => None,
    };
    let timeout_seconds = ctx
        .option("clitranslator_timeout")
        .and_then(Value::as_f64)
        .unwrap_or(60.0);
    let timeout = Duration::try_from_secs_f64(timeout_seconds)
        .ok()
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| {
            ctx.error(
                ErrorKind::InvalidConfiguration,
                "CLI timeout must be finite and positive",
            )
        })?;
    ctx.retry(process_policy("clitranslator.application"), || async {
        let result =
            run_process(ctx, &command, ctx.request.text.as_bytes(), timeout, false).await?;
        let mut text = check_process(ctx, result)?;
        if let Some(postprocess) = &postprocess {
            let result = run_process(ctx, postprocess, text.as_bytes(), timeout, false).await?;
            text = check_process(ctx, result)?;
        }
        Ok(output(text.trim()))
    })
    .await
}

fn claude_path(ctx: &ProviderContext<'_>) -> String {
    option_string(ctx, "claude_code_path")
        .unwrap_or("claude")
        .to_owned()
}

pub(super) async fn preflight_claude(ctx: &ProviderContext<'_>) -> ProviderResult<()> {
    let command = vec![claude_path(ctx), "--version".into()];
    let result = run_process(ctx, &command, b"", Duration::from_secs(5), true).await?;
    if !result.status.success() {
        return Err(ctx.error(ErrorKind::Value, "Claude Code CLI startup check failed"));
    }
    Ok(())
}

fn parse_claude_output(ctx: &ProviderContext<'_>, output: &str) -> ProviderResult<String> {
    let mut text = String::new();
    for line in output.trim().split('\n') {
        if !line.trim().starts_with('{') {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match value.get("type").and_then(Value::as_str) {
            Some("assistant") => {
                if let Some(contents) = value.pointer("/message/content").and_then(Value::as_array)
                {
                    for content in contents {
                        if content.get("type").and_then(Value::as_str) == Some("text")
                            && let Some(value) = content.get("text")
                        {
                            let value = value.as_str().ok_or_else(|| {
                                ctx.error(
                                    ErrorKind::Value,
                                    "Claude Code returned invalid text content",
                                )
                            })?;
                            text.push_str(value);
                        }
                    }
                }
            }
            Some("text") => {
                if let Some(value) = value.get("text") {
                    let value = value.as_str().ok_or_else(|| {
                        ctx.error(
                            ErrorKind::Value,
                            "Claude Code returned invalid text content",
                        )
                    })?;
                    text.push_str(value);
                }
            }
            _ => {}
        }
    }
    if text.trim().is_empty() {
        return Err(ctx.error(ErrorKind::Value, "No translation received from Claude Code"));
    }
    Ok(text.trim().to_owned())
}

pub(super) async fn claude(ctx: &ProviderContext<'_>) -> ProviderResult<ProviderOutput> {
    let model = option_string(ctx, "claude_code_model").unwrap_or_else(|| {
        if ctx.config.model.is_empty() {
            "sonnet"
        } else {
            &ctx.config.model
        }
    });
    let command = vec![
        claude_path(ctx),
        "-p".into(),
        "--model".into(),
        model.into(),
        "--max-turns".into(),
        "1".into(),
        "--input-format".into(),
        "stream-json".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
        "--disallowedTools".into(),
        DISALLOWED_TOOLS.into(),
    ];
    let input = serde_json::to_vec(&serde_json::json!({
        "type": "user",
        "message": {"role": "user", "content": ctx.prompt(&ctx.request.text)},
    }))
    .expect("serializable Claude input");
    ctx.retry(process_policy("claudecode.application"), || async {
        let result = run_process(ctx, &command, &input, Duration::from_secs(120), true).await?;
        let text = check_process(ctx, result)?;
        Ok(output(parse_claude_output(ctx, &text)?))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn executable_validation_requires_a_regular_file_with_execute_permission() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("fixture");
        std::fs::write(&path, b"fixture").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(!is_executable_file(&path));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(is_executable_file(&path));
        assert!(!is_executable_file(directory.path()));
    }

    #[test]
    fn command_parser_matches_python_shlex_without_shell_expansion() {
        assert_eq!(
            split_command("tool '' \"a b\" '# literal' #tag"),
            Some(vec![
                "tool".into(),
                "".into(),
                "a b".into(),
                "# literal".into(),
                "#tag".into(),
            ])
        );
        assert_eq!(
            split_command("tool \"\\$x\\`y\\\"z\\\\\" a\\\nb\rc"),
            Some(vec![
                "tool".into(),
                "\\$x\\`y\"z\\".into(),
                "a\nb".into(),
                "c".into(),
            ])
        );
        assert_eq!(split_command("tool 'unterminated"), None);
        assert_eq!(split_command("tool \\"), None);
    }
}

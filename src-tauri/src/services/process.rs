use crate::errors::{AppError, Result};
use crate::models::TranscriptSegment;
use std::{ffi::OsString, path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader},
    process::Command,
    sync::mpsc,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct Binaries {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub whisper: PathBuf,
}
impl Binaries {
    pub fn packaged() -> Result<Self> {
        let base = std::env::current_exe()?
            .parent()
            .ok_or_else(|| {
                AppError::new("binary", "O diretório do aplicativo não está disponível.")
            })?
            .to_path_buf();
        let resolve = |name: &str| {
            let suffix = if cfg!(windows) { ".exe" } else { "" };
            let installed = base.join(format!("{name}{suffix}"));
            if installed.is_file() {
                return installed;
            }
            if cfg!(debug_assertions) {
                return PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("binaries")
                    .join(format!("{name}-{}{suffix}", env!("LOCALSCRIBE_TARGET")));
            }
            installed
        };
        Ok(Self {
            // Keep application sidecars out of /usr/bin names owned by packages
            // such as Ubuntu's `ffmpeg` and `ffprobe`.
            ffmpeg: resolve("localscribe-ffmpeg"),
            ffprobe: resolve("localscribe-ffprobe"),
            whisper: super::cpu::select(
                resolve("localscribe-whisper-cli"),
                resolve("localscribe-whisper-avx2"),
                super::cpu::capabilities(),
                super::cpu::automatic()?,
            ),
        })
    }
}
pub struct Output {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub success: bool,
}
async fn drain(mut reader: impl AsyncRead + Unpin) -> std::io::Result<Vec<u8>> {
    let mut data = Vec::new();
    let mut chunk = [0; 8192];
    loop {
        let n = reader.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        let keep = n.min(1_048_576_usize.saturating_sub(data.len()));
        data.extend_from_slice(&chunk[..keep]);
    }
    Ok(data)
}
pub async fn run(
    executable: &PathBuf,
    args: &[OsString],
    cancel: &CancellationToken,
    timeout: Duration,
) -> Result<Output> {
    run_inner(executable, args, cancel, timeout, None).await
}

pub async fn run_with_progress(
    executable: &PathBuf,
    args: &[OsString],
    cancel: &CancellationToken,
    timeout: Duration,
    progress: mpsc::UnboundedSender<u64>,
) -> Result<Output> {
    run_inner(executable, args, cancel, timeout, Some(progress)).await
}

/// Whisper CLI's `--print-progress` writes percentages from its native
/// inference callback to stderr. Parse only that stable, labeled field.
pub async fn run_with_stderr_progress(
    executable: &PathBuf,
    args: &[OsString],
    cancel: &CancellationToken,
    timeout: Duration,
    progress: mpsc::UnboundedSender<u64>,
) -> Result<Output> {
    run_inner_impl(
        executable,
        args,
        cancel,
        timeout,
        Some(progress),
        true,
        None,
    )
    .await
}

pub async fn run_whisper_stream(
    executable: &PathBuf,
    args: &[OsString],
    cancel: &CancellationToken,
    timeout: Duration,
    job_id: &str,
    progress: mpsc::UnboundedSender<u64>,
    segments: mpsc::UnboundedSender<TranscriptSegment>,
) -> Result<Output> {
    run_inner_impl(
        executable,
        args,
        cancel,
        timeout,
        Some(progress),
        true,
        Some((job_id.to_owned(), segments)),
    )
    .await
}

async fn run_inner(
    executable: &PathBuf,
    args: &[OsString],
    cancel: &CancellationToken,
    timeout: Duration,
    progress: Option<mpsc::UnboundedSender<u64>>,
) -> Result<Output> {
    run_inner_impl(executable, args, cancel, timeout, progress, false, None).await
}

async fn run_inner_impl(
    executable: &PathBuf,
    args: &[OsString],
    cancel: &CancellationToken,
    timeout: Duration,
    progress: Option<mpsc::UnboundedSender<u64>>,
    stderr_progress: bool,
    live_segments: Option<(String, mpsc::UnboundedSender<TranscriptSegment>)>,
) -> Result<Output> {
    if cancel.is_cancelled() {
        return Err(AppError::cancelled());
    }
    if !executable.is_file() {
        return Err(AppError::new("missing_binary", "Um componente de processamento está ausente. Reinstale o LocalScribe ou execute o setup dos sidecars."));
    }
    let mut child = Command::new(executable)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let stdout = child.stdout.take().unwrap();
    let stdout_progress = if stderr_progress {
        None
    } else {
        progress.clone()
    };
    let stdout_segments = live_segments.clone();
    let out = tokio::spawn(async move {
        if let Some((job_id, segments)) = stdout_segments {
            drain_whisper_segments(stdout, &job_id, segments).await
        } else if let Some(progress) = stdout_progress {
            drain_with_progress(stdout, progress).await
        } else {
            drain(stdout).await
        }
    });
    let stderr = child.stderr.take().unwrap();
    let err = tokio::spawn(async move {
        if stderr_progress {
            drain_whisper_progress(stderr, progress.unwrap()).await
        } else {
            drain(stderr).await
        }
    });
    let status = tokio::select! {
        result = child.wait() => result.map_err(AppError::from),
        _ = cancel.cancelled() => { let _ = child.kill().await; let _ = child.wait().await; Err(AppError::cancelled()) },
        _ = tokio::time::sleep(timeout) => { let _ = child.kill().await; let _ = child.wait().await; Err(AppError::new("timeout", "O processador excedeu o tempo limite. Tente um arquivo menor.")) },
    };
    let stdout = out
        .await
        .map_err(|_| AppError::new("process", "Falha ao ler a saída do processador."))??;
    let stderr = err
        .await
        .map_err(|_| AppError::new("process", "Falha ao ler o diagnóstico do processador."))??;
    Ok(Output {
        stdout,
        stderr,
        success: status?.success(),
    })
}

async fn drain_whisper_segments(
    reader: impl AsyncRead + Unpin,
    job_id: &str,
    segments: mpsc::UnboundedSender<TranscriptSegment>,
) -> std::io::Result<Vec<u8>> {
    let mut reader = BufReader::new(reader);
    let mut data = Vec::new();
    let mut line = String::new();
    let mut index = 0usize;
    loop {
        line.clear();
        if reader.read_line(&mut line).await? == 0 {
            break;
        }
        if let Some((start_ms, end_ms, text)) = parse_whisper_segment(&line) {
            let _ = segments.send(TranscriptSegment {
                id: format!("{job_id}-{index}"),
                start_ms,
                end_ms,
                text,
            });
            index += 1;
        }
        let bytes = line.as_bytes();
        let keep = bytes.len().min(1_048_576_usize.saturating_sub(data.len()));
        data.extend_from_slice(&bytes[..keep]);
    }
    Ok(data)
}

fn parse_whisper_segment(line: &str) -> Option<(u64, u64, String)> {
    let line = line.trim_start();
    let line = line.strip_prefix('[')?;
    let (start, rest) = line.split_once(" --> ")?;
    let (end, text) = rest.split_once(']')?;
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    Some((
        parse_whisper_timestamp(start)?,
        parse_whisper_timestamp(end)?,
        text.to_owned(),
    ))
}

fn parse_whisper_timestamp(timestamp: &str) -> Option<u64> {
    let (hours, remainder) = timestamp.split_once(':')?;
    let (minutes, remainder) = remainder.split_once(':')?;
    let (seconds, millis) = remainder.split_once('.')?;
    let hours = hours.parse::<u64>().ok()?;
    let minutes = minutes.parse::<u64>().ok()?;
    let seconds = seconds.parse::<u64>().ok()?;
    let millis = millis.parse::<u64>().ok()?;
    if minutes >= 60 || seconds >= 60 || millis >= 1000 {
        return None;
    }
    Some(((hours * 60 + minutes) * 60 + seconds) * 1000 + millis)
}

async fn drain_whisper_progress(
    reader: impl AsyncRead + Unpin,
    progress: mpsc::UnboundedSender<u64>,
) -> std::io::Result<Vec<u8>> {
    let mut reader = BufReader::new(reader);
    let mut data = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).await? == 0 {
            break;
        }
        if let Some(value) = parse_whisper_progress(&line) {
            let _ = progress.send(value.min(100));
        }
        let bytes = line.as_bytes();
        let keep = bytes.len().min(1_048_576_usize.saturating_sub(data.len()));
        data.extend_from_slice(&bytes[..keep]);
    }
    Ok(data)
}

fn parse_whisper_progress(line: &str) -> Option<u64> {
    line.split("progress =")
        .nth(1)?
        .trim()
        .strip_suffix('%')?
        .trim()
        .parse()
        .ok()
}

#[cfg(test)]
mod whisper_progress_tests {
    use super::{parse_whisper_progress, parse_whisper_segment};

    #[test]
    fn parses_only_whisper_cli_progress_lines() {
        assert_eq!(
            parse_whisper_progress("whisper_print_progress_callback: progress =   5%\n"),
            Some(5)
        );
        assert_eq!(parse_whisper_progress("progress = 105%"), Some(105));
        assert_eq!(parse_whisper_progress("progress = nope%"), None);
        assert_eq!(parse_whisper_progress("loading model"), None);
    }

    #[test]
    fn parses_completed_cli_segments_with_timestamps() {
        assert_eq!(
            parse_whisper_segment("[00:00:01.250 --> 00:00:03.500]  Olá, mundo!\n"),
            Some((1250, 3500, "Olá, mundo!".into()))
        );
        assert_eq!(parse_whisper_segment("loading model\n"), None);
        assert_eq!(
            parse_whisper_segment("[00:00:61.250 --> 00:00:63.500] invalid\n"),
            None
        );
    }
}

async fn drain_with_progress(
    reader: impl AsyncRead + Unpin,
    progress: mpsc::UnboundedSender<u64>,
) -> std::io::Result<Vec<u8>> {
    let mut reader = BufReader::new(reader);
    let mut data = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).await? == 0 {
            break;
        }
        if let Some(value) = line
            .strip_prefix("out_time_us=")
            .and_then(|value| value.trim().parse::<u64>().ok())
        {
            let _ = progress.send(value);
        }
        let bytes = line.as_bytes();
        let keep = bytes.len().min(1_048_576_usize.saturating_sub(data.len()));
        data.extend_from_slice(&bytes[..keep]);
    }
    Ok(data)
}
pub fn process_error(code: &str, fallback: &str, stderr: &[u8]) -> AppError {
    let text = String::from_utf8_lossy(stderr);
    if text.contains("No space left on device") {
        AppError::new(
            "disk_full",
            "Espaço em disco insuficiente. Libere espaço e tente novamente.",
        )
    } else if text.contains("Permission denied") {
        AppError::new(
            "permission",
            "Sem permissão para acessar a mídia ou a pasta de trabalho.",
        )
    } else {
        AppError::new(code, fallback)
    }
}

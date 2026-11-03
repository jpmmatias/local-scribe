use crate::{
    errors::{AppError, Result},
    models::Media,
    services::process::{self, Binaries},
};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub const EXTENSIONS: &[&str] = &["mp4", "mov", "mkv", "webm", "mp3", "wav", "m4a"];
pub async fn inspect(bins: &Binaries, path: &Path, cancel: &CancellationToken) -> Result<Media> {
    let path = tokio::fs::canonicalize(path).await?;
    let meta = tokio::fs::metadata(&path).await?;
    if !meta.is_file()
        || !EXTENSIONS.contains(
            &path
                .extension()
                .and_then(|x| x.to_str())
                .unwrap_or("")
                .to_lowercase()
                .as_str(),
        )
    {
        return Err(AppError::new(
            "format",
            "Formato não suportado. Escolha MP4, MOV, MKV, WEBM, MP3, WAV ou M4A.",
        ));
    }
    let mut args: Vec<OsString> = [
        "-v",
        "error",
        "-protocol_whitelist",
        "file,pipe",
        "-show_entries",
        "format=duration:stream=codec_type",
        "-of",
        "json",
    ]
    .iter()
    .map(Into::into)
    .collect();
    args.push(path.as_os_str().into());
    let out = process::run(&bins.ffprobe, &args, cancel, Duration::from_secs(30)).await?;
    if !out.success {
        return Err(process::process_error(
            "corrupt_media",
            "Não foi possível abrir a mídia. O arquivo pode estar corrompido.",
            &out.stderr,
        ));
    }
    let data: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    let streams = data["streams"]
        .as_array()
        .ok_or_else(|| AppError::new("corrupt_media", "A mídia não contém streams válidos."))?;
    if !streams.iter().any(|s| s["codec_type"] == "audio") {
        return Err(AppError::new(
            "no_audio",
            "Este arquivo não contém uma faixa de áudio.",
        ));
    }
    let duration = data["format"]["duration"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);
    if !duration.is_finite() || duration <= 0.0 {
        return Err(AppError::new(
            "duration",
            "Não foi possível determinar a duração da mídia.",
        ));
    }
    Ok(Media {
        name: path.file_name().unwrap().to_string_lossy().into(),
        path: path.to_string_lossy().into(),
        size: meta.len(),
        duration_ms: (duration * 1000.0) as u64,
        video: streams.iter().any(|s| s["codec_type"] == "video"),
    })
}
pub async fn extract(
    bins: &Binaries,
    media: &Media,
    output: &Path,
    cancel: &CancellationToken,
    progress: mpsc::UnboundedSender<u64>,
) -> Result<()> {
    let needed = media
        .duration_ms
        .saturating_mul(32)
        .saturating_add(16_000_000);
    if fs2::available_space(output.parent().unwrap())? < needed {
        return Err(AppError::new(
            "disk_full",
            "Espaço insuficiente para preparar o áudio. Libere espaço em disco.",
        ));
    }
    let args = vec![
        "-nostdin".into(),
        "-y".into(),
        "-v".into(),
        "error".into(),
        "-progress".into(),
        "pipe:1".into(),
        "-stats_period".into(),
        "0.25".into(),
        "-protocol_whitelist".into(),
        "file,pipe".into(),
        "-i".into(),
        OsString::from(&media.path),
        "-map".into(),
        "0:a:0".into(),
        "-vn".into(),
        "-ac".into(),
        "1".into(),
        "-ar".into(),
        "16000".into(),
        "-c:a".into(),
        "pcm_s16le".into(),
        "-f".into(),
        "wav".into(),
        output.as_os_str().into(),
    ];
    let out = process::run_with_progress(
        &bins.ffmpeg,
        &args,
        cancel,
        Duration::from_secs(21600),
        progress,
    )
    .await?;
    if !out.success {
        return Err(process::process_error(
            "ffmpeg",
            "Não foi possível extrair o áudio. Verifique se a mídia pode ser reproduzida.",
            &out.stderr,
        ));
    }
    validate_wav(output.to_path_buf()).await
}

pub fn progress_percent(out_time_us: u64, duration_ms: u64) -> Option<f32> {
    if duration_ms == 0 {
        return None;
    }
    let duration_us = u128::from(duration_ms) * 1000;
    let percent = (u128::from(out_time_us) * 100 / duration_us).min(99) as f32;
    Some(percent)
}

#[cfg(test)]
mod progress_tests {
    use super::progress_percent;

    #[test]
    fn computes_progress_from_ffmpeg_output_time() {
        assert_eq!(progress_percent(5_000_000, 10_000), Some(50.0));
        assert_eq!(progress_percent(0, 10_000), Some(0.0));
    }

    #[test]
    fn never_reports_completion_before_the_process_finishes() {
        assert_eq!(progress_percent(10_000_000, 10_000), Some(99.0));
        assert_eq!(progress_percent(1, 0), None);
    }
}
pub async fn validate_wav(path: PathBuf) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let reader = hound::WavReader::open(path)
            .map_err(|_| AppError::new("wav", "O áudio convertido está inválido."))?;
        let spec = reader.spec();
        if spec.channels != 1
            || spec.sample_rate != 16000
            || spec.bits_per_sample != 16
            || spec.sample_format != hound::SampleFormat::Int
            || reader.duration() == 0
        {
            return Err(AppError::new(
                "wav",
                "A conversão não produziu áudio PCM mono de 16 kHz.",
            ));
        }
        Ok(())
    })
    .await
    .map_err(|_| AppError::new("worker", "Falha ao validar o áudio."))?
}

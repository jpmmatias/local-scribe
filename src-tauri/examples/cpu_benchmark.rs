//! One measured pipeline run. The JS driver handles warm-up, ordering and medians.
use localscribe_lib::{
    models::*,
    services::{
        cpu, ffmpeg,
        model_manager::{verify_file, Catalog},
        process::Binaries,
        whisper,
    },
};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, time::Instant};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}
fn wer(reference: &str, hypothesis: &str) -> f64 {
    let reference = words(reference);
    let hypothesis = words(hypothesis);
    let mut row: Vec<usize> = (0..=hypothesis.len()).collect();
    for (i, expected) in reference.iter().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, actual) in hypothesis.iter().enumerate() {
            let old = row[j + 1];
            row[j + 1] = (diagonal + usize::from(expected != actual))
                .min(row[j] + 1)
                .min(old + 1);
            diagonal = old;
        }
    }
    row[hypothesis.len()] as f64 / reference.len() as f64
}
#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        // Never expose source paths, subprocess output or transcript text.
        eprintln!("benchmark failed: {}", error.code);
        std::process::exit(1);
    }
}
async fn run() -> localscribe_lib::errors::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|arg| arg == "--capabilities") {
        println!(
            "{}",
            serde_json::json!({"available_threads": cpu::available_threads(), "avx2": cpu::capabilities().iter().all(|v| *v)})
        );
        return Ok(());
    }
    if args.len() != 5 {
        return Err(localscribe_lib::errors::AppError::new(
            "arguments",
            "Expected media, reference, model and model ID",
        ));
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries");
    let binary = |name| root.join(format!("localscribe-{name}-x86_64-unknown-linux-gnu"));
    let bins = Binaries {
        ffmpeg: binary("ffmpeg"),
        ffprobe: binary("ffprobe"),
        whisper: cpu::select(
            binary("whisper-cli"),
            binary("whisper-avx2"),
            cpu::capabilities(),
            cpu::automatic()?,
        ),
    };
    let threads = cpu::thread_count()?;
    let reference = std::fs::read_to_string(&args[2])?;
    if words(&reference).is_empty() {
        return Err(localscribe_lib::errors::AppError::new(
            "reference",
            "Empty reference",
        ));
    }
    let catalog: Catalog = serde_json::from_str(include_str!("../model-catalog.json"))?;
    let spec = catalog
        .models
        .iter()
        .find(|m| m.id == args[4])
        .ok_or_else(|| localscribe_lib::errors::AppError::new("model", "Unknown model"))?;
    let token = CancellationToken::new();
    let temp = tempfile::tempdir()?;
    let total = Instant::now();
    let start = Instant::now();
    verify_file(std::path::Path::new(&args[3]), spec, &token).await?;
    let verification_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    let media = ffmpeg::inspect(&bins, std::path::Path::new(&args[1]), &token).await?;
    let audio = temp.path().join("audio.wav");
    let (tx, _rx) = mpsc::unbounded_channel();
    ffmpeg::extract(&bins, &media, &audio, &token, tx).await?;
    let extraction_ms = start.elapsed().as_secs_f64() * 1000.0;
    let job = TranscriptionJob {
        id: "benchmark".into(),
        source_path: media.path,
        filename: media.name,
        model: spec.id.clone(),
        language: "pt".into(),
        status: Status::Transcribing,
        progress: None,
        created_at: String::new(),
        duration_ms: Some(media.duration_ms),
        error: None,
    };
    let (progress, _rx) = mpsc::unbounded_channel();
    let (segments, _rx) = mpsc::unbounded_channel();
    let start = Instant::now();
    let output = whisper::transcribe(
        &bins,
        &job,
        std::path::Path::new(&args[3]),
        &audio,
        &temp.path().join("result"),
        &token,
        progress,
        segments,
    )
    .await?;
    let transcript = output.transcript;
    let inference_ms = start.elapsed().as_secs_f64() * 1000.0;
    let total_ms = total.elapsed().as_secs_f64() * 1000.0;
    let timestamps_valid = !transcript.segments.is_empty()
        && transcript
            .segments
            .iter()
            .all(|s| s.start_ms <= s.end_ms && s.end_ms <= media.duration_ms)
        && transcript
            .segments
            .windows(2)
            .all(|s| s[0].start_ms <= s[1].start_ms && s[0].end_ms <= s[1].end_ms);
    println!(
        "{}",
        serde_json::json!({
            "verification_ms": verification_ms, "extraction_ms": extraction_ms, "inference_ms": inference_ms,
            "total_ms": total_ms, "audio_ms": media.duration_ms, "rtf": total_ms / media.duration_ms as f64,
            "inference_rtf": inference_ms / media.duration_ms as f64,
            "wer": wer(&reference, &transcript.full_text), "timestamps_valid": timestamps_valid,
            "model": spec.id, "model_sha256": spec.sha256, "threads": threads,
            "optimized": bins.whisper == binary("whisper-avx2"),
            "binary_sha256": format!("{:x}", Sha256::digest(std::fs::read(&bins.whisper)?)),
        })
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalized_word_error_rate() {
        assert_eq!(wer("Olá, AÇÃO!", "olá ação"), 0.0);
        assert_eq!(wer("um dois", "um três"), 0.5);
        assert_eq!(wer("um dois", "um"), 0.5);
        assert_eq!(wer("um", "um dois"), 1.0);
    }
}

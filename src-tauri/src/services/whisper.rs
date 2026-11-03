use crate::{
    errors::{AppError, Result},
    models::{Transcript, TranscriptSegment, TranscriptionJob},
    services::process::{self, Binaries},
};
use std::{path::Path, time::Duration};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

// The business layer consumes normalized transcripts. A future GPU adapter can
// implement this contract without changing commands, state, or the editor.
pub async fn transcribe(
    bins: &Binaries,
    job: &TranscriptionJob,
    model: &Path,
    audio: &Path,
    output_prefix: &Path,
    cancel: &CancellationToken,
    progress: mpsc::UnboundedSender<u64>,
    segments: mpsc::UnboundedSender<TranscriptSegment>,
) -> Result<(Transcript, String)> {
    let args = vec![
        "-m".into(),
        model.as_os_str().into(),
        "-f".into(),
        audio.as_os_str().into(),
        "-l".into(),
        job.language.clone().into(),
        "-oj".into(),
        "-of".into(),
        output_prefix.as_os_str().into(),
        "-ng".into(),
        "-t".into(),
        super::cpu::thread_count()?.to_string().into(),
        "-pp".into(),
    ];
    // No -tr / --translate: preserve the spoken language. CPU is explicit (-ng).
    let out = process::run_whisper_stream(
        &bins.whisper,
        &args,
        cancel,
        Duration::from_secs(86400),
        &job.id,
        progress,
        segments,
    )
    .await?;
    if !out.success {
        return Err(process::process_error("inference", "A transcrição falhou. Tente um modelo menor e feche outros aplicativos para liberar memória.", &out.stderr));
    }
    let raw = tokio::fs::read_to_string(output_prefix.with_extension("json")).await?;
    Ok((normalize(&raw, job)?, raw))
}
pub fn normalize(raw: &str, job: &TranscriptionJob) -> Result<Transcript> {
    let value: serde_json::Value = serde_json::from_str(raw)?;
    let rows = value["transcription"].as_array().ok_or_else(|| {
        AppError::new(
            "inference_output",
            "O processador retornou uma transcrição inválida.",
        )
    })?;
    let mut segments = vec![];
    let mut previous_start = 0;
    for (index, row) in rows.iter().enumerate() {
        let invalid = || {
            AppError::new(
                "timestamps",
                "O processador retornou marcações de tempo inválidas.",
            )
        };
        let start = row["offsets"]["from"].as_u64().ok_or_else(invalid)?;
        let end = row["offsets"]["to"].as_u64().ok_or_else(invalid)?;
        if end < start || start < previous_start {
            return Err(invalid());
        }
        let text = row["text"].as_str().ok_or_else(invalid)?.trim().to_string();
        previous_start = start;
        segments.push(TranscriptSegment {
            id: format!("{}-{index}", job.id),
            start_ms: start,
            end_ms: end,
            text,
        });
    }
    let language = value["result"]["language"]
        .as_str()
        .filter(|x| !x.is_empty())
        .unwrap_or(&job.language)
        .to_string();
    if language == "auto" {
        return Err(AppError::new(
            "language",
            "Não foi possível identificar o idioma da gravação.",
        ));
    }
    let mut transcript = Transcript {
        id: job.id.clone(),
        job_id: job.id.clone(),
        filename: job.filename.clone(),
        language,
        model: job.model.clone(),
        segments,
        full_text: String::new(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    transcript.rebuild();
    Ok(transcript)
}

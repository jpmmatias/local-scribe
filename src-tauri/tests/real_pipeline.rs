//! Opt-in integration tests: real FFmpeg and whisper.cpp, never a simulated engine.
use localscribe_lib::{
    models::*,
    services::{
        ffmpeg,
        model_manager::ModelManager,
        process::{self, Binaries},
        storage::Storage,
        transcription_manager::Manager,
    },
};
use std::{ffi::OsString, path::PathBuf, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

fn bins() -> Binaries {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries");
    Binaries {
        ffmpeg: root.join("localscribe-ffmpeg-x86_64-unknown-linux-gnu"),
        ffprobe: root.join("localscribe-ffprobe-x86_64-unknown-linux-gnu"),
        whisper: localscribe_lib::services::cpu::select(
            root.join("localscribe-whisper-cli-x86_64-unknown-linux-gnu"),
            root.join("localscribe-whisper-avx2-x86_64-unknown-linux-gnu"),
            localscribe_lib::services::cpu::capabilities(),
            localscribe_lib::services::cpu::automatic().unwrap(),
        ),
    }
}
#[tokio::test]
#[ignore = "Requires packaged sidecars, LOCALSCRIBE_TEST_MODEL, LOCALSCRIBE_TEST_MEDIA and LOCALSCRIBE_TEST_EXPECT"]
async fn real_transcription_edit_export_and_restart() {
    let media = PathBuf::from(
        std::env::var("LOCALSCRIBE_TEST_MEDIA").expect("Set the real audio/video fixture path"),
    );
    let model = PathBuf::from(
        std::env::var("LOCALSCRIBE_TEST_MODEL")
            .expect("Set the verified multilingual Base model path"),
    );
    let expected = std::env::var("LOCALSCRIBE_TEST_EXPECT")
        .expect("Set an independently known spoken phrase")
        .to_lowercase();
    let language = std::env::var("LOCALSCRIBE_TEST_LANGUAGE").unwrap_or("auto".into());
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("app");
    let source = temp.path().join(format!(
        "gravação 中文 ; $ teste.{}",
        media.extension().unwrap().to_string_lossy()
    ));
    tokio::fs::copy(&media, &source).await.unwrap();
    let before = tokio::fs::read(&source).await.unwrap();
    let (storage, db) = Storage::open(root.clone()).await.unwrap();
    tokio::fs::copy(model, root.join("models/ggml-base.bin"))
        .await
        .unwrap();
    let models = Arc::new(ModelManager::new(root.join("models"), Arc::new(|_| {})));
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let event_log = events.clone();
    let manager = Manager::new(
        storage,
        db,
        bins(),
        models,
        Arc::new(move |snapshot| event_log.lock().unwrap().push(snapshot)),
        Arc::new(|_| {}),
    )
    .await;
    manager
        .preferences(Preferences {
            model: "small".into(),
            language: "en".into(),
        })
        .await
        .unwrap();
    let info = ffmpeg::inspect(&bins(), &source, &CancellationToken::new())
        .await
        .unwrap();
    let retry_media = info.clone();
    let id = manager
        .enqueue(info.clone(), "base".into(), language.clone())
        .await
        .unwrap();
    assert_eq!(
        manager
            .enqueue(info, "base".into(), "auto".into())
            .await
            .unwrap_err()
            .code,
        "duplicate"
    );
    assert_eq!(manager.remove_model("base").await.unwrap_err().code, "busy");
    tokio::time::timeout(Duration::from_secs(600), async {
        loop {
            let data = manager.snapshot().await;
            let job = data.jobs.iter().find(|j| j.id == id).unwrap();
            if !job.status.active() {
                assert_eq!(job.status, Status::Completed, "{:?}", job.error);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        events.lock().unwrap().iter().any(|snapshot| {
            snapshot.jobs.iter().any(|job| {
                job.id == id && job.status == Status::Extracting && job.progress.is_some()
            })
        }),
        "FFmpeg extraction should emit real progress before inference"
    );
    assert!(
        events.lock().unwrap().iter().any(|snapshot| {
            snapshot.jobs.iter().any(|job| {
                job.id == id && job.status == Status::Transcribing && job.progress.is_none()
            })
        }),
        "Whisper inference should remain indeterminate"
    );
    assert!(events
        .lock()
        .unwrap()
        .windows(2)
        .all(|pair| pair[0].revision < pair[1].revision));
    let transcript = manager.transcript(&id).await.unwrap();
    assert!(
        transcript.full_text.to_lowercase().contains(&expected),
        "{}",
        transcript.full_text
    );
    assert_eq!(transcript.id, id);
    assert_eq!(transcript.job_id, id);
    assert_eq!(transcript.model, "base");
    assert!(!transcript.segments.is_empty());
    for (index, segment) in transcript.segments.iter().enumerate() {
        assert_eq!(segment.id, format!("{id}-{index}"));
    }
    assert!(transcript.segments.iter().all(|s| s.end_ms >= s.start_ms));
    let mut texts: Vec<String> = transcript.segments.iter().map(|s| s.text.clone()).collect();
    texts[0] = "Revisão com ação e Unicode 中文".into();
    let edited = manager.save_edits(&id, texts).await.unwrap();
    for format in ["txt", "srt", "vtt", "json"] {
        let output = localscribe_lib::services::exports::render(&edited, format).unwrap();
        assert!(String::from_utf8(output).unwrap().contains("Revisão"));
    }
    let json = localscribe_lib::services::exports::render(&edited, "json").unwrap();
    assert_eq!(serde_json::from_slice::<Transcript>(&json).unwrap(), edited);
    assert_eq!(
        localscribe_lib::services::exports::render(&edited, "txt").unwrap(),
        edited.full_text.as_bytes()
    );
    assert_eq!(edited.segments[0].id, transcript.segments[0].id);
    assert_eq!(edited.segments[0].start_ms, transcript.segments[0].start_ms);
    assert_eq!(edited.segments[0].end_ms, transcript.segments[0].end_ms);

    // Cancel through the public manager during inference, then retry successfully.
    let cancelled = manager
        .enqueue(retry_media.clone(), "base".into(), language.clone())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let snapshot = manager.snapshot().await;
            let job = snapshot
                .jobs
                .iter()
                .find(|job| job.id == cancelled)
                .unwrap();
            if job.status == Status::Transcribing {
                break;
            }
            assert!(
                job.status.active(),
                "job ended before cancellation: {:?}",
                job.status
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    manager.cancel(&cancelled).await.unwrap();
    assert_eq!(
        finished(&manager, &cancelled).await.status,
        Status::Cancelled
    );
    assert!(manager.transcript(&cancelled).await.is_err());
    assert!(!root.join("work").join(&cancelled).exists());

    // A real invalid-media failure is terminal, private, and does not poison the worker.
    let invalid = temp.path().join("private-recording.wav");
    tokio::fs::write(&invalid, b"private invalid media")
        .await
        .unwrap();
    let mut invalid_media = retry_media.clone();
    invalid_media.path = invalid.to_string_lossy().into_owned();
    let failed = manager
        .enqueue(invalid_media, "base".into(), language.clone())
        .await
        .unwrap();
    assert_eq!(finished(&manager, &failed).await.status, Status::Failed);
    assert!(manager.transcript(&failed).await.is_err());
    assert!(!root.join("work").join(&failed).exists());
    let diagnostics = tokio::fs::read_to_string(root.join("diagnostics.log"))
        .await
        .unwrap();
    assert!(!diagnostics.contains("private"));
    assert!(!diagnostics.contains(&source.to_string_lossy().to_string()));
    assert!(!diagnostics.contains(&transcript.full_text));
    let retried = manager
        .enqueue(retry_media, "base".into(), language)
        .await
        .unwrap();
    assert_eq!(finished(&manager, &retried).await.status, Status::Completed);
    assert_ne!(retried, id);
    assert!(manager
        .transcript(&retried)
        .await
        .unwrap()
        .full_text
        .to_lowercase()
        .contains(&expected));
    assert!(!root.join("work").join(&retried).exists());
    assert_eq!(tokio::fs::read(&source).await.unwrap(), before);
    assert!(!root.join("work").join(&id).exists());
    manager.shutdown().await;
    let (_, restored) = Storage::open(root).await.unwrap();
    assert_eq!(restored.preferences.model, "small");
    assert_eq!(restored.preferences.language, "en");
    assert_eq!(restored.version, 1);
    assert_eq!(restored.jobs[0].id, id);
    assert_eq!(restored.jobs[0].status, Status::Completed);
    assert_eq!(
        restored
            .jobs
            .iter()
            .find(|j| j.id == cancelled)
            .unwrap()
            .status,
        Status::Cancelled
    );
    assert_eq!(
        restored
            .jobs
            .iter()
            .find(|j| j.id == failed)
            .unwrap()
            .status,
        Status::Failed
    );
    assert_eq!(restored.records.len(), 2);
    assert_eq!(restored.records[0].edited, edited);
    assert_eq!(restored.records[0].original, transcript);
}
#[tokio::test]
#[ignore = "Requires packaged FFmpeg and LOCALSCRIBE_TEST_MEDIA"]
async fn cancellation_reaps_the_real_child() {
    let media = std::env::var("LOCALSCRIBE_TEST_MEDIA").unwrap();
    let args: Vec<OsString> = vec![
        "-nostdin".into(),
        "-re".into(),
        "-stream_loop".into(),
        "-1".into(),
        "-i".into(),
        media.into(),
        "-ac".into(),
        "1".into(),
        "-ar".into(),
        "16000".into(),
        "-c:a".into(),
        "pcm_s16le".into(),
        "-f".into(),
        "wav".into(),
        "pipe:1".into(),
    ];
    let token = CancellationToken::new();
    let cancel = token.clone();
    let binary = bins().ffmpeg;
    let process =
        tokio::spawn(
            async move { process::run(&binary, &args, &token, Duration::from_secs(60)).await },
        );
    tokio::time::sleep(Duration::from_millis(250)).await;
    cancel.cancel();
    let result = tokio::time::timeout(Duration::from_secs(5), process)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.err().unwrap().code, "cancelled");
}

async fn finished(manager: &Manager, id: &str) -> TranscriptionJob {
    tokio::time::timeout(Duration::from_secs(600), async {
        loop {
            let snapshot = manager.snapshot().await;
            let job = snapshot.jobs.iter().find(|job| job.id == id).unwrap();
            if !job.status.active() {
                return job.clone();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("job must reach a terminal state")
}

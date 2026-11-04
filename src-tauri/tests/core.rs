use localscribe_lib::{
    models::*,
    services::{
        exports,
        model_manager::{verify_file, ModelSpec},
        storage::Storage,
        whisper::normalize,
    },
};
use tokio_util::sync::CancellationToken;

fn job() -> TranscriptionJob {
    TranscriptionJob {
        id: "job".into(),
        source_path: "/source.mp4".into(),
        filename: "source.mp4".into(),
        model: "base".into(),
        language: "auto".into(),
        status: Status::Transcribing,
        progress: None,
        created_at: "2026-01-01T00:00:00Z".into(),
        duration_ms: Some(5000),
        error: None,
    }
}
#[test]
fn normalizes_offsets_without_translating() {
    let raw = r#"{"result":{"language":"pt"},"transcription":[{"offsets":{"from":1200,"to":2500},"text":" Olá, ação! "},{"offsets":{"from":3000,"to":4200},"text":"Tudo bem?"}]}"#;
    let result = normalize(raw, &job()).unwrap();
    assert_eq!(result.language, "pt");
    assert_eq!(result.segments[0].start_ms, 1200);
    assert_eq!(result.full_text, "Olá, ação!\nTudo bem?");
    let srt = String::from_utf8(exports::render(&result, "srt").unwrap()).unwrap();
    assert!(srt.contains("00:00:01,200 --> 00:00:02,500\nOlá, ação!"));
    let vtt = String::from_utf8(exports::render(&result, "vtt").unwrap()).unwrap();
    assert!(vtt.starts_with("WEBVTT\n\n"));
    assert!(vtt.contains("00:00:03.000 --> 00:00:04.200"));
    assert!(exports::render(&result, "exe").is_err());
}
#[test]
fn rejects_negative_or_reversed_timestamps() {
    for raw in [
        r#"{"transcription":[{"offsets":{"from":-1,"to":2},"text":"a"}]}"#,
        r#"{"transcription":[{"offsets":{"from":3,"to":2},"text":"a"}]}"#,
        r#"{"transcription":null}"#,
    ] {
        assert!(normalize(raw, &job()).is_err());
    }
}
#[tokio::test]
async fn verifies_integrity_including_same_size_corruption() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("model.bin");
    let spec = ModelSpec {
        id: "test".into(),
        bytes: 3,
        sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into(),
    };
    tokio::fs::write(&path, b"abc").await.unwrap();
    verify_file(&path, &spec, &CancellationToken::new())
        .await
        .unwrap();
    tokio::fs::write(&path, b"abd").await.unwrap();
    assert_eq!(
        verify_file(&path, &spec, &CancellationToken::new())
            .await
            .unwrap_err()
            .code,
        "checksum"
    );
}
#[tokio::test]
async fn restart_marks_incomplete_jobs_and_cleans_only_derived_data() {
    let temp = tempfile::tempdir().unwrap();
    let (store, mut db) = Storage::open(temp.path().join("app")).await.unwrap();
    db.jobs.push(job());
    store.save(&db).await.unwrap();
    tokio::fs::write(store.root.join("work/temporary.wav"), b"audio")
        .await
        .unwrap();
    let source = temp.path().join("source.wav");
    tokio::fs::write(&source, b"original").await.unwrap();
    let (_, recovered) = Storage::open(store.root.clone()).await.unwrap();
    assert_eq!(recovered.jobs[0].status, Status::Failed);
    assert!(!store.root.join("work/temporary.wav").exists());
    assert_eq!(tokio::fs::read(source).await.unwrap(), b"original");
}

#[test]
fn malformed_engine_output_has_an_inference_error_without_private_contents() {
    let error = normalize("private transcript: invalid JSON", &job()).unwrap_err();
    assert_eq!(error.code, "inference_output");
    assert!(!error.message.contains("private transcript"));
}

use crate::{
    errors::{AppError, Result},
    models::*,
    services::{ffmpeg, model_manager::ModelManager, process::Binaries, storage::Storage, whisper},
};
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::sync::{mpsc, Mutex, Notify};
use tokio_util::sync::CancellationToken;

pub type EventSink = Arc<dyn Fn(Snapshot) + Send + Sync>;
pub type LiveSegmentSink = Arc<dyn Fn(LiveTranscriptEvent) + Send + Sync>;
struct Inner {
    db: Database,
    tokens: HashMap<String, CancellationToken>,
    progress_emission: ProgressEmission,
}
const PROGRESS_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);
#[derive(Default)]
struct ProgressEmission {
    last: Option<tokio::time::Instant>,
    pending: bool,
}
impl ProgressEmission {
    fn due(&mut self, now: tokio::time::Instant) -> bool {
        if self.pending
            && self
                .last
                .is_none_or(|last| now.duration_since(last) >= PROGRESS_INTERVAL)
        {
            self.last = Some(now);
            self.pending = false;
            true
        } else {
            false
        }
    }
}
pub struct Manager {
    inner: Mutex<Inner>,
    pub storage: Storage,
    pub binaries: Binaries,
    pub models: Arc<ModelManager>,
    notify: Notify,
    stopping: AtomicBool,
    worker: Mutex<Option<tokio::task::JoinHandle<()>>>,
    sink: EventSink,
    live_segment_sink: LiveSegmentSink,
    // Serialize model removal/download with enqueuing, avoiding queued-model races.
    operations: Mutex<()>,
}
impl Manager {
    pub async fn new(
        storage: Storage,
        db: Database,
        binaries: Binaries,
        models: Arc<ModelManager>,
        sink: EventSink,
        live_segment_sink: LiveSegmentSink,
    ) -> Arc<Self> {
        let manager = Arc::new(Self {
            inner: Mutex::new(Inner {
                db,
                tokens: HashMap::new(),
                progress_emission: ProgressEmission::default(),
            }),
            storage,
            binaries,
            models,
            notify: Notify::new(),
            stopping: AtomicBool::new(false),
            worker: Mutex::new(None),
            sink,
            live_segment_sink,
            operations: Mutex::new(()),
        });
        let cloned = manager.clone();
        *manager.worker.lock().await = Some(tokio::spawn(async move {
            cloned.work().await;
        }));
        manager
    }
    pub async fn snapshot(&self) -> Snapshot {
        Snapshot::from(&self.inner.lock().await.db)
    }
    async fn change<T>(&self, update: impl FnOnce(&mut Database) -> Result<T>) -> Result<T> {
        let mut inner = self.inner.lock().await;
        let mut next = inner.db.clone();
        let result = update(&mut next)?;
        next.revision += 1;
        self.storage.save(&next).await?;
        inner.db = next;
        inner.progress_emission.pending = false;
        (self.sink)(Snapshot::from(&inner.db));
        Ok(result)
    }
    pub async fn enqueue(&self, media: Media, model: String, language: String) -> Result<String> {
        let _op = self.operations.lock().await;
        if self.stopping.load(Ordering::SeqCst) {
            return Err(AppError::new("closing", "O aplicativo está encerrando."));
        }
        self.models.spec(&model)?;
        if !language_valid(&language) {
            return Err(AppError::new(
                "language",
                "Selecione português, inglês ou detecção automática.",
            ));
        }
        if !self.models.path(&model)?.is_file() {
            return Err(AppError::new(
                "missing_model",
                "Instale o modelo antes de transcrever.",
            ));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let job = TranscriptionJob {
            id: id.clone(),
            source_path: media.path,
            filename: media.name,
            model,
            language,
            status: Status::Queued,
            progress: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            duration_ms: Some(media.duration_ms),
            error: None,
        };
        let mut inner = self.inner.lock().await;
        if inner
            .db
            .jobs
            .iter()
            .any(|j| j.source_path == job.source_path && j.status.active())
        {
            return Err(AppError::new(
                "duplicate",
                "Este arquivo já está na fila de transcrição.",
            ));
        }
        let mut next = inner.db.clone();
        next.jobs.push(job);
        next.revision += 1;
        self.storage.save(&next).await?;
        inner.tokens.insert(id.clone(), CancellationToken::new());
        inner.db = next;
        inner.progress_emission.pending = false;
        (self.sink)(Snapshot::from(&inner.db));
        drop(inner);
        self.notify.notify_one();
        Ok(id)
    }
    pub async fn cancel(&self, id: &str) -> Result<()> {
        let mut inner = self.inner.lock().await;
        let Some(index) = inner.db.jobs.iter().position(|j| j.id == id) else {
            return Err(missing());
        };
        if let Some(token) = inner.tokens.get(id) {
            token.cancel();
        }
        if inner.db.jobs[index].status == Status::Queued {
            let mut next = inner.db.clone();
            next.jobs[index].status = Status::Cancelled;
            next.revision += 1;
            self.storage.save(&next).await?;
            inner.db = next;
            inner.tokens.remove(id);
            inner.progress_emission.pending = false;
            (self.sink)(Snapshot::from(&inner.db));
        }
        Ok(())
    }
    async fn work(self: Arc<Self>) {
        loop {
            let notified = self.notify.notified();
            if self.stopping.load(Ordering::SeqCst) {
                break;
            }
            let next = {
                let inner = self.inner.lock().await;
                inner
                    .db
                    .jobs
                    .iter()
                    .find(|j| j.status == Status::Queued)
                    .and_then(|j| {
                        inner
                            .tokens
                            .get(&j.id)
                            .map(|token| (j.clone(), token.clone()))
                    })
            };
            let Some((job, token)) = next else {
                notified.await;
                continue;
            };
            let result = self.pipeline(&job, &token).await;
            // A cancelled process has been killed and reaped before reaching here.
            let work = self.storage.root.join("work").join(&job.id);
            let cleanup = match tokio::fs::remove_dir_all(&work).await {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(AppError::from(e)),
            };
            let result = match (result, cleanup) {
                (_, Err(e)) => Err(e),
                (result, _) => result,
            };
            if let Err(e) = &result {
                self.storage.diagnostic(&job.id, &e.code).await;
            }
            let mut inner = self.inner.lock().await;
            let mut next = inner.db.clone();
            let target = next.jobs.iter_mut().find(|j| j.id == job.id).unwrap();
            // Completion and cancellation have one linearization point under this lock.
            if token.is_cancelled() {
                target.status = Status::Cancelled;
                target.progress = None;
            } else {
                match result {
                    Ok((transcript, raw)) => {
                        target.status = Status::Completed;
                        target.progress = Some(100.0);
                        next.records.push(Record {
                            original: transcript.clone(),
                            edited: transcript,
                            source_json: raw,
                        });
                    }
                    Err(e) => {
                        target.status = Status::Failed;
                        target.progress = None;
                        target.error = Some(e.message);
                    }
                }
            }
            next.revision += 1;
            if let Err(e) = self.storage.save(&next).await {
                // Do not announce success if durable storage failed.
                next.records.retain(|r| r.edited.job_id != job.id);
                if let Some(j) = next.jobs.iter_mut().find(|j| j.id == job.id) {
                    j.status = Status::Failed;
                    j.progress = None;
                    j.error = Some(e.message);
                }
                self.storage.diagnostic(&job.id, "persistence_failed").await;
            }
            inner.db = next;
            inner.tokens.remove(&job.id);
            inner.progress_emission = ProgressEmission::default();
            (self.sink)(Snapshot::from(&inner.db));
        }
    }
    async fn pipeline(
        &self,
        job: &TranscriptionJob,
        token: &CancellationToken,
    ) -> Result<(Transcript, String)> {
        self.stage(&job.id, Status::Extracting, token).await?;
        let model = self.models.verify(&job.model, token).await?;
        let media = ffmpeg::inspect(&self.binaries, Path::new(&job.source_path), token).await?;
        let dir = self.storage.root.join("work").join(&job.id);
        tokio::fs::create_dir_all(&dir).await?;
        let audio = dir.join("audio.wav");
        let (progress_tx, mut progress_rx) = mpsc::unbounded_channel();
        let extraction = ffmpeg::extract(&self.binaries, &media, &audio, token, progress_tx);
        tokio::pin!(extraction);
        let mut progress_tick = tokio::time::interval(PROGRESS_INTERVAL);
        progress_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let extracted = loop {
            tokio::select! {
                result = &mut extraction => break result,
                _ = progress_tick.tick() => self.flush_progress().await,
                Some(out_time_us) = progress_rx.recv() => {
                    if let Some(progress) = ffmpeg::progress_percent(out_time_us, media.duration_ms) {
                        self.update_progress(&job.id, progress).await?;
                    }
                }
            }
        };
        while let Ok(out_time_us) = progress_rx.try_recv() {
            if let Some(progress) = ffmpeg::progress_percent(out_time_us, media.duration_ms) {
                self.update_progress(&job.id, progress).await?;
            }
        }
        extracted?;
        self.stage(&job.id, Status::Transcribing, token).await?;
        let (progress_tx, mut progress_rx) = mpsc::unbounded_channel();
        let (segment_tx, mut segment_rx) = mpsc::unbounded_channel();
        let output_prefix = dir.join("result");
        let inference = whisper::transcribe(
            &self.binaries,
            job,
            &model,
            &audio,
            &output_prefix,
            token,
            progress_tx,
            segment_tx,
        );
        tokio::pin!(inference);
        loop {
            tokio::select! {
                result = &mut inference => {
                    while let Ok(progress) = progress_rx.try_recv() {
                        self.update_progress(&job.id, progress as f32).await?;
                    }
                    while let Ok(segment) = segment_rx.try_recv() {
                        (self.live_segment_sink)(LiveTranscriptEvent { job_id: job.id.clone(), segment });
                    }
                    return result;
                },
                Some(progress) = progress_rx.recv() => {
                    self.update_progress(&job.id, progress as f32).await?;
                }
                _ = progress_tick.tick() => self.flush_progress().await,
                Some(segment) = segment_rx.recv() => {
                    (self.live_segment_sink)(LiveTranscriptEvent { job_id: job.id.clone(), segment });
                }
            }
        }
    }
    async fn stage(&self, id: &str, status: Status, token: &CancellationToken) -> Result<()> {
        if token.is_cancelled() {
            return Err(AppError::cancelled());
        }
        self.change(|db| {
            let job = db
                .jobs
                .iter_mut()
                .find(|j| j.id == id)
                .ok_or_else(missing)?;
            if job.status == Status::Cancelled {
                return Err(AppError::cancelled());
            }
            job.status = status;
            job.progress = None;
            Ok(())
        })
        .await?;
        self.inner.lock().await.progress_emission = ProgressEmission::default();
        Ok(())
    }
    async fn update_progress(&self, id: &str, progress: f32) -> Result<()> {
        let mut inner = self.inner.lock().await;
        let job = inner
            .db
            .jobs
            .iter_mut()
            .find(|job| job.id == id)
            .ok_or_else(missing)?;
        if !matches!(job.status, Status::Extracting | Status::Transcribing)
            || !progress.is_finite()
            || job.progress == Some(progress.clamp(0.0, 100.0))
        {
            return Ok(());
        }
        job.progress = Some(progress.clamp(0.0, 100.0));
        // Even snapshots requested between emissions must have a unique revision.
        inner.db.revision += 1;
        inner.progress_emission.pending = true;
        self.emit_progress(&mut inner);
        Ok(())
    }
    fn emit_progress(&self, inner: &mut Inner) {
        if inner.progress_emission.due(tokio::time::Instant::now()) {
            (self.sink)(Snapshot::from(&inner.db));
        }
    }
    async fn flush_progress(&self) {
        let mut inner = self.inner.lock().await;
        self.emit_progress(&mut inner);
    }
    pub async fn save_edits(&self, id: &str, texts: Vec<String>) -> Result<Transcript> {
        self.change(|db| {
            let record = db
                .records
                .iter_mut()
                .find(|r| r.edited.id == id)
                .ok_or_else(missing)?;
            if texts.len() != record.original.segments.len()
                || texts.iter().any(|s| s.len() > 100_000 || s.contains('\0'))
            {
                return Err(AppError::new(
                    "edits",
                    "Os segmentos editados estão inválidos.",
                ));
            }
            for (segment, text) in record.edited.segments.iter_mut().zip(texts) {
                segment.text = text;
            }
            record.edited.rebuild();
            Ok(record.edited.clone())
        })
        .await
    }
    pub async fn transcript(&self, id: &str) -> Result<Transcript> {
        self.inner
            .lock()
            .await
            .db
            .records
            .iter()
            .find(|r| r.edited.id == id)
            .map(|r| r.edited.clone())
            .ok_or_else(missing)
    }
    pub async fn delete(&self, id: &str) -> Result<()> {
        self.change(|db| {
            if db.jobs.iter().any(|j| j.id == id && j.status.active()) {
                return Err(AppError::new(
                    "busy",
                    "Cancele o processamento antes de excluir.",
                ));
            }
            db.records.retain(|r| r.edited.id != id);
            db.jobs.retain(|j| j.id != id);
            Ok(())
        })
        .await
    }
    pub async fn preferences(&self, preferences: Preferences) -> Result<()> {
        self.models.spec(&preferences.model)?;
        if !language_valid(&preferences.language) {
            return Err(AppError::new("language", "Idioma inválido."));
        }
        self.change(|db| {
            db.preferences = preferences;
            Ok(())
        })
        .await
    }
    pub async fn relink(&self, id: &str, media: Media) -> Result<()> {
        self.change(|db| {
            let job = db
                .jobs
                .iter_mut()
                .find(|j| j.id == id)
                .ok_or_else(missing)?;
            if job.status.active() {
                return Err(AppError::new("busy", "Aguarde o processamento terminar."));
            }
            job.source_path = media.path;
            Ok(())
        })
        .await
    }
    pub async fn remove_model(&self, id: &str) -> Result<()> {
        let _op = self.operations.lock().await;
        if self
            .inner
            .lock()
            .await
            .db
            .jobs
            .iter()
            .any(|j| j.model == id && j.status.active())
        {
            return Err(AppError::new(
                "busy",
                "Este modelo está sendo usado por um trabalho na fila.",
            ));
        }
        self.models.remove(id).await
    }
    pub async fn download_model(&self, id: &str) -> Result<()> {
        let _op = self.operations.lock().await;
        if self.stopping.load(Ordering::SeqCst) {
            return Err(AppError::new("closing", "O aplicativo está encerrando."));
        }
        if self
            .inner
            .lock()
            .await
            .db
            .jobs
            .iter()
            .any(|j| j.model == id && j.status.active())
        {
            return Err(AppError::new(
                "busy",
                "Este modelo está sendo usado por um trabalho na fila.",
            ));
        }
        self.models.download(id).await
    }
    pub async fn shutdown(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        let _op = self.operations.lock().await;
        for token in self.inner.lock().await.tokens.values() {
            token.cancel();
        }
        self.notify.notify_one();
        if let Some(worker) = self.worker.lock().await.take() {
            let _ = worker.await;
        }
        self.models.shutdown().await;
    }
}
fn missing() -> AppError {
    AppError::new(
        "missing_transcript",
        "Esta transcrição não está disponível.",
    )
}

#[cfg(test)]
mod progress_tests {
    use super::*;
    #[test]
    fn throttle_waits_250ms_and_emits_only_changes() {
        let now = tokio::time::Instant::now();
        let mut emission = ProgressEmission::default();
        assert!(!emission.due(now));
        emission.pending = true;
        assert!(emission.due(now));
        emission.pending = true;
        assert!(!emission.due(now + std::time::Duration::from_millis(249)));
        assert!(emission.due(now + PROGRESS_INTERVAL));
        assert!(!emission.due(now + PROGRESS_INTERVAL * 2));
    }
    #[tokio::test]
    async fn progress_is_transient_coalesced_ordered_and_stage_reset() {
        let temp = tempfile::tempdir().unwrap();
        let (storage, mut db) = Storage::open(temp.path().join("app")).await.unwrap();
        db.jobs.push(TranscriptionJob {
            id: "job".into(),
            source_path: "unused.wav".into(),
            filename: "unused.wav".into(),
            model: "base".into(),
            language: "pt".into(),
            status: Status::Extracting,
            progress: None,
            created_at: String::new(),
            duration_ms: Some(1000),
            error: None,
        });
        storage.save(&db).await.unwrap();
        let state = storage.root.join("state.json");
        let persisted = tokio::fs::read(&state).await.unwrap();
        let events = Arc::new(std::sync::Mutex::new(Vec::<Snapshot>::new()));
        let log = events.clone();
        let models = Arc::new(ModelManager::new(
            storage.root.join("models"),
            Arc::new(|_| {}),
        ));
        let manager = Manager::new(
            storage,
            db,
            Binaries {
                ffmpeg: "unused".into(),
                ffprobe: "unused".into(),
                whisper: "unused".into(),
            },
            models,
            Arc::new(move |event| log.lock().unwrap().push(event)),
            Arc::new(|_| {}),
        )
        .await;
        manager.update_progress("job", 1.0).await.unwrap();
        for value in 2..=99 {
            manager.update_progress("job", value as f32).await.unwrap();
        }
        assert_eq!(events.lock().unwrap().len(), 1);
        assert_eq!(manager.snapshot().await.jobs[0].progress, Some(99.0));
        assert_eq!(tokio::fs::read(&state).await.unwrap(), persisted);
        manager.inner.lock().await.progress_emission.last =
            Some(tokio::time::Instant::now() - PROGRESS_INTERVAL);
        manager.flush_progress().await;
        assert_eq!(events.lock().unwrap().len(), 2);
        assert_eq!(events.lock().unwrap()[1].jobs[0].progress, Some(99.0));
        manager.update_progress("job", 99.0).await.unwrap();
        manager.flush_progress().await;
        assert_eq!(events.lock().unwrap().len(), 2);
        manager
            .stage("job", Status::Transcribing, &CancellationToken::new())
            .await
            .unwrap();
        manager.update_progress("job", 5.0).await.unwrap();
        assert_eq!(events.lock().unwrap().len(), 4);
        manager.update_progress("job", 6.0).await.unwrap();
        manager.preferences(Preferences::default()).await.unwrap();
        manager.inner.lock().await.progress_emission.last =
            Some(tokio::time::Instant::now() - PROGRESS_INTERVAL);
        manager.flush_progress().await;
        assert_eq!(
            events.lock().unwrap().len(),
            5,
            "durable broadcast consumes pending progress"
        );
        assert!(events
            .lock()
            .unwrap()
            .windows(2)
            .all(|pair| pair[0].revision < pair[1].revision));
        // A real queued cancellation must be durable and ignore later progress.
        manager
            .change(|db| {
                db.jobs[0].status = Status::Queued;
                db.jobs[0].progress = None;
                Ok(())
            })
            .await
            .unwrap();
        manager.cancel("job").await.unwrap();
        manager.update_progress("job", 90.0).await.unwrap();
        manager.shutdown().await;
        let (_, recovered) = Storage::open(manager.storage.root.clone()).await.unwrap();
        assert_eq!(recovered.jobs[0].status, Status::Cancelled);
        assert_eq!(recovered.jobs[0].progress, None);
    }
}

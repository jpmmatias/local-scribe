use crate::{
    errors::{AppError, Result},
    models::*,
    services::{
        exports, ffmpeg,
        media_preview::PreviewServer,
        model_manager::{ModelInfo, ModelManager},
        process::Binaries,
        storage::{atomic_write, Storage},
        transcription_manager::Manager as Jobs,
    },
};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tokio_util::sync::CancellationToken;

struct Desktop {
    jobs: Arc<Jobs>,
    allowed: Mutex<HashSet<PathBuf>>,
    dirty: AtomicBool,
    closing: AtomicBool,
    preview: tokio::sync::Mutex<()>,
    media_preview: PreviewServer,
    stop: CancellationToken,
}
#[tauri::command]
async fn confirm_action(app: tauri::AppHandle, message: String) -> Result<bool> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .message(message)
        .buttons(MessageDialogButtons::OkCancel)
        .show(move |answer| {
            let _ = tx.send(answer);
        });
    rx.await
        .map_err(|_| AppError::new("dialog", "Não foi possível confirmar a operação."))
}
fn denied() -> AppError {
    AppError::new(
        "file_access",
        "Selecione o arquivo pelo diálogo ou arraste-o para a janela.",
    )
}
fn authorize(app: &tauri::AppHandle, path: &Path) -> Result<PathBuf> {
    let canonical = std::fs::canonicalize(path)?;
    app.state::<Desktop>()
        .allowed
        .lock()
        .unwrap()
        .insert(canonical.clone());
    Ok(canonical)
}
async fn selected(app: &tauri::AppHandle, path: PathBuf) -> Result<Media> {
    let path = authorize(app, &path)?;
    let state = app.state::<Desktop>();
    let media = ffmpeg::inspect(&state.jobs.binaries, &path, &state.stop).await?;
    Ok(media)
}
async fn pick(app: &tauri::AppHandle) -> Result<Option<PathBuf>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter("Audio / Video", ffmpeg::EXTENSIONS)
        .pick_file(move |file| {
            let _ = tx.send(file);
        });
    rx.await
        .map_err(|_| AppError::new("dialog", "Não foi possível abrir o seletor de arquivos."))?
        .map(|p| p.into_path().map_err(|_| denied()))
        .transpose()
}
#[tauri::command]
async fn pick_media(app: tauri::AppHandle) -> Result<Option<Media>> {
    match pick(&app).await? {
        Some(path) => Ok(Some(selected(&app, path).await?)),
        None => Ok(None),
    }
}
#[tauri::command]
async fn inspect_media(app: tauri::AppHandle, path: String) -> Result<Media> {
    let canonical = tokio::fs::canonicalize(path).await?;
    if !app
        .state::<Desktop>()
        .allowed
        .lock()
        .unwrap()
        .contains(&canonical)
    {
        return Err(denied());
    }
    selected(&app, canonical).await
}
#[tauri::command]
async fn snapshot(state: tauri::State<'_, Desktop>) -> Result<Snapshot> {
    Ok(state.jobs.snapshot().await)
}
#[tauri::command]
async fn start_job(
    app: tauri::AppHandle,
    path: String,
    model: String,
    language: String,
) -> Result<String> {
    let media = inspect_media(app.clone(), path).await?;
    app.state::<Desktop>()
        .jobs
        .enqueue(media, model, language)
        .await
}
#[tauri::command]
async fn cancel_job(state: tauri::State<'_, Desktop>, id: String) -> Result<()> {
    state.jobs.cancel(&id).await
}
#[tauri::command]
async fn save_transcript(
    state: tauri::State<'_, Desktop>,
    id: String,
    texts: Vec<String>,
) -> Result<Transcript> {
    state.jobs.save_edits(&id, texts).await
}
#[tauri::command]
async fn delete_transcript(state: tauri::State<'_, Desktop>, id: String) -> Result<()> {
    state.jobs.delete(&id).await
}
#[tauri::command]
async fn save_preferences(
    state: tauri::State<'_, Desktop>,
    preferences: Preferences,
) -> Result<()> {
    state.jobs.preferences(preferences).await
}
#[tauri::command]
async fn list_models(state: tauri::State<'_, Desktop>) -> Result<Vec<ModelInfo>> {
    state.jobs.models.list().await
}
#[tauri::command]
async fn download_model(state: tauri::State<'_, Desktop>, id: String) -> Result<()> {
    state.jobs.download_model(&id).await
}
#[tauri::command]
async fn cancel_download(state: tauri::State<'_, Desktop>, id: String) -> Result<()> {
    state.jobs.models.cancel(&id).await;
    Ok(())
}
#[tauri::command]
async fn remove_model(state: tauri::State<'_, Desktop>, id: String) -> Result<()> {
    state.jobs.remove_model(&id).await
}
#[tauri::command]
fn set_dirty(state: tauri::State<'_, Desktop>, dirty: bool) {
    state.dirty.store(dirty, Ordering::SeqCst);
}
#[tauri::command]
async fn transcript_media(app: tauri::AppHandle, id: String) -> Result<Media> {
    let snapshot = app.state::<Desktop>().jobs.snapshot().await;
    let job = snapshot
        .jobs
        .iter()
        .find(|j| j.id == id)
        .ok_or_else(denied)?;
    selected(&app, PathBuf::from(&job.source_path)).await
}
#[tauri::command]
async fn relink_media(app: tauri::AppHandle, id: String) -> Result<Option<Media>> {
    if let Some(path) = pick(&app).await? {
        let media = selected(&app, path).await?;
        app.state::<Desktop>()
            .jobs
            .relink(&id, media.clone())
            .await?;
        Ok(Some(media))
    } else {
        Ok(None)
    }
}
#[tauri::command]
async fn preview_audio(app: tauri::AppHandle, path: String) -> Result<String> {
    let media = inspect_media(app.clone(), path).await?;
    let state = app.state::<Desktop>();
    let _guard = state.preview.lock().await;
    let dir = state.jobs.storage.root.join("work").join("preview");
    tokio::fs::create_dir_all(&dir).await?;
    let output = dir.join(format!("{}.wav", uuid::Uuid::new_v4()));
    let (progress, _) = tokio::sync::mpsc::unbounded_channel();
    ffmpeg::extract(&state.jobs.binaries, &media, &output, &state.stop, progress).await?;
    state.media_preview.register(output).await
}
#[tauri::command]
async fn media_preview_url(app: tauri::AppHandle, path: String) -> Result<String> {
    let canonical = tokio::fs::canonicalize(path).await?;
    if !app
        .state::<Desktop>()
        .allowed
        .lock()
        .unwrap()
        .contains(&canonical)
    {
        return Err(denied());
    }
    app.state::<Desktop>()
        .media_preview
        .register(canonical)
        .await
}
#[tauri::command]
async fn export_transcript(
    app: tauri::AppHandle,
    id: String,
    format: String,
) -> Result<Option<String>> {
    let state = app.state::<Desktop>();
    let transcript = state.jobs.transcript(&id).await?;
    let bytes = exports::render(&transcript, &format)?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    let filename = format!(
        "{}.{}",
        Path::new(&transcript.filename)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy(),
        format
    );
    app.dialog()
        .file()
        .set_file_name(&filename)
        .add_filter(format.to_uppercase(), &[format.as_str()])
        .save_file(move |file| {
            let _ = tx.send(file);
        });
    let Some(file) = rx
        .await
        .map_err(|_| AppError::new("dialog", "Não foi possível abrir o diálogo de exportação."))?
    else {
        return Ok(None);
    };
    let path = file.into_path().map_err(|_| denied())?;
    let canonical = if path.exists() {
        tokio::fs::canonicalize(&path).await?
    } else {
        tokio::fs::canonicalize(path.parent().ok_or_else(denied)?)
            .await?
            .join(path.file_name().ok_or_else(denied)?)
    };
    let is_selected = state.allowed.lock().unwrap().contains(&canonical);
    if canonical.starts_with(&state.jobs.storage.root)
        || is_selected
        || state
            .jobs
            .snapshot()
            .await
            .jobs
            .iter()
            .any(|j| Path::new(&j.source_path) == canonical)
    {
        return Err(AppError::new("destination", "Escolha outro destino. A exportação não pode substituir a mídia original ou os dados internos."));
    }
    atomic_write(&canonical, &bytes).await?;
    Ok(Some(canonical.to_string_lossy().into()))
}
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let open_item = tauri::menu::MenuItem::with_id(
                app,
                "open",
                "Open media…",
                true,
                Some("CmdOrCtrl+O"),
            )?;
            let save_item = tauri::menu::MenuItem::with_id(
                app,
                "save",
                "Save transcript",
                true,
                Some("CmdOrCtrl+S"),
            )?;
            let file_menu = tauri::menu::SubmenuBuilder::new(app, "File")
                .item(&open_item)
                .item(&save_item)
                .build()?;
            let transcribe_item = tauri::menu::MenuItem::with_id(
                app,
                "workspace",
                "Transcribe",
                true,
                Some("CmdOrCtrl+1"),
            )?;
            let review_item =
                tauri::menu::MenuItem::with_id(app, "review", "Review", true, Some("CmdOrCtrl+2"))?;
            let history_item = tauri::menu::MenuItem::with_id(
                app,
                "history",
                "History",
                true,
                Some("CmdOrCtrl+3"),
            )?;
            let view_menu = tauri::menu::SubmenuBuilder::new(app, "View")
                .item(&transcribe_item)
                .item(&review_item)
                .item(&history_item)
                .build()?;
            let menu = tauri::menu::MenuBuilder::new(app)
                .items(&[&file_menu, &view_menu])
                .build()?;
            app.set_menu(menu)?;
            app.on_menu_event(|app, event| {
                let id = event.id().0.as_str();
                if matches!(id, "open" | "save" | "workspace" | "review" | "history") {
                    let _ = app.emit("menu-action", id.to_string());
                }
            });

            let handle = app.handle().clone();
            let model_handle = handle.clone();
            let live_handle = handle.clone();
            let root = app.path().app_data_dir()?;
            let stop = CancellationToken::new();
            let (jobs, media_preview) = tauri::async_runtime::block_on(async {
                let (storage, db) = Storage::open(root).await?;
                let media_preview = PreviewServer::start(stop.clone()).await?;
                let models = Arc::new(ModelManager::new(
                    storage.root.join("models"),
                    Arc::new(move |event| {
                        let _ = model_handle.emit("model-download", event);
                    }),
                ));
                let jobs = Jobs::new(
                    storage,
                    db,
                    Binaries::packaged()?,
                    models,
                    Arc::new(move |snapshot| {
                        let _ = handle.emit("state-changed", snapshot);
                    }),
                    Arc::new(move |event| {
                        let _ = live_handle.emit("transcription-segment", event);
                    }),
                )
                .await;
                Ok::<_, AppError>((jobs, media_preview))
            })?;
            app.manage(Desktop {
                jobs,
                media_preview,
                allowed: Mutex::new(HashSet::new()),
                dirty: AtomicBool::new(false),
                closing: AtomicBool::new(false),
                preview: tokio::sync::Mutex::new(()),
                stop,
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                begin_close(window.app_handle());
            }
            if let tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) = event {
                let selected: Vec<String> = paths
                    .iter()
                    .filter_map(|p| authorize(window.app_handle(), p).ok())
                    .map(|p| p.to_string_lossy().into())
                    .collect();
                let _ = window.emit("media-dropped", selected);
            }
        })
        .invoke_handler(tauri::generate_handler![
            confirm_action,
            pick_media,
            inspect_media,
            snapshot,
            start_job,
            cancel_job,
            save_transcript,
            delete_transcript,
            save_preferences,
            list_models,
            download_model,
            cancel_download,
            remove_model,
            set_dirty,
            transcript_media,
            relink_media,
            preview_audio,
            media_preview_url,
            export_transcript
        ])
        .build(tauri::generate_context!())
        .expect(
            "LocalScribe could not initialize; check application data permissions and JSON version",
        );
    app.run(|app, event| {
        if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
            if code.is_some() {
                return;
            }
            api.prevent_exit();
            begin_close(app);
        }
    });
}
fn begin_close(app: &tauri::AppHandle) {
    let state = app.state::<Desktop>();
    if state.closing.swap(true, Ordering::SeqCst) {
        return;
    }
    let handle = app.clone();
    let finish = move |confirmed: bool| {
        if !confirmed {
            handle
                .state::<Desktop>()
                .closing
                .store(false, Ordering::SeqCst);
            return;
        }
        tauri::async_runtime::spawn(async move {
            let state = handle.state::<Desktop>();
            state.stop.cancel();
            state.jobs.shutdown().await;
            let _guard = state.preview.lock().await;
            let _ = tokio::fs::remove_dir_all(state.jobs.storage.root.join("work")).await;
            handle.exit(0);
        });
    };
    if state.dirty.load(Ordering::SeqCst) {
        app.dialog()
            .message("Há alterações não salvas. Deseja sair e descartá-las?")
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancel)
            .show(finish);
    } else {
        finish(true);
    }
}

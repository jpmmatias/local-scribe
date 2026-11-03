use crate::errors::{AppError, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Mutex,
};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelSpec {
    pub id: String,
    pub bytes: u64,
    pub sha256: String,
}
#[derive(Deserialize)]
pub struct Catalog {
    pub revision: String,
    pub source: String,
    pub models: Vec<ModelSpec>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    #[serde(flatten)]
    pub spec: ModelSpec,
    pub installed: bool,
    pub downloading: bool,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadEvent {
    pub id: String,
    pub received: u64,
    pub total: u64,
    pub status: String,
    pub error: Option<String>,
}
pub type DownloadSink = Arc<dyn Fn(DownloadEvent) + Send + Sync>;
pub struct ModelManager {
    pub root: PathBuf,
    pub catalog: Catalog,
    downloads: Mutex<HashMap<String, CancellationToken>>,
    sink: DownloadSink,
}
impl ModelManager {
    pub fn new(root: PathBuf, sink: DownloadSink) -> Self {
        Self {
            root,
            catalog: serde_json::from_str(include_str!("../../model-catalog.json"))
                .expect("checked model catalog"),
            downloads: Mutex::new(HashMap::new()),
            sink,
        }
    }
    pub fn spec(&self, id: &str) -> Result<&ModelSpec> {
        self.catalog
            .models
            .iter()
            .find(|m| m.id == id)
            .ok_or_else(|| AppError::new("model", "Modelo não suportado."))
    }
    pub fn path(&self, id: &str) -> Result<PathBuf> {
        self.spec(id)?;
        Ok(self.root.join(format!("ggml-{id}.bin")))
    }
    pub async fn list(&self) -> Result<Vec<ModelInfo>> {
        let mut out = vec![];
        for spec in &self.catalog.models {
            let installed = fs::metadata(self.path(&spec.id)?)
                .await
                .is_ok_and(|m| m.is_file() && m.len() == spec.bytes);
            out.push(ModelInfo {
                spec: spec.clone(),
                installed,
                downloading: self.downloads.lock().await.contains_key(&spec.id),
            });
        }
        Ok(out)
    }
    pub async fn verify(&self, id: &str, cancel: &CancellationToken) -> Result<PathBuf> {
        let spec = self.spec(id)?;
        let path = self.path(id)?;
        if !path.is_file() {
            return Err(AppError::new(
                "missing_model",
                "Instale o modelo selecionado em Manage Models antes de transcrever.",
            ));
        }
        verify_file(&path, spec, cancel).await?;
        Ok(path)
    }
    pub async fn download(self: &Arc<Self>, id: &str) -> Result<()> {
        let spec = self.spec(id)?.clone();
        let mut active = self.downloads.lock().await;
        if active.contains_key(id) {
            return Err(AppError::new(
                "download_active",
                "Este modelo já está sendo baixado.",
            ));
        }
        if self.verify(id, &CancellationToken::new()).await.is_ok() {
            return Ok(());
        }
        let token = CancellationToken::new();
        active.insert(id.into(), token.clone());
        drop(active);
        let this = self.clone();
        tokio::spawn(async move {
            let result = this.fetch(&spec, &token).await;
            let _ = fs::remove_file(this.root.join(format!("{}.part", spec.id))).await;
            this.downloads.lock().await.remove(&spec.id);
            let event = match result {
                Ok(()) => DownloadEvent {
                    id: spec.id,
                    received: spec.bytes,
                    total: spec.bytes,
                    status: "completed".into(),
                    error: None,
                },
                Err(e) => DownloadEvent {
                    id: spec.id,
                    received: 0,
                    total: spec.bytes,
                    status: if e.code == "cancelled" {
                        "cancelled"
                    } else {
                        "failed"
                    }
                    .into(),
                    error: Some(e.message),
                },
            };
            (this.sink)(event);
        });
        Ok(())
    }
    async fn fetch(&self, spec: &ModelSpec, token: &CancellationToken) -> Result<()> {
        if fs2::available_space(&self.root)? < spec.bytes + 16_000_000 {
            return Err(AppError::new(
                "disk_full",
                "Espaço insuficiente para instalar este modelo.",
            ));
        }
        // Only this pinned upstream and its HTTPS CDN redirects are allowed.
        let client = reqwest::Client::builder()
            .https_only(true)
            .connect_timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                let host = attempt.url().host_str().unwrap_or("");
                if attempt.previous().len() >= 5
                    || attempt.url().scheme() != "https"
                    || !(host == "huggingface.co"
                        || host.ends_with(".huggingface.co")
                        || host.ends_with(".hf.co"))
                {
                    attempt.error("untrusted model redirect")
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map_err(|_| network_error())?;
        let url = format!(
            "{}/resolve/{}/ggml-{}.bin",
            self.catalog.source, self.catalog.revision, spec.id
        );
        let response = tokio::select! { r = client.get(url).send() => r.map_err(|_| network_error())?.error_for_status().map_err(|_| network_error())?, _ = token.cancelled() => return Err(AppError::cancelled()) };
        let part = self.root.join(format!("{}.part", spec.id));
        let mut file = fs::File::create(&part).await?;
        let mut stream = response.bytes_stream();
        let mut received = 0u64;
        let mut last = std::time::Instant::now() - Duration::from_secs(1);
        (self.sink)(DownloadEvent {
            id: spec.id.clone(),
            received,
            total: spec.bytes,
            status: "downloading".into(),
            error: None,
        });
        loop {
            let chunk = tokio::select! { r = tokio::time::timeout(Duration::from_secs(60), stream.next()) => r.map_err(|_| network_error())?, _ = token.cancelled() => return Err(AppError::cancelled()) };
            let Some(chunk) = chunk else {
                break;
            };
            let chunk = chunk.map_err(|_| network_error())?;
            received += chunk.len() as u64;
            if received > spec.bytes {
                return Err(AppError::new(
                    "checksum",
                    "O download tem tamanho inválido. Tente novamente.",
                ));
            }
            file.write_all(&chunk).await?;
            if last.elapsed() >= Duration::from_millis(200) {
                (self.sink)(DownloadEvent {
                    id: spec.id.clone(),
                    received,
                    total: spec.bytes,
                    status: "downloading".into(),
                    error: None,
                });
                last = std::time::Instant::now();
            }
        }
        file.sync_all().await?;
        drop(file);
        (self.sink)(DownloadEvent {
            id: spec.id.clone(),
            received,
            total: spec.bytes,
            status: "verifying".into(),
            error: None,
        });
        verify_file(&part, spec, token).await?;
        if token.is_cancelled() {
            return Err(AppError::cancelled());
        }
        fs::rename(&part, self.path(&spec.id)?).await?;
        Ok(())
    }
    pub async fn cancel(&self, id: &str) {
        if let Some(token) = self.downloads.lock().await.get(id) {
            token.cancel();
        }
    }
    pub async fn remove(&self, id: &str) -> Result<()> {
        let active = self.downloads.lock().await;
        if active.contains_key(id) {
            return Err(AppError::new(
                "busy",
                "Cancele o download antes de remover o modelo.",
            ));
        }
        let path = self.path(id)?;
        if path.exists() {
            fs::remove_file(path).await?;
        }
        Ok(())
    }
    pub async fn shutdown(&self) {
        for token in self.downloads.lock().await.values() {
            token.cancel();
        }
        loop {
            if self.downloads.lock().await.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}
fn network_error() -> AppError {
    AppError::new("download", "Não foi possível baixar o modelo. Verifique sua conexão e tente novamente. Modelos já instalados funcionam offline.")
}
pub async fn verify_file(path: &Path, spec: &ModelSpec, cancel: &CancellationToken) -> Result<()> {
    let mut file = fs::File::open(path).await?;
    let invalid = || {
        AppError::new(
            "checksum",
            "O modelo está incompleto ou corrompido. Remova e baixe novamente.",
        )
    };
    if file.metadata().await?.len() != spec.bytes {
        return Err(invalid());
    }
    let mut hash = Sha256::new();
    let mut buf = vec![0; 64 * 1024];
    loop {
        if cancel.is_cancelled() {
            return Err(AppError::cancelled());
        }
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    if format!("{:x}", hash.finalize()) != spec.sha256 {
        return Err(invalid());
    }
    Ok(())
}

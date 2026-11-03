use crate::{
    errors::{AppError, Result},
    models::{Database, Status},
};
use std::path::{Path, PathBuf};
use tokio::{fs, io::AsyncWriteExt};

#[derive(Clone)]
pub struct Storage {
    pub root: PathBuf,
}
impl Storage {
    pub async fn open(root: PathBuf) -> Result<(Self, Database)> {
        for p in [&root, &root.join("models"), &root.join("work")] {
            fs::create_dir_all(p).await?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).await?;
        }
        let storage = Self { root };
        let mut db: Database = match fs::read(storage.root.join("state.json")).await {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Database::default(),
            Err(e) => return Err(e.into()),
        };
        if db.version != 1 {
            return Err(AppError::new(
                "version",
                "Este projeto foi criado por uma versão incompatível do LocalScribe.",
            ));
        }
        for job in &mut db.jobs {
            if job.status.active() {
                job.status = Status::Failed;
                job.progress = None;
                job.error = Some("O aplicativo foi encerrado durante o processamento. Inicie uma nova tentativa.".into());
            }
        }
        // Only our derived, private working directory is discarded on recovery.
        fs::remove_dir_all(storage.root.join("work")).await?;
        fs::create_dir(storage.root.join("work")).await?;
        let mut entries = fs::read_dir(storage.root.join("models")).await?;
        while let Some(entry) = entries.next_entry().await? {
            if entry.path().extension().is_some_and(|x| x == "part") {
                fs::remove_file(entry.path()).await?;
            }
        }
        db.revision += 1;
        storage.save(&db).await?;
        Ok((storage, db))
    }
    pub async fn save(&self, db: &Database) -> Result<()> {
        atomic_write(&self.root.join("state.json"), &serde_json::to_vec(db)?).await
    }
    pub async fn diagnostic(&self, job_id: &str, code: &str) {
        // No file paths, audio, CLI stderr, or transcript text enter logs.
        let path = self.root.join("diagnostics.log");
        if fs::metadata(&path).await.is_ok_and(|m| m.len() > 256_000) {
            let _ = fs::rename(&path, self.root.join("diagnostics.previous.log")).await;
        }
        if let Ok(mut f) = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await
        {
            let _ = f
                .write_all(
                    format!(
                        "{} job={} code={}\n",
                        chrono::Utc::now().to_rfc3339(),
                        job_id,
                        code
                    )
                    .as_bytes(),
                )
                .await;
        }
    }
}
pub async fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::new("destination", "Pasta de destino inválida."))?;
    let tmp = parent.join(format!(".localscribe-{}.tmp", uuid::Uuid::new_v4()));
    let result = async {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .await?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            f.set_permissions(std::fs::Permissions::from_mode(0o600))
                .await?;
        }
        f.write_all(bytes).await?;
        f.sync_all().await?;
        drop(f);
        fs::rename(&tmp, path).await?;
        Ok(())
    }
    .await;
    if result.is_err() {
        let _ = fs::remove_file(&tmp).await;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn rejects_corruption_without_overwriting() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.json");
        fs::write(&path, "invalid").await.unwrap();
        assert!(Storage::open(temp.path().into()).await.is_err());
        assert_eq!(fs::read_to_string(path).await.unwrap(), "invalid");
    }
    #[tokio::test]
    async fn replaces_complete_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.json");
        atomic_write(&path, b"first").await.unwrap();
        atomic_write(&path, b"second").await.unwrap();
        assert_eq!(fs::read(path).await.unwrap(), b"second");
    }
}

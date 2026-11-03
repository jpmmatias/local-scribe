//! CPU-only execution policy. Keep automatic selection opt-in until release gates pass.
use crate::errors::{AppError, Result};
use std::path::{Path, PathBuf};

pub const AUTO_SELECTION_RELEASED: bool = false;

pub fn capabilities() -> [bool; 6] {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return [
        std::is_x86_feature_detected!("avx"),
        std::is_x86_feature_detected!("avx2"),
        std::is_x86_feature_detected!("fma"),
        std::is_x86_feature_detected!("f16c"),
        std::is_x86_feature_detected!("bmi2"),
        std::is_x86_feature_detected!("sse4.2"),
    ];
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    [false; 6]
}

pub fn available_threads() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
}

pub fn threads(value: Option<&str>, available: usize) -> Result<usize> {
    let available = available.max(1);
    match value {
        None => Ok(available.min(4)),
        Some(value) if !value.is_empty() && value.bytes().all(|c| c.is_ascii_digit()) => value
            .parse::<usize>()
            .ok()
            .filter(|n| (1..=available).contains(n))
            .ok_or_else(invalid_threads),
        _ => Err(invalid_threads()),
    }
}
fn invalid_threads() -> AppError {
    AppError::new(
        "cpu_config",
        "LOCALSCRIBE_THREADS deve ser um inteiro entre 1 e o paralelismo disponível.",
    )
}

pub fn thread_count() -> Result<usize> {
    let value = env("LOCALSCRIBE_THREADS")?;
    threads(value.as_deref(), available_threads())
}
fn env(name: &str) -> Result<Option<String>> {
    match std::env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(_) => Err(AppError::new("cpu_config", "Configuração de CPU inválida.")),
    }
}

pub fn automatic() -> Result<bool> {
    match env("LOCALSCRIBE_CPU")?.as_deref() {
        None => Ok(AUTO_SELECTION_RELEASED),
        Some("auto") => Ok(true),
        Some("baseline") => Ok(false),
        _ => Err(AppError::new(
            "cpu_config",
            "LOCALSCRIBE_CPU deve ser baseline ou auto.",
        )),
    }
}

pub fn select(
    baseline: PathBuf,
    optimized: PathBuf,
    features: [bool; 6],
    automatic: bool,
) -> PathBuf {
    if automatic && features.iter().all(|feature| *feature) && executable(&optimized) {
        optimized
    } else {
        baseline
    }
}
fn executable(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    metadata.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn thread_limits() {
        for (available, expected) in [(0, 1), (1, 1), (2, 2), (8, 4)] {
            assert_eq!(threads(None, available).unwrap(), expected);
        }
        for value in [
            "",
            "0",
            "5",
            "-1",
            "+1",
            "1.5",
            "two",
            " 2",
            "999999999999999999999999999",
        ] {
            assert!(threads(Some(value), 4).is_err());
        }
        assert_eq!(threads(Some("4"), 4).unwrap(), 4);
        assert_eq!(threads(Some("1"), 1).unwrap(), 1);
    }
    #[test]
    fn selection_requires_every_feature_and_executable() {
        let dir = tempfile::tempdir().unwrap();
        let baseline = dir.path().join("baseline");
        let optimized = dir.path().join("avx2");
        let select_with =
            |features, automatic| select(baseline.clone(), optimized.clone(), features, automatic);
        assert_eq!(select_with([true; 6], true), baseline);
        std::fs::write(&optimized, "test").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(select_with([true; 6], true), baseline);
            std::fs::set_permissions(&optimized, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert_eq!(select_with([true; 6], true), optimized);
        assert_eq!(select_with([true; 6], false), baseline);
        assert_eq!(select_with([false; 6], true), baseline);
        for missing in 0..6 {
            let mut features = [true; 6];
            features[missing] = false;
            assert_eq!(select_with(features, true), baseline);
        }
    }
}

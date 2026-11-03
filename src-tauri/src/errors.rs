use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct AppError {
    pub code: String,
    pub message: String,
}
pub type Result<T> = std::result::Result<T, AppError>;
impl AppError {
    pub fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
    pub fn cancelled() -> Self {
        Self::new("cancelled", "Processamento cancelado.")
    }
}
impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for AppError {}
impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::PermissionDenied => Self::new(
                "permission",
                "Sem permissão para acessar este arquivo ou pasta.",
            ),
            std::io::ErrorKind::NotFound => Self::new(
                "not_found",
                "Arquivo não encontrado. Ele pode ter sido movido ou excluído.",
            ),
            _ if e.raw_os_error() == Some(28) => Self::new(
                "disk_full",
                "Espaço em disco insuficiente. Libere espaço e tente novamente.",
            ),
            _ => Self::new(
                "io",
                "Não foi possível ler ou gravar o arquivo. Verifique o disco e as permissões.",
            ),
        }
    }
}
impl From<serde_json::Error> for AppError {
    fn from(_: serde_json::Error) -> Self {
        Self::new(
            "invalid_data",
            "Os dados salvos ou a saída do processador estão inválidos.",
        )
    }
}

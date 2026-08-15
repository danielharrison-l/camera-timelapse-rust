use serde::{Deserialize, Serialize};

/// Evento recebido da fila SQS `camera-timelapse-inbound` (enviado pela Fiscaliza API)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboundTimelapseEvent {
    /// ID único do job / requisição gerado pela Fiscaliza API
    pub job_id: String,
    /// Nome da pasta no SFTP (ex: "2026_07_28-2026_07_28" ou "2026_08_09-2026_09_07")
    pub session: String,
    /// Identificador opcional da câmera no Fiscaliza
    #[serde(default)]
    pub camera_id: Option<String>,
    /// Host do servidor SFTP da câmera (opcional, fallback para env)
    #[serde(default)]
    pub sftp_host: Option<String>,
    /// Porta do servidor SFTP (opcional, fallback para env)
    #[serde(default)]
    pub sftp_port: Option<u16>,
    /// Usuário do SFTP cifrado com RTSP_ENC_KEY (opcional, fallback para env)
    #[serde(default)]
    pub sftp_username: Option<String>,
    /// Senha do SFTP cifrada com RTSP_ENC_KEY (opcional, fallback para env)
    #[serde(default)]
    pub sftp_password: Option<String>,
    /// Limite opcional dos N primeiros frames (útil para testes/desenvolvimento)
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Evento publicado na fila SQS `camera-timelapse-outbound` (consumido pela Fiscaliza API)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutboundTimelapseEvent {
    /// ID do job correspondente
    pub job_id: String,
    /// Status do processamento: "COMPLETED" ou "FAILED"
    pub status: String,
    /// Nome da pasta / sessão processada
    pub session: String,
    /// Câmera vinculada
    pub camera_id: Option<String>,
    /// Bucket onde o arquivo MP4 foi salvo no S3
    pub s3_bucket: Option<String>,
    /// Chave do objeto no S3 (ex: "videos/2026_07_28-2026_07_28.mp4")
    pub s3_key: Option<String>,
    /// URL pública/endpoint para acesso ao arquivo MP4
    pub s3_url: Option<String>,
    /// Tamanho do vídeo em bytes
    pub file_size_bytes: u64,
    /// Tempo total gasto no processamento em segundos
    pub duration_seconds: f64,
    /// Quantidade total de fotos/frames codificados
    pub total_frames: usize,
    /// Frames por segundo (FPS) do vídeo final
    pub fps: u32,
    /// Data/hora de conclusão (ISO 8601 UTC)
    pub processed_at: String,
    /// Mensagem de erro caso o status seja "FAILED"
    pub error: Option<String>,
}

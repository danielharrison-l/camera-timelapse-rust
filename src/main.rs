mod config;
mod crypto;
mod encoder;
mod models;
mod s3;
mod sftp;
mod sqs;

use anyhow::Result;
use chrono::Utc;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tracing::{error, info, Level};
use tracing_subscriber::FmtSubscriber;

use config::Config;
use crypto::RtspCipher;
use encoder::FfmpegEncoder;
use models::OutboundTimelapseEvent;
use s3::S3Storage;
use sftp::{SftpConfig, SftpDownloader};
use sqs::SqsService;

const FIXED_FPS: u32 = 30;

fn resolve_sftp_config(event: &models::InboundTimelapseEvent, config: &Config) -> Result<SftpConfig> {
    let host = event.sftp_host.clone().unwrap_or_else(|| config.sftp_host.clone());
    let port = event.sftp_port.unwrap_or(config.sftp_port);

    let username = match &event.sftp_username {
        Some(raw_user) if !raw_user.trim().is_empty() => {
            if raw_user.starts_with("v1:") || raw_user.contains(':') {
                let key = config.rtsp_enc_key.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Usuário SFTP cifrado recebido no SQS, mas a variável RTSP_ENC_KEY não foi configurada no serviço.")
                })?;
                RtspCipher::decrypt(raw_user, key)?
            } else {
                raw_user.clone()
            }
        }
        _ => config.sftp_username.clone(),
    };

    let password = match &event.sftp_password {
        Some(raw_pass) if !raw_pass.trim().is_empty() => {
            if raw_pass.starts_with("v1:") || raw_pass.contains(':') {
                let key = config.rtsp_enc_key.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Senha SFTP cifrada recebida no SQS, mas a variável RTSP_ENC_KEY não foi configurada no serviço.")
                })?;
                RtspCipher::decrypt(raw_pass, key)?
            } else {
                raw_pass.clone()
            }
        }
        _ => config.sftp_password.clone(),
    };

    Ok(SftpConfig {
        host,
        port,
        username,
        password,
    })
}

async fn run_standalone_cli(config: Arc<Config>) -> Result<()> {
    info!("🎬 Executando Microserviço em MODO STANDALONE CLI (Sem dependência de SQS)...");
    info!("📌 Servidor SFTP Target : {}:{}", config.sftp_host, config.sftp_port);
    info!("📌 Usuário SFTP Target  : {}", config.sftp_username);
    info!("🔍 Buscando todas as imagens em todas as pastas na raiz ('/')...");

    let start_time = Instant::now();
    let sftp_downloader = SftpDownloader::new(config.clone());
    let encoder = FfmpegEncoder::new(config.ffmpeg_path.clone());

    let sftp_cfg = SftpConfig {
        host: config.sftp_host.clone(),
        port: config.sftp_port,
        username: config.sftp_username.clone(),
        password: config.sftp_password.clone(),
    };

    let session = "all";
    let temp_dir = PathBuf::from("temp_output/standalone_cli");
    let frames_dir = temp_dir.join("frames");
    let output_mp4 = PathBuf::from("output_standalone_timelapse.mp4");

    // 1. Download de todas as fotos do período completo
    let total_frames = sftp_downloader
        .download_frames(&sftp_cfg, session, &frames_dir, None)
        .await?;

    // 2. Renderização em 30 FPS fixos
    encoder
        .encode_frames(&frames_dir, &output_mp4, FIXED_FPS, None, total_frames)
        .await?;

    let duration_secs = start_time.elapsed().as_secs_f64();
    let file_size_mb = match std::fs::metadata(&output_mp4) {
        Ok(meta) => meta.len() as f64 / (1024.0 * 1024.0),
        Err(_) => 0.0,
    };

    info!(
        "✅ Processamento Standalone Concluído em {:.2}s! {} frames | {:.2} MB",
        duration_secs, total_frames, file_size_mb
    );
    info!("📹 Vídeo final gerado com sucesso em: {}", output_mp4.display());

    // Limpa diretório de frames temporários
    let _ = tokio::fs::remove_dir_all(&temp_dir).await;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("Falha ao inicializar o logger tracing");

    let config = Arc::new(Config::from_env());

    // Verifica se foi executado com argumentos de linha de comando ou env CLI_MODE
    let args: Vec<String> = std::env::args().collect();
    let is_cli_mode = args.iter().any(|arg| arg == "--cli" || arg == "--standalone" || arg == "-c")
        || std::env::var("CLI_MODE").unwrap_or_default() == "true"
        || std::env::var("CLI_MODE").unwrap_or_default() == "1";

    if is_cli_mode {
        return run_standalone_cli(config).await;
    }

    info!("🚀 Iniciando Microserviço Event-Driven Rust Timelapse (Modo SQS Daemon)...");

    let sqs_service = SqsService::new(config.clone()).await?;
    let sftp_downloader = SftpDownloader::new(config.clone());
    let encoder = FfmpegEncoder::new(config.ffmpeg_path.clone());
    let s3_storage = S3Storage::new(config.clone()).await?;

    info!("📌 SQS Inbound Queue  : {}", config.sqs_inbound_queue_url);
    info!("📌 SQS Outbound Queue : {}", config.sqs_outbound_queue_url);
    info!("📌 Target S3 Bucket   : {}", config.s3_default_bucket);
    info!("📌 FPS Fixo Config.   : {} FPS", FIXED_FPS);
    info!("⚡ Servidor pronto. Aguardando eventos da Fiscaliza API na fila SQS...");

    loop {
        match sqs_service.poll_inbound_messages().await {
            Ok(messages) => {
                for msg in messages {
                    let start_time = Instant::now();
                    let event = msg.event;
                    let receipt_handle = msg.receipt_handle;

                    info!(
                        "📥 Evento Recebido | Job ID: '{}' | Sessão: '{}' | Câmera: '{:?}'",
                        event.job_id, event.session, event.camera_id
                    );

                    let temp_dir = PathBuf::from(format!("temp_output/{}", event.job_id));
                    let frames_dir = temp_dir.join("frames");
                    let output_mp4 = temp_dir.join(format!("{}.mp4", event.session));

                    let config_clone = config.clone();
                    let sftp_downloader_clone = sftp_downloader.clone();
                    let encoder_clone = encoder.clone();
                    let s3_storage_clone = s3_storage.clone();
                    let event_clone = event.clone();

                    let result: Result<(usize, String, String, String, u64)> = async move {
                        // 0. Resolução e Descriptografia de Credenciais SFTP (RTSP_ENC_KEY)
                        let sftp_cfg = resolve_sftp_config(&event_clone, &config_clone)?;

                        // 1. Download das fotos via SFTP Pipelined
                        let total_frames = sftp_downloader_clone
                            .download_frames(&sftp_cfg, &event_clone.session, &frames_dir, event_clone.limit)
                            .await?;

                        // 2. Renderização Multi-Core Parallel Chunk Encoding a 30 FPS fixos
                        encoder_clone
                            .encode_frames(&frames_dir, &output_mp4, FIXED_FPS, None, total_frames)
                            .await?;

                        // 3. Upload do vídeo para o S3 (Bucket e Prefixo lidos via envs)
                        let target_s3_bucket = config_clone.s3_default_bucket.clone();
                        let target_s3_key = format!(
                            "{}{}/{}.mp4",
                            config_clone.s3_default_prefix, event_clone.job_id, event_clone.session
                        );

                        let (bucket, key, url, size_bytes) = s3_storage_clone
                            .upload_file(&output_mp4, Some(&target_s3_bucket), Some(&target_s3_key))
                            .await?;

                        Ok((total_frames, bucket, key, url, size_bytes))
                    }
                    .await;

                    let duration_secs = start_time.elapsed().as_secs_f64();
                    let processed_at = Utc::now().to_rfc3339();

                    match result {
                        Ok((total_frames, bucket, key, url, size_bytes)) => {
                            info!(
                                "✅ Job '{}' concluído em {:.2}s! {} frames | {} MB | URL: {}",
                                event.job_id, duration_secs, total_frames, size_bytes / (1024 * 1024), url
                            );

                            let outbound_event = OutboundTimelapseEvent {
                                job_id: event.job_id.clone(),
                                status: "COMPLETED".to_string(),
                                session: event.session.clone(),
                                camera_id: event.camera_id.clone(),
                                s3_bucket: Some(bucket),
                                s3_key: Some(key),
                                s3_url: Some(url),
                                file_size_bytes: size_bytes,
                                duration_seconds: duration_secs,
                                total_frames,
                                fps: FIXED_FPS,
                                processed_at,
                                error: None,
                            };

                            if let Err(e) = sqs_service.publish_outbound_event(&outbound_event).await {
                                error!("Erro ao publicar evento de conclusão para SQS Outbound: {}", e);
                            }

                            if let Err(e) = sqs_service.delete_inbound_message(&receipt_handle).await {
                                error!("Erro ao deletar mensagem da fila Inbound: {}", e);
                            }
                        }
                        Err(err) => {
                            let err_msg = err.to_string();
                            error!("❌ Job '{}' falhou em {:.2}s: {}", event.job_id, duration_secs, err_msg);

                            let outbound_event = OutboundTimelapseEvent {
                                job_id: event.job_id.clone(),
                                status: "FAILED".to_string(),
                                session: event.session.clone(),
                                camera_id: event.camera_id.clone(),
                                s3_bucket: None,
                                s3_key: None,
                                s3_url: None,
                                file_size_bytes: 0,
                                duration_seconds: duration_secs,
                                total_frames: 0,
                                fps: FIXED_FPS,
                                processed_at,
                                error: Some(err_msg),
                            };

                            let _ = sqs_service.publish_outbound_event(&outbound_event).await;
                            let _ = sqs_service.delete_inbound_message(&receipt_handle).await;
                        }
                    }

                    // Limpa diretório de arquivos temporários do job
                    let _ = tokio::fs::remove_dir_all(&temp_dir).await;
                }
            }
            Err(e) => {
                error!("Erro ao fazer polling no SQS Inbound: {}", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        }
    }
}

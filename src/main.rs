mod config;
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
use encoder::FfmpegEncoder;
use models::OutboundTimelapseEvent;
use s3::S3Storage;
use sftp::SftpDownloader;
use sqs::SqsService;

#[tokio::main]
async fn main() -> Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("Falha ao inicializar o logger tracing");

    info!("🚀 Iniciando Microserviço Event-Driven Rust Timelapse...");

    let config = Arc::new(Config::from_env());
    let sqs_service = SqsService::new(config.clone()).await?;
    let sftp_downloader = SftpDownloader::new(config.clone());
    let encoder = FfmpegEncoder::new(config.ffmpeg_path.clone());
    let s3_storage = S3Storage::new(config.clone()).await?;

    info!("📌 SQS Inbound Queue  : {}", config.sqs_inbound_queue_url);
    info!("📌 SQS Outbound Queue : {}", config.sqs_outbound_queue_url);
    info!("📌 Target S3 Bucket   : {}", config.s3_default_bucket);
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

                    let fps = event.fps.unwrap_or(10);
                    let temp_dir = PathBuf::from(format!("temp_output/{}", event.job_id));
                    let frames_dir = temp_dir.join("frames");
                    let output_mp4 = temp_dir.join(format!("{}.mp4", event.session));

                    let result: Result<(usize, String, String, String, u64)> = async {
                        // 1. Download das fotos via SFTP Pipelined (93.7 MB/s)
                        let total_frames = sftp_downloader
                            .download_frames(&event.session, &frames_dir, event.limit)
                            .await?;

                        // 2. Renderização Multi-Core Parallel Chunk Encoding (450+ FPS)
                        encoder
                            .encode_frames(&frames_dir, &output_mp4, fps, event.scale.as_deref(), total_frames)
                            .await?;

                        // 3. Upload do vídeo para o S3 / LocalStack
                        let (bucket, key, url, size_bytes) = s3_storage
                            .upload_file(&output_mp4, event.s3_bucket.as_deref(), event.s3_key.as_deref())
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
                                fps,
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
                                fps,
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

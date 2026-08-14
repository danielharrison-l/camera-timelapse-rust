use anyhow::{anyhow, Result};
use std::fs;
use std::path::Path;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command as TokioCommand;
use tracing::info;

pub struct FfmpegEncoder {
    ffmpeg_path: String,
}

impl FfmpegEncoder {
    pub fn new(ffmpeg_path: String) -> Self {
        Self { ffmpeg_path }
    }

    pub async fn encode_frames(
        &self,
        frames_dir: &Path,
        output_mp4: &Path,
        fps: u32,
        scale: Option<&str>,
        total_frames: usize,
    ) -> Result<()> {
        if let Some(parent) = output_mp4.parent() {
            fs::create_dir_all(parent)?;
        }

        let scale_filter = match scale {
            Some(s) if s.starts_with("scale=") => s.to_string(),
            Some(s) => format!("scale={}", s),
            None => "scale=1280:720".to_string(),
        };

        if total_frames == 0 {
            return Err(anyhow!("Nenhum frame para codificar"));
        }

        let num_chunks = 4;
        let chunk_size = (total_frames + num_chunks - 1) / num_chunks;
        let temp_dir = frames_dir.join("temp_ts_chunks");
        fs::create_dir_all(&temp_dir)?;

        let mut chunk_tasks = Vec::new();
        let mut ts_files = Vec::new();

        info!("FFmpeg: Codificando {} frames em {} partes paralelas via Image Pipe stdin", total_frames, num_chunks);

        for chunk_idx in 0..num_chunks {
            let start_idx = chunk_idx * chunk_size + 1;
            let end_idx = std::cmp::min((chunk_idx + 1) * chunk_size, total_frames);

            if start_idx > total_frames {
                break;
            }

            let ts_output = temp_dir.join(format!("segment_{}.ts", chunk_idx));
            ts_files.push(ts_output.clone());

            let ffmpeg_path = self.ffmpeg_path.clone();
            let scale_filter = scale_filter.clone();
            let fps_str = fps.to_string();
            let frames_dir_buf = frames_dir.to_path_buf();

            let task = tokio::spawn(async move {
                let mut child = TokioCommand::new(&ffmpeg_path)
                    .args(&[
                        "-y",
                        "-hide_banner",
                        "-loglevel",
                        "error",
                        "-f",
                        "image2pipe",
                        "-c:v",
                        "mjpeg",
                        "-framerate",
                        &fps_str,
                        "-i",
                        "-",
                        "-vf",
                        &scale_filter,
                        "-c:v",
                        "libx264",
                        "-preset",
                        "ultrafast",
                        "-crf",
                        "28",
                        "-threads",
                        "0",
                        "-pix_fmt",
                        "yuv420p",
                        "-f",
                        "mpegts",
                        ts_output.to_str().unwrap(),
                    ])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()?;

                let mut stdin = child
                    .stdin
                    .take()
                    .ok_or_else(|| anyhow!("Falha ao abrir stdin para o segmento {}", chunk_idx))?;

                let mut buffer = vec![0u8; 256 * 1024];

                for frame_idx in start_idx..=end_idx {
                    let frame_path = frames_dir_buf.join(format!("frame_{:06}.jpg", frame_idx));
                    if let Ok(mut f) = tokio::fs::File::open(&frame_path).await {
                        loop {
                            match tokio::io::AsyncReadExt::read(&mut f, &mut buffer).await {
                                Ok(0) => break,
                                Ok(n) => {
                                    if stdin.write_all(&buffer[..n]).await.is_err() {
                                        break;
                                    }
                                }
                                Err(_) => break,
                            }
                        }
                    }
                }

                // Fecha stdin explicitamente enviando EOF para o FFmpeg
                drop(stdin);

                let status = child.wait().await?;
                if !status.success() {
                    return Err(anyhow!("FFmpeg chunk {} falhou com status {}", chunk_idx, status));
                }
                Ok::<(), anyhow::Error>(())
            });

            chunk_tasks.push(task);
        }

        for task in chunk_tasks {
            task.await??;
        }

        let concat_str = ts_files
            .iter()
            .map(|p| p.to_str().unwrap())
            .collect::<Vec<&str>>()
            .join("|");
        let concat_input = format!("concat:{}", concat_str);

        info!("FFmpeg: Concatenando {} segmentos .ts em MP4 final via copy", ts_files.len());

        let mut concat_child = TokioCommand::new(&self.ffmpeg_path)
            .args(&[
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-i",
                &concat_input,
                "-c",
                "copy",
                "-movflags",
                "+faststart",
                output_mp4.to_str().unwrap(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        let concat_status = concat_child.wait().await?;

        let _ = fs::remove_dir_all(&temp_dir);

        if concat_status.success() {
            Ok(())
        } else {
            Err(anyhow!("FFmpeg concat falhou com status {}", concat_status))
        }
    }
}

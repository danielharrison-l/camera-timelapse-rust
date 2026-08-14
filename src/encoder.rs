use anyhow::{anyhow, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
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
        let ffmpeg_path = self.ffmpeg_path.clone();
        let frames_dir = frames_dir.to_path_buf();
        let output_mp4 = output_mp4.to_path_buf();
        let scale = scale.map(|s| s.to_string());

        tokio::task::spawn_blocking(move || {
            let encoder = FfmpegEncoder::new(ffmpeg_path);
            encoder.encode_sync(&frames_dir, &output_mp4, fps, scale.as_deref(), total_frames)
        })
        .await?
    }

    fn encode_sync(
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

        let scale_filter = scale.unwrap_or("scale=1280:720");

        if total_frames < 20 {
            return self.encode_single(frames_dir, output_mp4, fps, scale_filter);
        }

        let num_chunks = 4;
        let chunk_size = (total_frames + num_chunks - 1) / num_chunks;
        let temp_dir = frames_dir.join("temp_ts_chunks");
        fs::create_dir_all(&temp_dir)?;

        let mut handles = Vec::new();

        for chunk_idx in 0..num_chunks {
            let start_idx = chunk_idx * chunk_size + 1;
            let end_idx = std::cmp::min((chunk_idx + 1) * chunk_size, total_frames);

            if start_idx > total_frames {
                break;
            }

            let chunk_frames_dir = temp_dir.join(format!("chunk_{}", chunk_idx));
            fs::create_dir_all(&chunk_frames_dir)?;

            for (new_idx, orig_idx) in (start_idx..=end_idx).enumerate() {
                let orig_filename = format!("frame_{:06}.jpg", orig_idx);
                let new_filename = format!("frame_{:06}.jpg", new_idx + 1);
                let src = frames_dir.join(&orig_filename);
                let dst = chunk_frames_dir.join(&new_filename);
                if src.exists() {
                    fs::copy(&src, &dst)?;
                }
            }

            let ts_output = temp_dir.join(format!("segment_{}.ts", chunk_idx));
            let ffmpeg_path = self.ffmpeg_path.clone();
            let scale_filter = scale_filter.to_string();

            let handle = std::thread::spawn(move || -> Result<PathBuf> {
                let input_pattern = chunk_frames_dir.join("frame_%06d.jpg");
                let status = Command::new(&ffmpeg_path)
                    .arg("-y")
                    .arg("-hide_banner")
                    .arg("-loglevel")
                    .arg("error")
                    .arg("-framerate")
                    .arg(fps.to_string())
                    .arg("-start_number")
                    .arg("1")
                    .arg("-i")
                    .arg(&input_pattern)
                    .arg("-vf")
                    .arg(&scale_filter)
                    .arg("-c:v")
                    .arg("libx264")
                    .arg("-preset")
                    .arg("ultrafast")
                    .arg("-crf")
                    .arg("32")
                    .arg("-pix_fmt")
                    .arg("yuv420p")
                    .arg("-f")
                    .arg("mpegts")
                    .arg(&ts_output)
                    .status()?;

                if status.success() {
                    Ok(ts_output)
                } else {
                    Err(anyhow!("FFmpeg chunk {} falhou com status {}", chunk_idx, status))
                }
            });
            handles.push(handle);
        }

        let mut ts_files = Vec::new();
        for handle in handles {
            let ts_path = handle.join().unwrap()?;
            ts_files.push(ts_path.to_string_lossy().to_string());
        }

        let concat_input = format!("concat:{}", ts_files.join("|"));
        info!("FFmpeg: Concatenando {} segmentos .ts em MP4 final", ts_files.len());

        let concat_status = Command::new(&self.ffmpeg_path)
            .arg("-y")
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("error")
            .arg("-i")
            .arg(&concat_input)
            .arg("-c")
            .arg("copy")
            .arg("-movflags")
            .arg("+faststart")
            .arg(output_mp4)
            .status()?;

        let _ = fs::remove_dir_all(&temp_dir);

        if concat_status.success() {
            Ok(())
        } else {
            Err(anyhow!("FFmpeg concat falhou com status {}", concat_status))
        }
    }

    fn encode_single(
        &self,
        frames_dir: &Path,
        output_mp4: &Path,
        fps: u32,
        scale_filter: &str,
    ) -> Result<()> {
        let input_pattern = frames_dir.join("frame_%06d.jpg");
        let status = Command::new(&self.ffmpeg_path)
            .arg("-y")
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("error")
            .arg("-framerate")
            .arg(fps.to_string())
            .arg("-start_number")
            .arg("1")
            .arg("-i")
            .arg(&input_pattern)
            .arg("-vf")
            .arg(scale_filter)
            .arg("-c:v")
            .arg("libx264")
            .arg("-preset")
            .arg("medium")
            .arg("-crf")
            .arg("23")
            .arg("-pix_fmt")
            .arg("yuv420p")
            .arg("-movflags")
            .arg("+faststart")
            .arg(output_mp4)
            .status()?;

        if status.success() {
            Ok(())
        } else {
            Err(anyhow!("FFmpeg encodificação síncrona falhou com status {}", status))
        }
    }
}

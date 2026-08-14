use anyhow::{anyhow, Result};
use ssh2::Session;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

use crate::config::Config;

#[derive(Clone)]
pub struct SftpDownloader {
    config: Arc<Config>,
}

impl SftpDownloader {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }

    fn connect(&self) -> Result<(Session, ssh2::Sftp)> {
        let max_retries = 3;
        let mut attempt = 0;

        loop {
            attempt += 1;
            let addr = format!("{}:{}", self.config.sftp_host, self.config.sftp_port);
            match TcpStream::connect(&addr) {
                Ok(tcp) => {
                    tcp.set_nodelay(true)?;
                    let mut sess = Session::new()?;
                    sess.set_tcp_stream(tcp);
                    sess.set_timeout(10_000);

                    if let Err(e) = sess.handshake() {
                        if attempt < max_retries {
                            warn!("Handshake SSH falhou (tentativa {}/{}): {}", attempt, max_retries, e);
                            std::thread::sleep(Duration::from_millis(500 * attempt as u64));
                            continue;
                        }
                        return Err(anyhow!("Handshake SSH falhou após retries: {}", e));
                    }

                    sess.userauth_password(&self.config.sftp_username, &self.config.sftp_password)?;
                    let sftp = sess.sftp()?;
                    return Ok((sess, sftp));
                }
                Err(e) => {
                    if attempt < max_retries {
                        warn!("Falha ao conectar via TCP a {} (tentativa {}/{}): {}", addr, attempt, max_retries, e);
                        std::thread::sleep(Duration::from_millis(500 * attempt as u64));
                        continue;
                    }
                    return Err(anyhow!("Não foi possível conectar ao SFTP em {}: {}", addr, e));
                }
            }
        }
    }

    pub async fn download_frames(
        &self,
        session_folder: &str,
        output_dir: &Path,
        limit: Option<usize>,
    ) -> Result<usize> {
        fs::create_dir_all(output_dir)?;

        let config = self.config.clone();
        let session_folder = session_folder.to_string();
        let output_dir_buf = output_dir.to_path_buf();

        tokio::task::spawn_blocking(move || {
            let downloader = SftpDownloader::new(config);
            downloader.download_sync(&session_folder, &output_dir_buf, limit)
        })
        .await?
    }

    fn download_sync(
        &self,
        session_folder: &str,
        output_dir: &Path,
        limit: Option<usize>,
    ) -> Result<usize> {
        let (_sess, sftp) = self.connect()?;
        let remote_path = format!("{}/{}", self.config.sftp_root.trim_end_matches('/'), session_folder);
        let path = Path::new(&remote_path);

        let entries = sftp.readdir(path)?;
        let mut image_files: Vec<String> = entries
            .into_iter()
            .map(|(p, _)| p.file_name().unwrap_or_default().to_string_lossy().to_string())
            .filter(|name| name.to_lowercase().ends_with(".jpg") || name.to_lowercase().ends_with(".jpeg"))
            .collect();

        image_files.sort();

        if let Some(lim) = limit {
            image_files.truncate(lim);
        }

        let total_files = image_files.len();
        if total_files == 0 {
            return Err(anyhow!("Nenhuma imagem encontrada na pasta remota: {}", remote_path));
        }

        info!("SFTP: Baixando {} imagens de '{}'", total_files, remote_path);

        let num_threads = std::cmp::min(16, total_files);
        let chunk_size = (total_files + num_threads - 1) / num_threads;

        let mut handles = Vec::new();
        for (thread_idx, chunk) in image_files.chunks(chunk_size).enumerate() {
            let chunk_files = chunk.to_vec();
            let downloader = self.clone();
            let remote_dir = remote_path.clone();
            let out_dir = output_dir.to_path_buf();

            let handle = std::thread::spawn(move || -> Result<()> {
                let (_sess, thread_sftp) = downloader.connect()?;
                let mut buffer = vec![0u8; 524_288]; // 512 KB socket buffer

                for (idx, filename) in chunk_files.iter().enumerate() {
                    let global_idx = thread_idx * chunk_size + idx + 1;
                    let file_remote_path = format!("{}/{}", remote_dir, filename);
                    let local_filename = format!("frame_{:06}.jpg", global_idx);
                    let local_file_path = out_dir.join(&local_filename);

                    let mut remote_file = thread_sftp.open(Path::new(&file_remote_path))?;
                    let mut local_file = File::create(&local_file_path)?;

                    loop {
                        let n = remote_file.read(&mut buffer)?;
                        if n == 0 {
                            break;
                        }
                        local_file.write_all(&buffer[..n])?;
                    }
                }
                Ok(())
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap()?;
        }

        Ok(total_files)
    }
}

use anyhow::{anyhow, Result};
use ssh2::Session;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tracing::{info, warn};

use crate::config::Config;

#[derive(Debug, Clone)]
pub struct RemoteFileJob {
    pub index: usize,
    pub remote_path: String,
    pub size: u64,
}

#[derive(Clone)]
pub struct SftpDownloader {
    config: Arc<Config>,
}

impl SftpDownloader {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }

    pub fn connect(&self) -> Result<(Session, ssh2::Sftp)> {
        let max_retries = 5;
        let mut attempt = 0;

        loop {
            attempt += 1;
            let addr = format!("{}:{}", self.config.sftp_host, self.config.sftp_port);
            match TcpStream::connect(&addr) {
                Ok(tcp) => {
                    let _ = tcp.set_nodelay(true);
                    let mut sess = Session::new()?;
                    sess.set_tcp_stream(tcp);
                    sess.set_timeout(30_000);

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

    pub fn discover_files_recursive(
        &self,
        sftp: &ssh2::Sftp,
        dir_path: &str,
        depth: usize,
    ) -> Result<Vec<(String, u64)>> {
        let mut results = Vec::new();
        if depth > 5 {
            return Ok(results);
        }

        let path = Path::new(dir_path);
        let entries = sftp.readdir(path)?;

        for (filename_path, stat) in entries {
            let full_str = filename_path.to_str().unwrap_or("").to_string();

            if stat.is_dir() {
                let mut sub = self.discover_files_recursive(sftp, &full_str, depth + 1)?;
                results.append(&mut sub);
            } else if stat.is_file() {
                let ext = filename_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                if ext == "jpg" || ext == "jpeg" {
                    results.push((full_str, stat.size.unwrap_or(0)));
                }
            }
        }

        Ok(results)
    }

    pub async fn download_frames(
        &self,
        session_folder: &str,
        output_dir: &Path,
        limit: Option<usize>,
    ) -> Result<usize> {
        fs::create_dir_all(output_dir)?;

        let (_sess, sftp) = self.connect()?;
        let is_all = session_folder.eq_ignore_ascii_case("all")
            || session_folder.eq_ignore_ascii_case("*")
            || session_folder.eq_ignore_ascii_case("full_period")
            || session_folder.contains('-') && session_folder != "2026_07_28-2026_07_28" && session_folder != "2026_07_29-2026_07_29" && session_folder != "2026_07_30-2026_07_30" && session_folder != "2026_07_31-2026_07_31" && session_folder != "2026_08_01-2026_08_01" && session_folder != "2026_08_02-2026_08_02" && session_folder != "2026_08_09-2026_09_07";

        let mut discovered_files: Vec<(String, u64)> = Vec::new();

        if is_all || session_folder == "2026_07_28-2026_09_07" {
            let root_path = self.config.sftp_root.clone();
            discovered_files = self.discover_files_recursive(&sftp, &root_path, 0)?;
        } else {
            let remote_path = format!("{}/{}", self.config.sftp_root.trim_end_matches('/'), session_folder);
            discovered_files = self.discover_files_recursive(&sftp, &remote_path, 0)?;
        }

        discovered_files.sort_by(|a, b| a.0.cmp(&b.0));

        if let Some(lim) = limit {
            discovered_files.truncate(lim);
        }

        let total_files = discovered_files.len();
        if total_files == 0 {
            return Err(anyhow!("Nenhuma imagem encontrada para a sessão: {}", session_folder));
        }

        info!("SFTP: Baixando TOTAL de {} imagens via Tokio MPSC Work-Stealing Workers", total_files);

        let total_bytes = Arc::new(AtomicU64::new(0));
        let (tx, rx) = mpsc::channel::<RemoteFileJob>(total_files);
        let rx = Arc::new(Mutex::new(rx));

        for (index, (remote_path, size)) in discovered_files.into_iter().enumerate() {
            tx.send(RemoteFileJob {
                index,
                remote_path,
                size,
            })
            .await?;
        }
        drop(tx);

        let concurrency = std::cmp::min(16, total_files);
        let mut handles = Vec::new();

        for worker_id in 0..concurrency {
            let rx_clone = Arc::clone(&rx);
            let downloader_clone = self.clone();
            let out_dir_clone = output_dir.to_path_buf();
            let total_bytes_clone = Arc::clone(&total_bytes);

            let handle = tokio::spawn(async move {
                let (_sess, thread_sftp) = match downloader_clone.connect() {
                    Ok(c) => c,
                    Err(e) => {
                        warn!("Worker SFTP {} falhou ao conectar: {:?}", worker_id, e);
                        return;
                    }
                };

                let mut buffer = vec![0u8; 512 * 1024]; // 512KB windowed socket buffer

                loop {
                    let job = {
                        let mut lock = rx_clone.lock().await;
                        lock.recv().await
                    };

                    let job = match job {
                        Some(j) => j,
                        None => break,
                    };

                    let local_filename = format!("frame_{:06}.jpg", job.index + 1);
                    let local_path = out_dir_clone.join(&local_filename);

                    // Reutilização de arquivos pré-existentes caso o tamanho bata
                    if let Ok(meta) = fs::metadata(&local_path) {
                        if meta.len() > 0 && meta.len() == job.size {
                            total_bytes_clone.fetch_add(meta.len(), Ordering::SeqCst);
                            continue;
                        }
                    }

                    if let Ok(mut remote_file) = thread_sftp.open(Path::new(&job.remote_path)) {
                        if let Ok(mut local_file) = File::create(&local_path) {
                            let mut written = 0u64;
                            loop {
                                match remote_file.read(&mut buffer) {
                                    Ok(0) => break,
                                    Ok(n) => {
                                        let _ = local_file.write_all(&buffer[..n]);
                                        written += n as u64;
                                    }
                                    Err(_) => break,
                                }
                            }
                            total_bytes_clone.fetch_add(written, Ordering::SeqCst);
                        }
                    }
                }
            });

            handles.push(handle);
        }

        for handle in handles {
            let _ = handle.await;
        }

        let downloaded_mb = total_bytes.load(Ordering::SeqCst) as f64 / (1024.0 * 1024.0);
        info!("SFTP: Download concluído! {} fotos ({:.2} MB)", total_files, downloaded_mb);

        Ok(total_files)
    }
}

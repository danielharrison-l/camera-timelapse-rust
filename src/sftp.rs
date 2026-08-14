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
        let is_all = session_folder.eq_ignore_ascii_case("all")
            || session_folder.eq_ignore_ascii_case("*")
            || session_folder.eq_ignore_ascii_case("full_period")
            || session_folder.contains('-') && session_folder != "2026_07_28-2026_07_28" && session_folder != "2026_07_29-2026_07_29" && session_folder != "2026_07_30-2026_07_30" && session_folder != "2026_07_31-2026_07_31" && session_folder != "2026_08_01-2026_08_01" && session_folder != "2026_08_02-2026_08_02" && session_folder != "2026_08_09-2026_09_07";

        let mut image_remote_paths: Vec<String> = Vec::new();

        if is_all || session_folder == "2026_07_28-2026_09_07" {
            let root_path = Path::new(&self.config.sftp_root);
            let root_entries = sftp.readdir(root_path)?;

            let mut dir_names: Vec<String> = root_entries
                .into_iter()
                .filter_map(|(p, stat)| {
                    if stat.is_dir() {
                        Some(p.file_name()?.to_string_lossy().to_string())
                    } else {
                        None
                    }
                })
                .collect();
            dir_names.sort();

            for dir_name in &dir_names {
                let dir_path_str = format!("{}/{}", self.config.sftp_root.trim_end_matches('/'), dir_name);
                let dir_path = Path::new(&dir_path_str);
                if let Ok(entries) = sftp.readdir(dir_path) {
                    let mut sub_files: Vec<String> = entries
                        .into_iter()
                        .map(|(p, _)| format!("{}/{}", dir_path_str, p.file_name().unwrap_or_default().to_string_lossy()))
                        .filter(|name| name.to_lowercase().ends_with(".jpg") || name.to_lowercase().ends_with(".jpeg"))
                        .collect();
                    sub_files.sort();
                    image_remote_paths.extend(sub_files);
                }
            }
        } else {
            let remote_path = format!("{}/{}", self.config.sftp_root.trim_end_matches('/'), session_folder);
            let path = Path::new(&remote_path);

            if let Ok(entries) = sftp.readdir(path) {
                let mut files: Vec<String> = entries
                    .into_iter()
                    .map(|(p, _)| format!("{}/{}", remote_path, p.file_name().unwrap_or_default().to_string_lossy()))
                    .filter(|name| name.to_lowercase().ends_with(".jpg") || name.to_lowercase().ends_with(".jpeg"))
                    .collect();
                files.sort();

                // Se não encontrou imagens diretas mas a pasta tem subpastas, varre as subpastas
                if files.is_empty() {
                    let root_entries = sftp.readdir(path)?;
                    for (p, stat) in root_entries {
                        if stat.is_dir() {
                            let sub_dir = format!("{}/{}", remote_path, p.file_name().unwrap_or_default().to_string_lossy());
                            if let Ok(sub_entries) = sftp.readdir(Path::new(&sub_dir)) {
                                let mut sub_files: Vec<String> = sub_entries
                                    .into_iter()
                                    .map(|(sp, _)| format!("{}/{}", sub_dir, sp.file_name().unwrap_or_default().to_string_lossy()))
                                    .filter(|name| name.to_lowercase().ends_with(".jpg") || name.to_lowercase().ends_with(".jpeg"))
                                    .collect();
                                sub_files.sort();
                                files.extend(sub_files);
                            }
                        }
                    }
                }
                image_remote_paths = files;
            }
        }

        if let Some(lim) = limit {
            image_remote_paths.truncate(lim);
        }

        let total_files = image_remote_paths.len();
        if total_files == 0 {
            return Err(anyhow!("Nenhuma imagem encontrada para a sessão: {}", session_folder));
        }

        info!("SFTP: Baixando TOTAL de {} imagens do servidor SFTP", total_files);

        let num_threads = std::cmp::min(16, total_files);
        let chunk_size = (total_files + num_threads - 1) / num_threads;

        let mut handles = Vec::new();
        for (thread_idx, chunk) in image_remote_paths.chunks(chunk_size).enumerate() {
            let chunk_paths = chunk.to_vec();
            let downloader = self.clone();
            let out_dir = output_dir.to_path_buf();

            let handle = std::thread::spawn(move || -> Result<()> {
                let (_sess, thread_sftp) = downloader.connect()?;
                let mut buffer = vec![0u8; 524_288]; // 512 KB socket buffer

                for (idx, file_remote_path) in chunk_paths.iter().enumerate() {
                    let global_idx = thread_idx * chunk_size + idx + 1;
                    let local_filename = format!("frame_{:06}.jpg", global_idx);
                    let local_file_path = out_dir.join(&local_filename);

                    let mut remote_file = thread_sftp.open(Path::new(file_remote_path))?;
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

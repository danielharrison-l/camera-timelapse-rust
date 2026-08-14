use anyhow::{anyhow, Result};
use aws_config::BehaviorVersion;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client as S3Client;
use std::path::Path;
use std::sync::Arc;
use tracing::info;

use crate::config::Config;

#[derive(Clone)]
pub struct S3Storage {
    client: S3Client,
    config: Arc<Config>,
}

impl S3Storage {
    pub async fn new(config: Arc<Config>) -> Result<Self> {
        let mut aws_config_builder = aws_config::defaults(BehaviorVersion::latest())
            .region(aws_config::Region::new(config.aws_region.clone()));

        if let Some(ref endpoint) = config.aws_endpoint_url {
            aws_config_builder = aws_config_builder.endpoint_url(endpoint);
        }

        let aws_config = aws_config_builder.load().await;
        let s3_config = aws_sdk_s3::config::Builder::from(&aws_config)
            .force_path_style(true)
            .build();

        let client = S3Client::from_conf(s3_config);

        Ok(Self { client, config })
    }

    pub async fn upload_file(
        &self,
        file_path: &Path,
        bucket_override: Option<&str>,
        key_override: Option<&str>,
    ) -> Result<(String, String, String, u64)> {
        let file_name = file_path.file_name().unwrap_or_default().to_string_lossy();
        let bucket = bucket_override
            .unwrap_or(&self.config.s3_default_bucket)
            .to_string();

        let key = key_override
            .map(|k| k.to_string())
            .unwrap_or_else(|| format!("{}{}", self.config.s3_default_prefix, file_name));

        let metadata = tokio::fs::metadata(file_path).await?;
        let file_size = metadata.len();

        info!("S3: Fazendo upload de '{}' ({} MB) para s3://{}/{}", file_path.display(), file_size / (1024 * 1024), bucket, key);

        let body = ByteStream::from_path(file_path).await?;

        self.client
            .put_object()
            .bucket(&bucket)
            .key(&key)
            .body(body)
            .content_type("video/mp4")
            .send()
            .await
            .map_err(|e| anyhow!("Upload S3 falhou: {}", e))?;

        let url = match &self.config.aws_endpoint_url {
            Some(endpoint) => format!("{}/{}/{}", endpoint.trim_end_matches('/'), bucket, key),
            None => format!("https://{}.s3.{}.amazonaws.com/{}", bucket, self.config.aws_region, key),
        };

        Ok((bucket, key, url, file_size))
    }
}

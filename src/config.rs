use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub sftp_host: String,
    pub sftp_port: u16,
    pub sftp_username: String,
    pub sftp_password: String,
    pub sftp_root: String,

    pub sqs_inbound_queue_url: String,
    pub sqs_outbound_queue_url: String,
    pub sqs_visibility_timeout: i32,
    pub sqs_wait_time_seconds: i32,

    pub s3_default_bucket: String,
    pub s3_default_prefix: String,

    pub aws_region: String,
    pub aws_endpoint_url: Option<String>,

    pub ffmpeg_path: String,
    pub worker_concurrency: usize,
}

impl Config {
    pub fn from_env() -> Self {
        dotenvy::dotenv().ok();

        Self {
            sftp_host: env::var("SFTP_HOST").unwrap_or_else(|_| "sftpgo.dev.vision360.app.br".to_string()),
            sftp_port: env::var("SFTP_PORT")
                .unwrap_or_else(|_| "22".to_string())
                .parse()
                .unwrap_or(22),
            sftp_username: env::var("SFTP_USERNAME").unwrap_or_else(|_| "camera_garagem".to_string()),
            sftp_password: env::var("SFTP_PASSWORD").unwrap_or_else(|_| "senhaGaragem456!".to_string()),
            sftp_root: env::var("SFTP_ROOT").unwrap_or_else(|_| "/".to_string()),

            sqs_inbound_queue_url: env::var("SQS_INBOUND_QUEUE_URL").unwrap_or_else(|_| {
                "http://sqs.us-east-1.localhost.localstack.cloud:4566/000000000000/camera-timelapse-inbound".to_string()
            }),
            sqs_outbound_queue_url: env::var("SQS_OUTBOUND_QUEUE_URL").unwrap_or_else(|_| {
                "http://sqs.us-east-1.localhost.localstack.cloud:4566/000000000000/camera-timelapse-outbound".to_string()
            }),
            sqs_visibility_timeout: env::var("SQS_VISIBILITY_TIMEOUT")
                .unwrap_or_else(|_| "1800".to_string())
                .parse()
                .unwrap_or(1800),
            sqs_wait_time_seconds: env::var("SQS_WAIT_TIME_SECONDS")
                .unwrap_or_else(|_| "10".to_string())
                .parse()
                .unwrap_or(10),

            s3_default_bucket: env::var("S3_DEFAULT_BUCKET").unwrap_or_else(|_| "cameras-videos-bucket".to_string()),
            s3_default_prefix: env::var("S3_DEFAULT_PREFIX").unwrap_or_else(|_| "videos/".to_string()),

            aws_region: env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string()),
            aws_endpoint_url: env::var("AWS_ENDPOINT_URL").ok().filter(|s| !s.is_empty()),

            ffmpeg_path: env::var("FFMPEG_PATH").unwrap_or_else(|_| "ffmpeg".to_string()),
            worker_concurrency: env::var("WORKER_CONCURRENCY")
                .unwrap_or_else(|_| "1".to_string())
                .parse()
                .unwrap_or(1),
        }
    }
}

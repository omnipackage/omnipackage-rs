use crate::config::S3Config;
use crate::logger::Logger;
use anyhow::{Context, Result};
use aws_sdk_s3::config::retry::RetryConfig;
use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region, RequestChecksumCalculation, ResponseChecksumValidation, StalledStreamProtectionConfig};
use aws_sdk_s3::error::{DisplayErrorContext, SdkError};
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::{Client, Config};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

mod probe;

const BODY_READ_ATTEMPTS: u32 = 5;
const STALL_GRACE_PERIOD: Duration = Duration::from_secs(30);

pub struct S3 {
    client: Client,
    bucket: String,
    path: String,
    supports_if_match: OnceLock<bool>,
}

#[derive(Debug)]
pub enum UploadError {
    PreconditionFailed,
    Other(anyhow::Error),
}

impl std::fmt::Display for UploadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UploadError::PreconditionFailed => write!(f, "S3 precondition failed (412)"),
            UploadError::Other(e) => write!(f, "{:#}", e),
        }
    }
}

impl std::error::Error for UploadError {}

impl From<anyhow::Error> for UploadError {
    fn from(e: anyhow::Error) -> Self {
        UploadError::Other(e)
    }
}

fn block<F: std::future::Future>(f: F) -> F::Output {
    static RT: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    RT.get_or_init(|| tokio::runtime::Builder::new_current_thread().enable_all().build().expect("cannot create tokio runtime"))
        .block_on(f)
}

fn build_client(config: &S3Config) -> Client {
    let credentials = Credentials::new(&config.access_key_id, &config.secret_access_key, None, None, "static");

    let s3_config = Config::builder()
        .endpoint_url(&config.endpoint)
        .credentials_provider(credentials)
        .region(Region::new(config.region.as_deref().unwrap_or("auto").to_string()))
        .force_path_style(config.force_path_style)
        .request_checksum_calculation(RequestChecksumCalculation::WhenRequired)
        .response_checksum_validation(ResponseChecksumValidation::WhenRequired)
        .behavior_version(BehaviorVersion::latest())
        .retry_config(RetryConfig::adaptive().with_max_attempts(10))
        .stalled_stream_protection(StalledStreamProtectionConfig::enabled().grace_period(STALL_GRACE_PERIOD).build())
        .build();

    Client::from_conf(s3_config)
}

impl S3 {
    pub fn new(config: &S3Config, path: impl Into<String>) -> Self {
        Self {
            client: build_client(config),
            bucket: config.bucket.clone(),
            path: path.into(),
            supports_if_match: OnceLock::new(),
        }
    }

    pub fn supports_if_match(&self) -> bool {
        if let Some(v) = self.supports_if_match.get() {
            return *v;
        }
        let detected = block(probe::detect(&self.client, &self.bucket, &self.path));
        *self.supports_if_match.get_or_init(|| detected)
    }

    pub fn bucket_exists(&self) -> Result<bool, anyhow::Error> {
        block(async {
            match self.client.head_bucket().bucket(&self.bucket).send().await {
                Ok(_) => Ok(true),
                Err(e) => {
                    let ee = e.into_service_error();
                    if ee.is_not_found() {
                        Ok(false)
                    } else {
                        Err(anyhow::anyhow!("error checking bucket '{}': {}", self.bucket, DisplayErrorContext(&ee)))
                    }
                }
            }
        })
    }

    pub fn download_all(&self, to: &Path) -> Result<(), anyhow::Error> {
        block(async {
            let objects = self.list_objects().await?;

            for key in objects {
                let relative = key.strip_prefix(&self.path).unwrap_or(&key).trim_start_matches('/');
                let local_path = to.join(relative);

                if let Some(parent) = local_path.parent() {
                    std::fs::create_dir_all(parent).with_context(|| format!("cannot create dir {}", parent.display()))?;
                }

                let (bytes, _) = self.get_object_body(&key).await?.ok_or_else(|| anyhow::anyhow!("cannot download {}: object is gone", key))?;

                std::fs::write(&local_path, bytes).with_context(|| format!("cannot write {}", local_path.display()))?;
            }

            Ok(())
        })
    }

    pub fn upload_all(&self, from: &Path, skip: &HashSet<PathBuf>) -> Result<(), anyhow::Error> {
        block(async {
            let pattern = format!("{}/**/*", from.to_string_lossy());
            let entries = glob::glob(&pattern).with_context(|| format!("invalid glob pattern: {}", pattern))?;

            for entry in entries {
                let path = entry.context("glob error")?;
                if !path.is_file() {
                    continue;
                }
                if skip.contains(&path) {
                    continue;
                }

                let relative = path.strip_prefix(from).context("cannot strip prefix")?;
                let key = format!("{}/{}", self.path.trim_end_matches('/'), relative.to_string_lossy());

                let body = ByteStream::from_path(&path).await.with_context(|| format!("cannot read {}", path.display()))?;

                self.client
                    .put_object()
                    .bucket(&self.bucket)
                    .key(&key)
                    .body(body)
                    .send()
                    .await
                    .with_context(|| format!("cannot upload {}", key))?;
            }

            Ok(())
        })
    }

    pub fn delete_deleted_files(&self, from: &Path) -> Result<(), anyhow::Error> {
        block(async {
            let objects = self.list_objects().await?;

            for key in objects {
                let relative = key.strip_prefix(&self.path).unwrap_or(&key).trim_start_matches('/');
                let local_path = from.join(relative);

                if !local_path.exists() {
                    self.client
                        .delete_object()
                        .bucket(&self.bucket)
                        .key(&key)
                        .send()
                        .await
                        .with_context(|| format!("cannot delete {}", key))?;
                }
            }

            Ok(())
        })
    }

    pub fn download_file(&self, key: &str) -> Result<Vec<u8>, anyhow::Error> {
        block(async {
            let full_key = format!("{}/{}", self.path.trim_end_matches('/'), key.trim_start_matches('/'));

            let (bytes, _) = self.get_object_body(&full_key).await?.ok_or_else(|| anyhow::anyhow!("cannot download {}: not found", full_key))?;
            Ok(bytes)
        })
    }

    pub fn upload_file(&self, key: &str, data: Vec<u8>, content_type: Option<&str>) -> Result<(), anyhow::Error> {
        block(async {
            let full_key = format!("{}/{}", self.path.trim_end_matches('/'), key.trim_start_matches('/'));

            let body = ByteStream::from(data);
            let mut req = self.client.put_object().bucket(&self.bucket).key(&full_key).body(body);
            if let Some(ct) = content_type {
                req = req.content_type(ct);
            }
            req.send().await.with_context(|| format!("cannot upload {}", full_key))?;
            Ok(())
        })
    }

    pub fn download_file_with_etag(&self, key: &str) -> Result<(Vec<u8>, Option<String>), anyhow::Error> {
        block(async {
            let full_key = format!("{}/{}", self.path.trim_end_matches('/'), key.trim_start_matches('/'));

            Ok(self.get_object_body(&full_key).await?.unwrap_or((vec![], None)))
        })
    }

    pub fn upload_file_if_match(&self, key: &str, data: Vec<u8>, content_type: Option<&str>, etag: Option<String>) -> Result<(), UploadError> {
        block(async {
            let full_key = format!("{}/{}", self.path.trim_end_matches('/'), key.trim_start_matches('/'));

            let body = ByteStream::from(data);
            let mut req = self.client.put_object().bucket(&self.bucket).key(&full_key).body(body);
            if let Some(ct) = content_type {
                req = req.content_type(ct);
            }
            req = match &etag {
                Some(e) => req.if_match(e.clone()),
                None => req.if_none_match("*"),
            };

            match req.send().await {
                Ok(_) => Ok(()),
                Err(e) => {
                    if matches!(http_status_from_put_err(&e), Some(412)) {
                        Err(UploadError::PreconditionFailed)
                    } else {
                        Err(UploadError::Other(anyhow::anyhow!("cannot upload {}: {}", full_key, DisplayErrorContext(&e))))
                    }
                }
            }
        })
    }

    async fn get_object_body(&self, key: &str) -> Result<Option<(Vec<u8>, Option<String>)>, anyhow::Error> {
        let mut attempt: u32 = 1;

        loop {
            let response = match self.client.get_object().bucket(&self.bucket).key(key).send().await {
                Ok(r) => r,
                Err(e) => {
                    if matches!(http_status_from_get_err(&e), Some(404)) {
                        return Ok(None);
                    }
                    if let SdkError::ServiceError(svc) = &e
                        && svc.err().is_no_such_key()
                    {
                        return Ok(None);
                    }
                    return Err(anyhow::anyhow!("cannot download {}: {}", key, DisplayErrorContext(&e)));
                }
            };

            let etag = response.e_tag().map(|s| s.to_string());

            match response.body.collect().await {
                Ok(body) => return Ok(Some((body.into_bytes().to_vec(), etag))),
                Err(e) if attempt < BODY_READ_ATTEMPTS => {
                    Logger::new().warn(format!("interrupted read of {} ({}), retrying {}/{}", key, DisplayErrorContext(&e), attempt + 1, BODY_READ_ATTEMPTS));
                    tokio::time::sleep(Duration::from_secs(1u64 << (attempt - 1))).await;
                    attempt += 1;
                }
                Err(e) => return Err(anyhow::Error::new(e).context(format!("cannot read body of {}", key))),
            }
        }
    }

    async fn list_objects(&self) -> Result<Vec<String>, anyhow::Error> {
        let mut keys = vec![];
        let mut continuation_token: Option<String> = None;

        loop {
            let mut req = self.client.list_objects_v2().bucket(&self.bucket).prefix(&self.path);

            if let Some(token) = continuation_token {
                req = req.continuation_token(token);
            }

            let response = req.send().await.context("cannot list objects")?;

            for obj in response.contents() {
                if let Some(key) = obj.key() {
                    keys.push(key.to_string());
                }
            }

            if response.is_truncated().unwrap_or(false) {
                continuation_token = response.next_continuation_token().map(|s| s.to_string());
            } else {
                break;
            }
        }

        Ok(keys)
    }
}

fn http_status_from_put_err(e: &SdkError<aws_sdk_s3::operation::put_object::PutObjectError>) -> Option<u16> {
    if let SdkError::ServiceError(svc) = e {
        return Some(svc.raw().status().as_u16());
    }
    None
}

fn http_status_from_get_err(e: &SdkError<aws_sdk_s3::operation::get_object::GetObjectError>) -> Option<u16> {
    if let SdkError::ServiceError(svc) = e {
        return Some(svc.raw().status().as_u16());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    fn serve(listener: TcpListener, responses: Vec<&'static [u8]>) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap() > 2 {
                    line.clear();
                }
                stream.write_all(response).unwrap();
                stream.flush().unwrap();
            }
        })
    }

    fn config(port: u16) -> S3Config {
        S3Config {
            bucket: "bucket".to_string(),
            path_in_bucket: Some("path".to_string()),
            bucket_public_url: None,
            endpoint: format!("http://127.0.0.1:{port}"),
            access_key_id: "key".to_string(),
            secret_access_key: "secret".to_string(),
            region: Some("auto".to_string()),
            force_path_style: true,
            cloudflare_zone_id: None,
            cloudflare_api_token: None,
        }
    }

    #[test]
    fn retries_a_body_read_cut_short() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let cut_short: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\nETag: \"abc\"\r\n\r\nhello";
        let complete: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\nETag: \"abc\"\r\n\r\nhello world";
        let server = serve(listener, vec![cut_short, complete]);

        let s3 = S3::new(&config(port), "path");
        let (bytes, etag) = s3.download_file_with_etag("install.html").unwrap();

        assert_eq!(bytes, b"hello world");
        assert_eq!(etag.as_deref(), Some("\"abc\""));
        server.join().unwrap();
    }

    #[test]
    fn missing_object_reads_as_empty() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let not_found: &[u8] = b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
        let server = serve(listener, vec![not_found]);

        let s3 = S3::new(&config(port), "path");
        let (bytes, etag) = s3.download_file_with_etag("install.html").unwrap();

        assert!(bytes.is_empty());
        assert!(etag.is_none());
        server.join().unwrap();
    }
}

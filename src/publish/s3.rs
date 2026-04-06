use crate::config::S3Config;
use anyhow::{Context, Result};
use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::{Client, Config};
use std::path::Path;

pub struct S3 {
    client: Client,
    bucket: String,
    path: String,
}

fn block<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Runtime::new().expect("cannot create tokio runtime").block_on(f)
}

fn build_client(config: &S3Config) -> Client {
    let credentials = Credentials::new(&config.access_key_id, &config.secret_access_key, None, None, "static");

    let s3_config = Config::builder()
        .endpoint_url(&config.endpoint)
        .credentials_provider(credentials)
        .region(Region::new(config.region.as_deref().unwrap_or("auto").to_string()))
        .force_path_style(config.force_path_style)
        .behavior_version(BehaviorVersion::latest())
        .build();

    Client::from_conf(s3_config)
}

impl S3 {
    pub fn new(config: &S3Config, path: impl Into<String>) -> Self {
        Self {
            client: build_client(config),
            bucket: config.bucket.clone(),
            path: path.into(),
        }
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
                        Err(anyhow::anyhow!("error checking bucket '{}': {}", self.bucket, ee))
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

                let response = self
                    .client
                    .get_object()
                    .bucket(&self.bucket)
                    .key(&key)
                    .send()
                    .await
                    .with_context(|| format!("cannot download {}", key))?;

                let bytes = response.body.collect().await.with_context(|| format!("cannot read body of {}", key))?.into_bytes();

                std::fs::write(&local_path, bytes).with_context(|| format!("cannot write {}", local_path.display()))?;
            }

            Ok(())
        })
    }

    pub fn upload_all(&self, from: &Path) -> Result<(), anyhow::Error> {
        block(async {
            let pattern = format!("{}/**/*", from.to_string_lossy());
            let entries = glob::glob(&pattern).with_context(|| format!("invalid glob pattern: {}", pattern))?;

            for entry in entries {
                let path = entry.with_context(|| "glob error".to_string())?;
                if !path.is_file() {
                    continue;
                }

                let relative = path.strip_prefix(from).with_context(|| "cannot strip prefix".to_string())?;
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

            let response = self
                .client
                .get_object()
                .bucket(&self.bucket)
                .key(&full_key)
                .send()
                .await
                .with_context(|| format!("cannot download {}", full_key))?;

            let bytes = response.body.collect().await.with_context(|| format!("cannot read body of {}", full_key))?.into_bytes().to_vec();
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

    async fn list_objects(&self) -> Result<Vec<String>, anyhow::Error> {
        let mut keys = vec![];
        let mut continuation_token: Option<String> = None;

        loop {
            let mut req = self.client.list_objects_v2().bucket(&self.bucket).prefix(&self.path);

            if let Some(token) = continuation_token {
                req = req.continuation_token(token);
            }

            let response = req.send().await.with_context(|| "cannot list objects".to_string())?;

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

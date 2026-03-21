// src/publish/s3.rs

use crate::config::S3Config;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use std::path::Path;

pub struct S3 {
    client: Client,
    bucket: String,
    path: String,
}

fn block<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Runtime::new().expect("cannot create tokio runtime").block_on(f)
}

impl S3 {
    pub fn new(config: &S3Config, path: impl Into<String>) -> Self {
        let credentials = Credentials::new(&config.access_key_id, &config.secret_access_key, None, None, "static");

        let s3_config = aws_sdk_s3::Config::builder()
            .endpoint_url(&config.endpoint)
            .credentials_provider(credentials)
            .region(Region::new(config.region.as_deref().unwrap_or("auto").to_string()))
            .force_path_style(config.force_path_style)
            .behavior_version(BehaviorVersion::latest())
            .build();

        Self {
            client: Client::from_conf(s3_config),
            bucket: config.bucket.clone(),
            path: path.into(),
        }
    }

    pub fn bucket_exists(&self) -> Result<bool, String> {
        block(async {
            match self.client.head_bucket().bucket(&self.bucket).send().await {
                Ok(_) => Ok(true),
                Err(e) => {
                    if e.into_service_error().is_not_found() {
                        Ok(false)
                    } else {
                        Err(format!("error checking bucket '{}'", self.bucket))
                    }
                }
            }
        })
    }

    pub fn download_all(&self, to: &Path) -> Result<(), String> {
        block(async {
            let objects = self.list_objects().await?;

            for key in objects {
                let relative = key.strip_prefix(&self.path).unwrap_or(&key).trim_start_matches('/');
                let local_path = to.join(relative);

                if let Some(parent) = local_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| format!("cannot create dir {}: {}", parent.display(), e))?;
                }

                let response = self
                    .client
                    .get_object()
                    .bucket(&self.bucket)
                    .key(&key)
                    .send()
                    .await
                    .map_err(|e| format!("cannot download {}: {}", key, e))?;

                let bytes = response.body.collect().await.map_err(|e| format!("cannot read body of {}: {}", key, e))?.into_bytes();

                std::fs::write(&local_path, bytes).map_err(|e| format!("cannot write {}: {}", local_path.display(), e))?;
            }

            Ok(())
        })
    }

    pub fn upload_all(&self, from: &Path) -> Result<(), String> {
        block(async {
            let pattern = format!("{}/**/*", from.to_string_lossy());
            let entries = glob::glob(&pattern).map_err(|e| format!("invalid glob pattern: {}", e))?;

            for entry in entries {
                let path = entry.map_err(|e| format!("glob error: {}", e))?;
                if !path.is_file() {
                    continue;
                }

                let relative = path.strip_prefix(from).map_err(|e| format!("cannot strip prefix: {}", e))?;
                let key = format!("{}/{}", self.path.trim_end_matches('/'), relative.to_string_lossy());

                let body = aws_sdk_s3::primitives::ByteStream::from_path(&path)
                    .await
                    .map_err(|e| format!("cannot read {}: {}", path.display(), e))?;

                self.client
                    .put_object()
                    .bucket(&self.bucket)
                    .key(&key)
                    .body(body)
                    .send()
                    .await
                    .map_err(|e| format!("cannot upload {}: {}", key, e))?;
            }

            Ok(())
        })
    }

    pub fn delete_deleted_files(&self, from: &Path) -> Result<(), String> {
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
                        .map_err(|e| format!("cannot delete {}: {}", key, e))?;
                }
            }

            Ok(())
        })
    }

    async fn list_objects(&self) -> Result<Vec<String>, String> {
        let mut keys = vec![];
        let mut continuation_token: Option<String> = None;

        loop {
            let mut req = self.client.list_objects_v2().bucket(&self.bucket).prefix(&self.path);

            if let Some(token) = continuation_token {
                req = req.continuation_token(token);
            }

            let response = req.send().await.map_err(|e| format!("cannot list objects: {}", e))?;

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

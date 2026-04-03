use super::s3::{block, build_client};
use crate::config::{Repository, S3Config};
use aws_sdk_s3::{Client, primitives::ByteStream};
use std::error::Error;
use std::thread::sleep;
use std::time::{Duration, Instant};

pub struct DistributedLock {
    backend: Box<dyn LockBackend + Send + Sync>,
}

pub struct LockGuard {
    lock: DistributedLock,
}

impl LockGuard {
    pub fn new(lock: DistributedLock) -> Result<Self, Box<dyn Error>> {
        lock.acquire()?;
        Ok(Self { lock })
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = self.lock.release();
    }
}

impl DistributedLock {
    pub fn new(config: Repository, ttl: u32, lock_key: String) -> Self {
        let backend: Box<dyn LockBackend + Send + Sync> = match config.provider.as_str() {
            // TODO: implement provider-specific atomic locks, if-none-match whenever supported. only fallback to naive unreliable lock
            "s3" => Box::new(S3NaiveLock::new(config.s3().clone(), ttl, lock_key)),
            _ => todo!(),
        };

        Self { backend }
    }

    pub fn acquire(&self) -> Result<(), Box<dyn Error>> {
        self.backend.acquire()
    }

    pub fn release(&self) -> Result<(), Box<dyn Error>> {
        self.backend.release()
    }
}

trait LockBackend {
    fn acquire(&self) -> Result<(), Box<dyn Error>>;
    fn release(&self) -> Result<(), Box<dyn Error>>;
}

struct S3NaiveLock {
    config: S3Config,
    ttl: u32,
    lock_key: String,
    client: Client,
}

impl S3NaiveLock {
    pub fn new(config: S3Config, ttl: u32, lock_key: String) -> Self {
        Self {
            config: config.clone(),
            ttl,
            lock_key,
            client: build_client(&config),
        }
    }

    fn full_key(&self) -> String {
        match &self.config.path_in_bucket {
            Some(prefix) => format!("{}/{}.lock", prefix.trim_end_matches('/'), self.lock_key),
            None => self.lock_key.clone(),
        }
    }
}

impl LockBackend for S3NaiveLock {
    fn acquire(&self) -> Result<(), Box<dyn Error>> {
        let key = self.full_key();
        let bucket = self.config.bucket.clone();
        let ttl = self.ttl;

        block(async {
            let deadline = Instant::now() + Duration::from_secs(ttl as u64);

            let mut attempt: u32 = 0;

            while std::time::Instant::now() < deadline {
                // 1. check if lock exists
                let exists = self.client.head_object().bucket(&bucket).key(&key).send().await.is_ok();

                if !exists {
                    // 2. try to acquire
                    match self.client.put_object().bucket(&bucket).key(&key).body(ByteStream::from_static(b"lock")).send().await {
                        Ok(_) => return Ok(()),
                        Err(_) => {
                            // someone raced us → continue loop
                        }
                    }
                }

                // 3. sleep 2–5 seconds
                let delay_secs = 2 + (attempt % 4);
                sleep(Duration::from_secs(delay_secs as u64));
                attempt += 1;
            }

            Err::<(), Box<dyn Error>>(format!("failed to acquire lock within {}s", ttl).into())
        })
    }

    fn release(&self) -> Result<(), Box<dyn Error>> {
        let key = self.full_key();
        let bucket = self.config.bucket.clone();

        block(async {
            let _ = self.client.delete_object().bucket(&bucket).key(&key).send().await;

            Ok(())
        })
    }
}

use crate::logger::Logger;
use crate::publish::s3::{S3, UploadError};
use anyhow::Result;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_RETRY_ATTEMPTS: u32 = 10;

pub fn put_with_retry<F>(s3: &S3, key: &str, content_type: &str, mut build_body: F) -> Result<()>
where
    F: FnMut(&[u8]) -> Result<Vec<u8>>,
{
    if !s3.supports_if_match() {
        Logger::new().warn(format!(
            "S3 endpoint does not honor If-Match on PUT; {} update may race under parallel publishes (last writer wins)",
            key
        ));
        let existing = s3.download_file(key).unwrap_or_default();
        let body = build_body(&existing)?;
        s3.upload_file(key, body, Some(content_type))?;
        return Ok(());
    }

    for attempt in 0..MAX_RETRY_ATTEMPTS {
        let (existing, etag) = s3.download_file_with_etag(key)?;
        let body = build_body(&existing)?;
        match s3.upload_file_if_match(key, body, Some(content_type), etag) {
            Ok(()) => return Ok(()),
            Err(UploadError::PreconditionFailed) => {
                std::thread::sleep(retry_backoff(attempt));
                continue;
            }
            Err(UploadError::Other(e)) => return Err(e),
        }
    }

    Err(anyhow::anyhow!("failed to update {} after {} attempts due to repeated precondition failures", key, MAX_RETRY_ATTEMPTS))
}

fn retry_backoff(attempt: u32) -> Duration {
    let base_ms: u64 = 100u64 << attempt.min(10);
    let jitter_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| u64::from(d.subsec_nanos()) % 50).unwrap_or(0);
    Duration::from_millis(base_ms + jitter_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_backoff_grows_with_attempt() {
        let a0 = retry_backoff(0);
        let a3 = retry_backoff(3);
        assert!(a3 >= a0);
        assert!(a0.as_millis() >= 100 && a0.as_millis() < 200);
        assert!(a3.as_millis() >= 800 && a3.as_millis() < 900);
    }

    #[test]
    fn test_retry_backoff_clamps_high_attempts() {
        let a = retry_backoff(50);
        assert!(a.as_millis() <= 200_000);
    }
}

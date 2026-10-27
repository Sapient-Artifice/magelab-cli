//! Identity checks for advisory headless-backend process records.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{Duration, Instant};

fn require_regular_file(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => anyhow::ensure!(
            metadata.file_type().is_file(),
            "Backend process record and lock must be regular files, not symlinks: {}",
            path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn record_lock(path: &Path, timeout: Duration) -> Result<std::fs::File> {
    let mut name = path.as_os_str().to_os_string();
    name.push(".lock");
    let lock_path = std::path::PathBuf::from(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    require_regular_file(&lock_path)?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)?;
    require_regular_file(&lock_path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let opened = file.metadata()?;
        let current = std::fs::symlink_metadata(&lock_path)?;
        anyhow::ensure!(
            opened.dev() == current.dev() && opened.ino() == current.ino(),
            "Backend process record lock was replaced while opening it."
        );
    }
    let deadline = Instant::now() + timeout;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            },
            Err(std::fs::TryLockError::WouldBlock) => anyhow::bail!("Backend process record is busy; retry after the current launch or shutdown finishes."),
            Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackendPidFile {
    pub pid: u32,
    pub url: String,
    pub started_at: f64,
    #[serde(default)]
    pub instance_id: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BackendIdentity {
    pub status: String,
    pub pid: u32,
    pub started_at: f64,
    pub instance_id: String,
    pub version: String,
}

impl BackendPidFile {
    pub fn verify(&self, live: &BackendIdentity) -> Result<u32> {
        let instance = self.instance_id.as_deref().filter(|value| !value.is_empty())
            .context("Legacy PID record has no instance identity. Check the process manually before stopping it.")?;
        anyhow::ensure!(
            live.status == "ok"
                && live.pid > 1
                && live.started_at.is_finite()
                && live.started_at > 0.0
                && !live.version.is_empty(),
            "Backend health did not return a valid process identity."
        );
        anyhow::ensure!(
            instance == live.instance_id,
            "Backend instance changed; refusing to stop a different process."
        );
        if self.pid != 0 {
            anyhow::ensure!(
                self.pid == live.pid && (self.started_at - live.started_at).abs() < 0.001,
                "Backend PID or process start time changed; refusing to stop a different process."
            );
        }
        if let Some(version) = &self.version {
            anyhow::ensure!(
                version == &live.version,
                "Backend version changed; refusing to trust the stale PID record."
            );
        }
        Ok(live.pid)
    }
}

pub fn read(path: &Path) -> Result<Option<BackendPidFile>> {
    let _lock = record_lock(path, Duration::from_secs(5))?;
    read_unlocked(path)
}

fn read_unlocked(path: &Path) -> Result<Option<BackendPidFile>> {
    require_regular_file(path)?;
    match std::fs::read_to_string(path) {
        Ok(contents) => Ok(Some(
            serde_json::from_str(&contents)
                .context("Invalid backend PID record; inspect it before retrying.")?,
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub fn write_pending(path: &Path, record: &BackendPidFile) -> Result<()> {
    use std::io::Write;
    let _lock = record_lock(path, Duration::from_secs(5))?;
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path)
        .context("Another backend launch owns the PID record. Inspect it with `mage stop` before retrying.")?;
    file.write_all(serde_json::to_string_pretty(record)?.as_bytes())?;
    file.sync_all()?;
    Ok(())
}

pub fn remove_owned(path: &Path, expected: &BackendPidFile) -> Result<()> {
    let _lock = record_lock(path, Duration::from_secs(5))?;
    let Some(current) = read_unlocked(path)? else {
        return Ok(());
    };
    let same = match expected.instance_id.as_deref() {
        Some(instance) if !instance.is_empty() => current.instance_id.as_deref() == Some(instance),
        _ => current == *expected,
    };
    if same {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// Prune only a known exited process, without consulting its former URL.
/// A pending record has no actual backend PID and is never assumed dead.
pub fn prune_exited(
    path: &Path,
    record: &BackendPidFile,
    is_running: impl FnOnce(u32) -> bool,
) -> Result<bool> {
    if record.pid <= 1 || is_running(record.pid) {
        return Ok(false);
    }
    remove_owned(path, record)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending() -> BackendPidFile {
        BackendPidFile {
            pid: 0,
            url: "http://127.0.0.1:11115".into(),
            started_at: 10.0,
            instance_id: Some("owned-instance".into()),
            version: None,
        }
    }

    #[test]
    fn record_mutex_excludes_other_handles_and_survives_record_removal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("backend.pid.json");
        let held = record_lock(&path, Duration::ZERO).unwrap();
        assert!(record_lock(&path, Duration::from_millis(20)).is_err());
        drop(held);
        write_pending(&path, &pending()).unwrap();
        remove_owned(&path, &pending()).unwrap();
        assert!(dir.path().join("backend.pid.json.lock").is_file());
        assert!(record_lock(&path, Duration::ZERO).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn record_and_mutex_symlinks_are_rejected() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("backend.pid.json");
        let target = dir.path().join("other");
        std::fs::write(&target, "untouched").unwrap();
        let lock_path = dir.path().join("backend.pid.json.lock");
        symlink(&target, &lock_path).unwrap();
        assert!(write_pending(&path, &pending()).is_err());
        std::fs::remove_file(&lock_path).unwrap();
        symlink(&target, &path).unwrap();
        assert!(read(&path).is_err());
        assert!(remove_owned(&path, &pending()).is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "untouched");
    }

    fn live() -> BackendIdentity {
        BackendIdentity {
            status: "ok".into(),
            pid: 42,
            started_at: 10.25,
            instance_id: "owned-instance".into(),
            version: "0.13.2".into(),
        }
    }

    #[test]
    fn exited_record_is_pruned_without_trusting_a_replacement_at_its_url() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("backend.pid.json");
        let record = BackendPidFile {
            pid: 42,
            ..pending()
        };
        write_pending(&path, &record).unwrap();
        assert!(prune_exited(&path, &record, |pid| {
            assert_eq!(pid, 42);
            false
        })
        .unwrap());
        assert!(read(&path).unwrap().is_none());

        let replacement = BackendPidFile {
            pid: 43,
            instance_id: Some("replacement".into()),
            ..pending()
        };
        write_pending(&path, &replacement).unwrap();
        assert!(prune_exited(&path, &record, |_| false).unwrap());
        assert_eq!(read(&path).unwrap(), Some(replacement));
    }

    #[test]
    fn pending_and_live_records_are_not_pruned() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("backend.pid.json");
        let record = pending();
        write_pending(&path, &record).unwrap();
        assert!(
            !prune_exited(&path, &record, |_| panic!("pending PID must not be probed")).unwrap()
        );
        let live = BackendPidFile {
            pid: 42,
            ..record.clone()
        };
        assert!(!prune_exited(&path, &live, |_| true).unwrap());
        assert_eq!(read(&path).unwrap(), Some(record));
    }

    #[test]
    fn pending_uses_backend_pid_without_trusting_launcher_pid() {
        assert_eq!(pending().verify(&live()).unwrap(), 42);
    }

    #[test]
    fn stale_legacy_and_reused_pid_records_are_rejected() {
        let mut record = pending();
        record.instance_id = None;
        assert!(record.verify(&live()).is_err());
        record = pending();
        record.instance_id = Some("other-instance".into());
        assert!(record.verify(&live()).is_err());
        record = pending();
        record.pid = 42;
        assert!(record.verify(&live()).is_err());
        record.started_at = 10.25;
        record.version = Some("older-version".into());
        assert!(record.verify(&live()).is_err());
        record.version = Some("0.13.2".into());
        assert_eq!(record.verify(&live()).unwrap(), 42);
    }

    #[test]
    fn pending_write_and_cleanup_preserve_another_launch() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("backend.pid.json");
        let record = pending();
        write_pending(&path, &record).unwrap();
        assert!(write_pending(&path, &record).is_err());
        let other = BackendPidFile {
            instance_id: Some("new-launch".into()),
            ..record.clone()
        };
        std::fs::write(&path, serde_json::to_string(&other).unwrap()).unwrap();
        remove_owned(&path, &record).unwrap();
        assert_eq!(read(&path).unwrap(), Some(other.clone()));
        remove_owned(&path, &other).unwrap();
        assert_eq!(read(&path).unwrap(), None);
    }

    #[test]
    fn integer_legacy_timestamp_still_loads_for_dead_process_cleanup() {
        let record: BackendPidFile =
            serde_json::from_str(r#"{"pid":42,"url":"http://127.0.0.1:11115","started_at":10}"#)
                .unwrap();
        assert_eq!(record.started_at, 10.0);
        assert_eq!(record.instance_id, None);
    }
}

use anyhow::Result;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

/// SFTP file entry.
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<String>,
    pub permissions: Option<String>,
}

/// SFTP transfer progress.
#[derive(Debug, Clone)]
pub enum TransferEvent {
    Started {
        file: String,
        total: u64,
        is_upload: bool,
    },
    Progress {
        file: String,
        transferred: u64,
        total: u64,
    },
    Completed {
        file: String,
    },
    Error {
        file: String,
        error: String,
    },
}

/// Current transfer state.
#[derive(Debug, Clone)]
pub struct TransferState {
    pub file: String,
    pub transferred: u64,
    pub total: u64,
    pub is_upload: bool,
}

/// SFTP engine for remote file operations.
pub struct SftpEngine {
    sftp: Option<russh_sftp::client::SftpSession>,
    event_tx: mpsc::UnboundedSender<TransferEvent>,
    current_dir: String,
    parent_dir: Option<String>,
}

impl SftpEngine {
    pub fn new(event_tx: mpsc::UnboundedSender<TransferEvent>) -> Self {
        Self {
            sftp: None,
            event_tx,
            current_dir: "/".to_string(),
            parent_dir: None,
        }
    }

    /// Check if SFTP session is initialized.
    pub fn is_connected(&self) -> bool {
        self.sftp.is_some()
    }

    /// Initialize SFTP session over an SSH channel stream.
    pub async fn init<S>(&mut self, stream: S) -> Result<()>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let sftp = russh_sftp::client::SftpSession::new(stream).await?;
        let cwd = sftp.canonicalize(".").await.unwrap_or_else(|_| "/".to_string());
        self.current_dir = cwd.clone();
        self.parent_dir = Self::get_parent(&cwd);
        self.sftp = Some(sftp);
        Ok(())
    }

    /// Get parent directory path.
    fn get_parent(path: &str) -> Option<String> {
        let path = path.trim_end_matches('/');
        if path.is_empty() || path == "/" {
            return None;
        }
        let idx = path.rfind('/')?;
        if idx == 0 {
            Some("/".to_string())
        } else {
            Some(path[..idx].to_string())
        }
    }

    /// List files in the current directory.
    pub async fn list_current(&mut self) -> Result<Vec<FileEntry>> {
        self.list_dir(&self.current_dir.clone()).await
    }

    /// List files in the specified directory.
    pub async fn list_dir(&mut self, path: &str) -> Result<Vec<FileEntry>> {
        let sftp = self
            .sftp
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("SFTP not initialized"))?;

        let items = sftp.read_dir(path).await?;
        let path_owned = path.to_string();

        let mut entries: Vec<FileEntry> = items
            .map(|item| {
                let meta = item.metadata();
                let name = item.file_name();
                let file_path = format!("{}/{}", path_owned.trim_end_matches('/'), name);
                FileEntry {
                    path: file_path,
                    name,
                    is_dir: meta.is_dir(),
                    size: meta.size.unwrap_or(0),
                    modified: meta.mtime.map(|t| {
                        chrono::DateTime::from_timestamp(t as i64, 0)
                            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                            .unwrap_or_else(|| t.to_string())
                    }),
                    permissions: meta.permissions.map(|p| format!("{p:03o}")),
                }
            })
            .collect();

        // Sort: directories first, then by name
        entries.sort_by(|a, b| {
            if a.is_dir != b.is_dir {
                b.is_dir.cmp(&a.is_dir)
            } else {
                a.name.to_lowercase().cmp(&b.name.to_lowercase())
            }
        });

        self.current_dir = path.to_string();
        self.parent_dir = Self::get_parent(path);
        Ok(entries)
    }

    /// Navigate to parent directory.
    pub fn parent_dir(&self) -> Option<&str> {
        self.parent_dir.as_deref()
    }

    /// Get current directory.
    pub fn current_dir(&self) -> &str {
        &self.current_dir
    }

    /// Change to a subdirectory.
    pub async fn cd(&mut self, name: &str) -> Result<Vec<FileEntry>> {
        let new_path = if self.current_dir == "/" {
            format!("/{}", name.trim_start_matches('/'))
        } else {
            format!("{}/{}", self.current_dir.trim_end_matches('/'), name.trim_start_matches('/'))
        };
        self.list_dir(&new_path).await
    }

    /// Go up to parent directory.
    pub async fn cd_up(&mut self) -> Result<Vec<FileEntry>> {
        if let Some(parent) = self.parent_dir.clone() {
            self.list_dir(&parent).await
        } else {
            anyhow::bail!("Already at root directory")
        }
    }

    /// Download a remote file to a local path.
    pub async fn download(&self, remote_path: &str, local_path: &str) -> Result<()> {
        let sftp = self
            .sftp
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("SFTP not initialized"))?;

        let mut remote_file = sftp.open(remote_path).await?;
        let meta = remote_file.metadata().await?;
        let total_size = meta.size.unwrap_or(0);

        // Send started event
        self.event_tx.send(TransferEvent::Started {
            file: remote_path.to_string(),
            total: total_size,
            is_upload: false,
        })?;

        // Create parent directories if needed
        if let Some(parent) = std::path::Path::new(local_path).parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }

        let mut local_file = tokio::fs::File::create(local_path).await?;
        let mut transferred: u64 = 0;
        let mut buf = vec![0u8; 32768];

        loop {
            let n = remote_file.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            local_file.write_all(&buf[..n]).await?;
            transferred += n as u64;
            self.event_tx.send(TransferEvent::Progress {
                file: remote_path.to_string(),
                transferred,
                total: total_size,
            })?;
        }

        self.event_tx.send(TransferEvent::Completed {
            file: remote_path.to_string(),
        })?;

        Ok(())
    }

    /// Upload a local file to a remote path.
    pub async fn upload(&self, local_path: &str, remote_path: &str) -> Result<()> {
        let sftp = self
            .sftp
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("SFTP not initialized"))?;

        let mut local_file = tokio::fs::File::open(local_path).await?;
        let total_size = local_file.metadata().await?.len();

        self.event_tx.send(TransferEvent::Started {
            file: local_path.to_string(),
            total: total_size,
            is_upload: true,
        })?;

        let mut remote_file = sftp.create(remote_path).await?;
        let mut transferred: u64 = 0;
        let mut buf = vec![0u8; 32768];

        loop {
            let n = local_file.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            remote_file.write_all(&buf[..n]).await?;
            transferred += n as u64;
            self.event_tx.send(TransferEvent::Progress {
                file: local_path.to_string(),
                transferred,
                total: total_size,
            })?;
        }

        self.event_tx.send(TransferEvent::Completed {
            file: local_path.to_string(),
        })?;

        Ok(())
    }

    /// Create a directory.
    pub async fn mkdir(&self, path: &str) -> Result<()> {
        let sftp = self
            .sftp
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("SFTP not initialized"))?;
        sftp.create_dir(path).await?;
        Ok(())
    }

    /// Remove a file.
    pub async fn remove_file(&self, path: &str) -> Result<()> {
        let sftp = self
            .sftp
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("SFTP not initialized"))?;
        sftp.remove_file(path).await?;
        Ok(())
    }

    /// Remove a directory.
    pub async fn remove_dir(&self, path: &str) -> Result<()> {
        let sftp = self
            .sftp
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("SFTP not initialized"))?;
        sftp.remove_dir(path).await?;
        Ok(())
    }

    /// Rename a file or directory.
    pub async fn rename(&self, old_path: &str, new_path: &str) -> Result<()> {
        let sftp = self
            .sftp
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("SFTP not initialized"))?;
        sftp.rename(old_path, new_path).await?;
        Ok(())
    }
}

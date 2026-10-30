//! Interactive stdin reopened inside Wine, after pressure-vessel's /dev/null.
//! Only umu hook hosts need this; stdout/stderr retain their original pipes.
use std::fs::OpenOptions;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::task::{Context, Poll};

use nix::sys::stat::Mode;
use tokio::io::AsyncWrite;
use vnl_core::AppError;

pub(crate) struct FifoStdin {
    // Close the last writer before removing the directory, including on abort.
    writer: tokio::fs::File,
    _directory: tempfile::TempDir,
    command: String,
    launcher: PathBuf,
}

impl FifoStdin {
    pub fn new(prefix: &Path, executable: &Path) -> Result<Self, AppError> {
        let name =
            executable.file_name().and_then(|n| n.to_str()).ok_or_else(|| AppError::invalid("Invalid CLI name"))?;
        // cmd.exe receives only a known basename and a generated C: path, never
        // user paths or shell metacharacters. cwd remains the CLI's directory.
        if !matches!(
            name.to_ascii_lowercase().as_str(),
            "lunahostcli32.exe" | "lunahostcli64.exe" | "lunahostcli.exe" | "textractorcli.exe"
        ) {
            return Err(AppError::invalid("Unsupported interactive hook CLI name"));
        }
        let drive =
            if prefix.join("pfx/drive_c").is_dir() { prefix.join("pfx/drive_c") } else { prefix.join("drive_c") };
        let prepare = || -> std::io::Result<Self> {
            let directory = tempfile::Builder::new()
                .prefix("yomika-hook-")
                .permissions(std::fs::Permissions::from_mode(0o700))
                .tempdir_in(&drive)?;
            let path = directory.path().join("stdin.fifo");
            nix::unistd::mkfifo(&path, Mode::S_IRUSR | Mode::S_IWUSR).map_err(std::io::Error::from)?;
            // RDWR avoids a blocking open while Wine starts; NONBLOCK keeps
            // failed starts cancellable. Commands are bounded below PIPE_BUF.
            let file = OpenOptions::new().read(true).write(true).custom_flags(nix::libc::O_NONBLOCK).open(&path)?;
            let dirname = directory.path().file_name().and_then(|n| n.to_str()).expect("ASCII generated directory");
            let command = format!("{name} < C:\\{dirname}\\stdin.fifo");
            Ok(Self {
                writer: tokio::fs::File::from_std(file),
                _directory: directory,
                command,
                launcher: drive.join("windows/system32/cmd.exe"),
            })
        };
        prepare()
            .map_err(|e| AppError::internal(format!("Cannot create private hook stdin in {}: {e}", drive.display())))
    }

    pub fn launcher(&self) -> &Path {
        &self.launcher
    }

    pub fn command(&self) -> &str {
        &self.command
    }
}

impl AsyncWrite for FifoStdin {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.writer).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.writer).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.writer).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn fifo_transmits_utf16_with_closed_parent_stdin_and_delivers_eof() {
        for layout in ["drive_c", "pfx/drive_c"] {
            let root = tempfile::tempdir().unwrap();
            let prefix = root.path().join("prefix with spaces 日本語");
            std::fs::create_dir_all(prefix.join(layout)).unwrap();
            let mut input = FifoStdin::new(&prefix, Path::new("/tools with spaces/LunaHostCLI32.exe")).unwrap();
            let directory = input._directory.path().to_path_buf();
            let fifo = directory.join("stdin.fifo");
            assert!(fifo.symlink_metadata().unwrap().file_type().is_fifo());
            assert_eq!(directory.metadata().unwrap().permissions().mode() & 0o777, 0o700);
            assert_eq!(fifo.metadata().unwrap().permissions().mode() & 0o777, 0o600);
            assert!(!input.command().contains(['"', '%', '&', '|']));
            let mut child = tokio::process::Command::new("/bin/sh")
                .args(["-c", "exec cat < \"$1\"", "test"])
                .arg(&fifo)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            let mut stdout = child.stdout.take().unwrap();
            for command in ["attach -P123\n", "HS4@0:Game.exe -P123\n", "detach -P123\n"] {
                let bytes = crate::hook::protocol::encode_utf16le(command);
                input.write_all(&bytes).await.unwrap();
                input.flush().await.unwrap();
                let mut received = vec![0; bytes.len()];
                tokio::time::timeout(Duration::from_secs(2), stdout.read_exact(&mut received)).await.unwrap().unwrap();
                assert_eq!(received, bytes);
            }
            drop(input);
            assert!(!directory.exists());
            let mut rest = Vec::new();
            tokio::time::timeout(Duration::from_secs(2), stdout.read_to_end(&mut rest)).await.unwrap().unwrap();
            assert!(child.wait().await.unwrap().success());
            assert!(rest.is_empty());
        }
    }

    #[tokio::test]
    async fn aborted_start_without_reader_removes_fifo_and_directory() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("drive_c")).unwrap();
        let input = FifoStdin::new(root.path(), Path::new("TextractorCLI.exe")).unwrap();
        let directory = input._directory.path().to_path_buf();
        let (entered, ready) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let _input = input;
            entered.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        ready.await.unwrap();
        task.abort();
        let _ = task.await;
        assert!(!directory.exists());
        assert!(std::fs::read_dir(root.path().join("drive_c")).unwrap().next().is_none());
    }

    #[tokio::test]
    async fn accepts_every_existing_cli_basename() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("drive_c")).unwrap();
        for name in ["LunaHostCLI32.exe", "LunaHostCLI64.exe", "LunaHostCLI.exe", "TextractorCLI.exe"] {
            let input = FifoStdin::new(root.path(), Path::new(name)).unwrap();
            assert!(input.command().starts_with(&format!("{name} < C:")));
        }
    }

    #[test]
    fn refuses_unavailable_prefix_or_shell_metacharacters() {
        let root = tempfile::tempdir().unwrap();
        assert!(FifoStdin::new(root.path(), Path::new("LunaHostCLI32.exe")).is_err());
        std::fs::create_dir(root.path().join("drive_c")).unwrap();
        assert!(FifoStdin::new(root.path(), Path::new("LunaHostCLI32.exe & bad")).is_err());
        assert!(std::fs::read_dir(root.path().join("drive_c")).unwrap().next().is_none());
    }
}

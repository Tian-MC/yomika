//! Finding the game's **Windows** PID inside Wine.
//!
//! Wine processes have Linux PIDs (visible in `/proc`) and separate Windows
//! PIDs assigned by wineserver; hook hosts need the Windows one. We run Wine's
//! own `tasklist.exe /fo csv /nh` inside the game's prefix (same runner, same
//! wineserver, via `PrefixCommandBuilder`) and match the image name.
//! Verified on Wine 11: output lines look like
//! `"notepad.exe","356","Console","1","41,036 K"` while the same process is
//! Linux PID 879409 with `/proc/<pid>/cmdline` = `C:\windows\system32\notepad.exe`.
//! `/proc` is used only for diagnostics ("the game is running in Linux but
//! not visible to tasklist" = probably a different prefix/wineserver).

use std::path::Path;
use std::time::Duration;

use vnl_core::{AppError, CommandSpec, ErrorCode, PrefixCommandBuilder};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WinProcess {
    pub image: String,
    pub pid: u32,
}

/// Wine/Proton infrastructure processes never to be confused with a game.
const SYSTEM_IMAGES: &[&str] = &[
    "services.exe",
    "explorer.exe",
    "winedevice.exe",
    "svchost.exe",
    "plugplay.exe",
    "start.exe",
    "tasklist.exe",
    "rpcss.exe",
    "conhost.exe",
    "cmd.exe",
    "wineboot.exe",
    "rundll32.exe",
    "steam.exe",
    "tabtip.exe",
    "winemenubuilder.exe",
    "mscorsvw.exe",
    "lunahostcli32.exe",
    "lunahostcli64.exe",
    "lunahostcli.exe",
    "textractorcli.exe",
];

fn split_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => fields.push(std::mem::take(&mut cur)),
            c => cur.push(c),
        }
    }
    fields.push(cur);
    fields
}

pub fn parse_tasklist_csv(output: &str) -> Vec<WinProcess> {
    output
        .lines()
        .filter(|l| l.trim_start().starts_with('"'))
        .filter_map(|l| {
            let f = split_csv_line(l.trim());
            let pid = f.get(1)?.trim().parse().ok()?;
            Some(WinProcess { image: f.first()?.trim().to_string(), pid })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PidMatch {
    Exact(WinProcess),
    NotFound { candidates: Vec<WinProcess> },
}

pub fn find_game(processes: &[WinProcess], exe_name: &str) -> PidMatch {
    let basename = |name: &str| name.rsplit(['/', '\\']).next().unwrap_or(name).to_lowercase();
    let want = basename(exe_name);
    let matches: Vec<_> = processes.iter().filter(|p| basename(&p.image) == want).collect();
    if matches.len() == 1 {
        return PidMatch::Exact(matches[0].clone());
    }
    let candidates =
        processes.iter().filter(|p| !SYSTEM_IMAGES.contains(&p.image.to_lowercase().as_str())).cloned().collect();
    PidMatch::NotFound { candidates }
}

pub fn tasklist_command(prefix: &dyn PrefixCommandBuilder) -> Result<CommandSpec, AppError> {
    prefix.build(Path::new("tasklist.exe"), &["/fo".to_string(), "csv".to_string(), "/nh".to_string()], None)
}

// A cancelled Proton helper can leave children even when kill_on_drop kills its launcher.
struct HelperGroup(u32);
impl Drop for HelperGroup {
    fn drop(&mut self) {
        let _ = nix::sys::signal::killpg(nix::unistd::Pid::from_raw(self.0 as i32), nix::sys::signal::Signal::SIGKILL);
    }
}

fn decode_tasklist(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.iter().take(64).any(|b| *b == 0) {
        String::from_utf16_lossy(&bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect::<Vec<_>>())
            .trim_start_matches('\u{FEFF}')
            .to_string()
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// Runs `tasklist.exe` in the prefix.
pub async fn list_windows_processes(prefix: &dyn PrefixCommandBuilder) -> Result<Vec<WinProcess>, AppError> {
    let spec = tasklist_command(prefix)?;
    let mut cmd = tokio::process::Command::from(spec.to_std_command());
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .process_group(0);
    let child = cmd
        .spawn()
        .map_err(|e| AppError::new(ErrorCode::Process, format!("cannot run tasklist.exe in the prefix: {e}")))?;
    let _group = HelperGroup(child.id().expect("new child has a PID"));
    let out = tokio::time::timeout(Duration::from_secs(30), child.wait_with_output())
        .await
        .map_err(|_| AppError::new(ErrorCode::Process, "tasklist.exe timed out in the game prefix"))?
        .map_err(|e| AppError::new(ErrorCode::Process, format!("cannot run tasklist.exe in the prefix: {e}")))?;
    let text = decode_tasklist(&out.stdout);
    let procs = parse_tasklist_csv(&text);
    if procs.is_empty() && !out.status.success() {
        return Err(AppError::new(ErrorCode::Process, format!("tasklist.exe failed ({})", out.status))
            .with_details(format!("{}\n{}", spec.preview(), String::from_utf8_lossy(&out.stderr))));
    }
    Ok(procs)
}

/// Linux PIDs whose argv[0] ends with the exe name (Wine sets argv[0] to the
/// Windows path, e.g. `Z:\games\x\game.exe`). Diagnostics only.
pub fn linux_pids_for_exe(exe_name: &str) -> Vec<u32> {
    let want = exe_name.to_lowercase();
    let Ok(dir) = std::fs::read_dir("/proc") else { return Vec::new() };
    dir.filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| {
            std::fs::read(format!("/proc/{pid}/cmdline"))
                .ok()
                .and_then(|c| c.split(|b| *b == 0).next().map(|a| String::from_utf8_lossy(a).to_lowercase()))
                .is_some_and(|argv0| argv0.rsplit(['\\', '/']).next() == Some(want.as_str()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from `wine tasklist /fo csv /nh` (Wine 11) on the dev machine.
    const SAMPLE: &str = r#""services.exe","48","Console","1","12,440 K"
"explorer.exe","72","Console","1","140,356 K"
"winedevice.exe","108","Console","1","14,776 K"
"svchost.exe","164","Console","1","10,660 K"
"plugplay.exe","184","Console","1","23,024 K"
"start.exe","348","Console","1","21,312 K"
"notepad.exe","356","Console","1","41,036 K"
"tasklist.exe","372","Console","1","22,376 K"
"#;

    #[test]
    fn parses_wine_tasklist() {
        let procs = parse_tasklist_csv(&format!("ProtonFixes noise\n{SAMPLE}"));
        assert_eq!(procs.len(), 8);
        assert_eq!(procs[6], WinProcess { image: "notepad.exe".into(), pid: 356 });
    }

    #[test]
    fn matching() {
        let procs = parse_tasklist_csv(SAMPLE);
        assert_eq!(
            find_game(&procs, "NOTEPAD.EXE"),
            PidMatch::Exact(WinProcess { image: "notepad.exe".into(), pid: 356 })
        );
        assert_eq!(find_game(&procs, r"Z:\games\NOTEPAD.EXE"), find_game(&procs, "notepad.exe"));
        let mut ambiguous = procs.clone();
        ambiguous.push(WinProcess { image: "notepad.exe".into(), pid: 999 });
        assert!(matches!(find_game(&ambiguous, "notepad.exe"), PidMatch::NotFound { .. }));
        // Never inject an unrelated sole candidate when the name is mangled.
        assert!(matches!(find_game(&procs, "ゲーム.exe"), PidMatch::NotFound { .. }));
        let mut two = procs.clone();
        two.push(WinProcess { image: "other.exe".into(), pid: 400 });
        assert!(matches!(find_game(&two, "game.exe"), PidMatch::NotFound { candidates } if candidates.len() == 2));
    }

    #[test]
    fn decodes_utf16_tasklist_without_using_linux_pids() {
        let sample = "\u{FEFF}\"ゲーム.exe\",\"356\",\"Console\"\n";
        let bytes = super::super::protocol::encode_utf16le(sample);
        let procs = parse_tasklist_csv(&decode_tasklist(&bytes));
        assert!(matches!(find_game(&procs, "ゲーム.exe"), PidMatch::Exact(p) if p.pid == 356));
    }

    #[test]
    fn quoted_csv() {
        assert_eq!(split_csv_line(r#""a ""b"", c","2""#), vec![r#"a "b", c"#, "2"]);
    }
}

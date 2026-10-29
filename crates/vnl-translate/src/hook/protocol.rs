//! Text protocol of the Textractor-compatible CLI hosts
//! (`TextractorCLI.exe`, `LunaHostCLI32/64.exe` from LunaHook ≤ 3.x).
//!
//! Source: Textractor `host/CLI/main.cpp` (GPL-3.0) — LunaHostCLI kept the
//! same protocol (see the MIT `node-lunaHook` wrapper and its tests):
//! - stdin and stdout are UTF-16LE (`_setmode(..., _O_U16TEXT)`), lines end in
//!   `\n` (`\r\n` after CRT text-mode translation).
//! - On start it prints `Usage: {'attach'|'detach'|hookcode} -Pprocessid`.
//! - Commands: `attach -P<pid>`, `detach -P<pid>`, `<hookcode> -P<pid>`.
//!   **Any malformed line makes the host exit** (`ExitProcess(0)`), so
//!   commands are validated before being written.
//! - Each sentence: `[%I64X:%I32X:%I64X:%I64X:%I64X:%s:%s] %s\n` =
//!   `[handle:pid:addr:ctx:ctx2:name:hookcode] text` (hex numbers). The text
//!   may contain newlines; continuation lines have no header.
//! - Host messages (injection results, "pipe connected", errors) come from
//!   the "Console" thread (pid 0, handle 0); handle 1 / pid 0 is the host's
//!   own "Clipboard" thread and is ignored.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostSentence {
    pub handle: u64,
    pub pid: u32,
    pub addr: u64,
    pub ctx: u64,
    pub ctx2: u64,
    pub name: String,
    pub hook_code: String,
    pub text: String,
}

impl HostSentence {
    pub fn is_console(&self) -> bool {
        self.pid == 0 && self.handle == 0
    }

    pub fn is_host_clipboard(&self) -> bool {
        self.pid == 0 && self.handle != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostMessageKind {
    PipeConnected,
    AlreadyInjected,
    ArchitectureMismatch,
    InjectFailed,
    HookFailed,
    InvalidCode,
    Other,
}

/// Classifies a console message (English Textractor/LunaHook wording plus a
/// few common translations).
pub fn classify_console(msg: &str) -> HostMessageKind {
    let m = msg.to_lowercase();
    if m.contains("architecture mismatch") || m.contains("only textractor x86") || m.contains("only textractor x64") {
        HostMessageKind::ArchitectureMismatch
    } else if m.contains("couldn't inject")
        || m.contains("could not inject")
        || m.contains("inject failed")
        || m.contains("无法注入")
    {
        HostMessageKind::InjectFailed
    } else if m.contains("already injected") || m.contains("已经注入") {
        HostMessageKind::AlreadyInjected
    } else if m.contains("pipe connected") || m.contains("管道已连接") {
        HostMessageKind::PipeConnected
    } else if m.contains("failed to insert hook") || m.contains("too many hooks") {
        HostMessageKind::HookFailed
    } else if m.contains("invalid code") {
        HostMessageKind::InvalidCode
    } else {
        HostMessageKind::Other
    }
}

fn parse_hex<T: TryFrom<u64>>(s: &str) -> Option<T> {
    u64::from_str_radix(s.trim(), 16).ok().and_then(|v| T::try_from(v).ok())
}

/// Parses a header line. Returns `None` for lines that are not sentence headers.
pub fn parse_header_line(line: &str) -> Option<HostSentence> {
    let rest = line.strip_prefix('[')?;
    let mut parts = rest.splitn(6, ':');
    let handle = parse_hex::<u64>(parts.next()?)?;
    let pid = parse_hex::<u32>(parts.next()?)?;
    let addr = parse_hex::<u64>(parts.next()?)?;
    let ctx = parse_hex::<u64>(parts.next()?)?;
    let ctx2 = parse_hex::<u64>(parts.next()?)?;
    let tail = parts.next()?; // "name:hookcode] text" or "name] text"
    let close = tail.find("] ").map(|i| (i, 2)).or_else(|| tail.strip_suffix(']').map(|t| (t.len(), 1)))?;
    let (head, text) = (&tail[..close.0], &tail[close.0 + close.1..]);
    let (name, hook_code) = match head.split_once(':') {
        Some((n, c)) => (n.to_string(), c.to_string()),
        None => (head.to_string(), String::new()),
    };
    Some(HostSentence { handle, pid, addr, ctx, ctx2, name, hook_code, text: text.to_string() })
}

/// Line-oriented parser that joins continuation lines into the preceding
/// sentence. Call [`HostParser::flush`] when the stream has been idle briefly.
#[derive(Debug, Default)]
pub struct HostParser {
    pending: Option<HostSentence>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    Sentence(HostSentence),
    Usage,
    /// Output that is not part of the protocol (runner/Wine noise).
    Noise(String),
}

impl HostParser {
    pub fn feed_line(&mut self, line: &str) -> Vec<HostEvent> {
        let line = line.trim_end_matches(['\r', '\n']);
        let mut out = Vec::new();
        if let Some(s) = parse_header_line(line) {
            out.extend(self.flush());
            self.pending = Some(s);
        } else if line.starts_with("Usage:") {
            out.extend(self.flush());
            out.push(HostEvent::Usage);
        } else if let Some(p) = self.pending.as_mut() {
            p.text.push('\n');
            p.text.push_str(line);
        } else if !line.trim().is_empty() {
            out.push(HostEvent::Noise(line.to_string()));
        }
        out
    }

    pub fn flush(&mut self) -> Option<HostEvent> {
        self.pending.take().map(HostEvent::Sentence)
    }
}

/// Incremental decoder that auto-detects UTF-16LE (the CLI's encoding) vs.
/// UTF-8 (e.g. a host built differently, or runner noise before the host starts).
#[derive(Debug, Default)]
pub struct StreamDecoder {
    utf16: Option<bool>,
    bytes: Vec<u8>,
    text: String,
}

impl StreamDecoder {
    /// Feeds raw bytes; returns complete lines (without terminators).
    pub fn push(&mut self, data: &[u8]) -> Vec<String> {
        self.bytes.extend_from_slice(data);
        if self.utf16.is_none() {
            if self.bytes.len() < 4 {
                return Vec::new();
            }
            let bom16 = self.bytes.starts_with(&[0xFF, 0xFE]);
            let sample = &self.bytes[..self.bytes.len().min(64)];
            let odd_zeros = sample.iter().skip(1).step_by(2).filter(|b| **b == 0).count();
            self.utf16 = Some(bom16 || odd_zeros * 3 >= sample.len() / 2);
            if bom16 {
                self.bytes.drain(..2);
            }
        }
        if self.utf16 == Some(true) {
            let even = self.bytes.len() & !1;
            let units: Vec<u16> =
                self.bytes[..even].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
            // Keep a trailing high surrogate for the next chunk.
            let keep = matches!(units.last(), Some(u) if (0xD800..0xDC00).contains(u));
            let take = if keep { units.len() - 1 } else { units.len() };
            self.text.push_str(&String::from_utf16_lossy(&units[..take]));
            self.bytes.drain(..take * 2);
        } else {
            match std::str::from_utf8(&self.bytes) {
                Ok(s) => {
                    self.text.push_str(s);
                    self.bytes.clear();
                }
                Err(e) => {
                    let valid = e.valid_up_to();
                    self.text.push_str(std::str::from_utf8(&self.bytes[..valid]).unwrap_or_default());
                    let drop = if e.error_len().is_some() { valid + e.error_len().unwrap_or(1) } else { valid };
                    if e.error_len().is_some() {
                        self.text.push('\u{FFFD}');
                    }
                    self.bytes.drain(..drop);
                }
            }
        }
        let mut lines = Vec::new();
        while let Some(i) = self.text.find('\n') {
            let line: String = self.text.drain(..=i).collect();
            lines.push(line.trim_end_matches(['\n', '\r']).to_string());
        }
        lines
    }

    pub fn finish(&mut self) -> Option<String> {
        let rest = std::mem::take(&mut self.text);
        if rest.trim().is_empty() { None } else { Some(rest.trim_end_matches('\r').to_string()) }
    }
}

pub fn encode_utf16le(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// Validates a user hook code (H-code `/HS4@0:game.exe`, R-code `/RS@44`).
/// Must not contain whitespace (the host's `swscanf("%500s -P%d")` would
/// split it and exit).
pub fn validate_hook_code(code: &str) -> Result<String, String> {
    let code = code.trim();
    if code.is_empty() {
        return Err("hook code is empty".into());
    }
    if code.chars().any(char::is_whitespace) {
        return Err("hook codes must not contain spaces".into());
    }
    if code.chars().count() > 480 {
        return Err("hook code is too long".into());
    }
    let body = code.strip_prefix('/').unwrap_or(code);
    match body.chars().next().map(|c| c.to_ascii_uppercase()) {
        Some('H' | 'R') => Ok(code.to_string()),
        _ => Err(format!("'{code}' is not a hook code (expected /H… or /R…)")),
    }
}

/// Compares a configured hook code with the code a host reports for a thread
/// (the host appends `:module` and may drop the leading `/`).
pub fn hook_code_matches(configured: &str, reported: &str) -> bool {
    let norm = |s: &str| s.trim().trim_start_matches('/').to_ascii_uppercase();
    let (c, r) = (norm(configured), norm(reported));
    !c.is_empty() && (r == c || r.starts_with(&format!("{c}:")) || c.starts_with(&format!("{r}:")))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Sample lines from node-lunaHook's test-suite (LunaHostCLI 3.2.1 output)
    // and Textractor's CLI format string.
    const SAMPLE_SHORT: &str = "[5:6:8:7:1F:SomeName] 「とある有名お風呂」";
    const SAMPLE_FULL: &str =
        "[17:3388:67A78E:0:19F95C:System43:HSN4:-14@27A78E:Haharanman.exe] わずかな灯りに照らされた道の真ん中で、";
    const CONSOLE: &str = "[0:0:FFFFFFFFFFFFFFFF:FFFFFFFFFFFFFFFF:FFFFFFFFFFFFFFFF:Console:HB0@0] Textractor: architecture mismatch: only Textractor x86 can inject this process";

    #[test]
    fn parses_sample_lines() {
        let s = parse_header_line(SAMPLE_SHORT).unwrap();
        assert_eq!((s.handle, s.pid, s.addr, s.ctx, s.ctx2), (5, 6, 8, 7, 0x1F));
        assert_eq!(s.name, "SomeName");
        assert_eq!(s.hook_code, "");
        assert_eq!(s.text, "「とある有名お風呂」");

        let s = parse_header_line(SAMPLE_FULL).unwrap();
        assert_eq!((s.handle, s.pid, s.addr, s.ctx2), (0x17, 0x3388, 0x67A78E, 0x19F95C));
        assert_eq!(s.name, "System43");
        assert_eq!(s.hook_code, "HSN4:-14@27A78E:Haharanman.exe");
        assert!(s.text.starts_with("わずかな"));

        let c = parse_header_line(CONSOLE).unwrap();
        assert!(c.is_console());
        assert_eq!(classify_console(&c.text), HostMessageKind::ArchitectureMismatch);
        assert!(parse_header_line("Usage: {'attach'|'detach'|hookcode} -Pprocessid").is_none());
        assert!(parse_header_line("[not:a:header").is_none());
    }

    #[test]
    fn joins_continuation_lines() {
        let mut p = HostParser::default();
        assert!(p.feed_line(SAMPLE_FULL).is_empty());
        assert!(p.feed_line("わずかな灯りに照らされた道の真ん中で、").is_empty());
        assert!(p.feed_line("大柄の男が、ずぶ濡れで立っていた。\r").is_empty());
        let ev = p.feed_line(SAMPLE_SHORT);
        let HostEvent::Sentence(s) = &ev[0] else { panic!("{ev:?}") };
        assert_eq!(
            s.text,
            "わずかな灯りに照らされた道の真ん中で、\nわずかな灯りに照らされた道の真ん中で、\n大柄の男が、ずぶ濡れで立っていた。"
        );
        assert!(matches!(p.flush(), Some(HostEvent::Sentence(s)) if s.name == "SomeName"));
        assert_eq!(p.feed_line("Usage: {'attach'|'detach'|hookcode} -Pprocessid"), vec![HostEvent::Usage]);
        assert_eq!(p.feed_line("wine: noise"), vec![HostEvent::Noise("wine: noise".into())]);
    }

    #[test]
    fn decodes_utf16_in_arbitrary_chunks() {
        let text = format!("Usage: x\r\n{SAMPLE_SHORT}\r\n𠮷野家\r\n");
        let bytes = encode_utf16le(&text);
        let mut d = StreamDecoder::default();
        let mut lines = Vec::new();
        for chunk in bytes.chunks(3) {
            lines.extend(d.push(chunk));
        }
        assert_eq!(lines, vec!["Usage: x".to_string(), SAMPLE_SHORT.to_string(), "𠮷野家".to_string()]);
    }

    #[test]
    fn decodes_utf8_fallback() {
        let mut d = StreamDecoder::default();
        let bytes = "hello 世界\nnext".as_bytes();
        let mut lines = d.push(&bytes[..8]);
        lines.extend(d.push(&bytes[8..]));
        assert_eq!(lines, vec!["hello 世界"]);
        assert_eq!(d.finish().as_deref(), Some("next"));
    }

    #[test]
    fn hook_code_validation_and_matching() {
        assert!(validate_hook_code("/HS4@0:game.exe").is_ok());
        assert!(validate_hook_code("HSN4:-14@27A78E:Haharanman.exe").is_ok());
        assert!(validate_hook_code("/RS@44").is_ok());
        assert!(validate_hook_code("attach").is_err());
        assert!(validate_hook_code("/HS4@0 x").is_err());
        assert!(hook_code_matches("/HSN4:-14@27A78E", "HSN4:-14@27A78E:Haharanman.exe"));
        assert!(hook_code_matches("HSN4:-14@27A78E:Haharanman.exe", "HSN4:-14@27A78E:Haharanman.exe"));
        assert!(!hook_code_matches("/HS4@0", "HSN4:-14@27A78E:x.exe"));
    }

    #[test]
    fn console_classification() {
        assert_eq!(classify_console("Textractor: pipe connected"), HostMessageKind::PipeConnected);
        assert_eq!(classify_console("Textractor: couldn't inject"), HostMessageKind::InjectFailed);
        assert_eq!(classify_console("Textractor: already injected"), HostMessageKind::AlreadyInjected);
        assert_eq!(classify_console("Textractor: failed to insert hook"), HostMessageKind::HookFailed);
        assert_eq!(classify_console("hello"), HostMessageKind::Other);
    }
}

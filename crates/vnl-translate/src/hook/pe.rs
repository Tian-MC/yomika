//! Minimal PE header reader to detect 32/64-bit executables (the hook host
//! must match the game's architecture).

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeArch {
    X86,
    X64,
    Arm64,
    Other(u16),
}

impl PeArch {
    pub fn label(self) -> &'static str {
        match self {
            PeArch::X86 => "32-bit (x86)",
            PeArch::X64 => "64-bit (x64)",
            PeArch::Arm64 => "ARM64",
            PeArch::Other(_) => "unknown architecture",
        }
    }
}

pub fn parse_pe_arch(header: &[u8]) -> Option<PeArch> {
    if header.len() < 0x40 || &header[..2] != b"MZ" {
        return None;
    }
    let off = u32::from_le_bytes(header[0x3C..0x40].try_into().ok()?) as usize;
    let sig = header.get(off..off + 6)?;
    if &sig[..4] != b"PE\0\0" {
        return None;
    }
    Some(match u16::from_le_bytes([sig[4], sig[5]]) {
        0x014C => PeArch::X86,
        0x8664 => PeArch::X64,
        0xAA64 => PeArch::Arm64,
        other => PeArch::Other(other),
    })
}

pub fn pe_arch(path: &Path) -> std::io::Result<Option<PeArch>> {
    let mut f = std::fs::File::open(path)?;
    let mut dos = [0u8; 0x40];
    if f.read_exact(&mut dos).is_err() || &dos[..2] != b"MZ" {
        return Ok(None);
    }
    let off = u32::from_le_bytes(dos[0x3C..0x40].try_into().unwrap()) as u64;
    if off > 16 * 1024 * 1024 {
        return Ok(None);
    }
    f.seek(SeekFrom::Start(off))?;
    let mut sig = [0u8; 6];
    if f.read_exact(&mut sig).is_err() {
        return Ok(None);
    }
    let mut buf = dos.to_vec();
    buf.resize(off as usize, 0);
    buf.extend_from_slice(&sig);
    Ok(parse_pe_arch(&buf))
}

/// Builds a minimal fake PE header (tests).
pub fn fake_pe(machine: u16) -> Vec<u8> {
    let mut v = vec![0u8; 0x80];
    v[0] = b'M';
    v[1] = b'Z';
    v[0x3C..0x40].copy_from_slice(&0x40u32.to_le_bytes());
    v[0x40..0x44].copy_from_slice(b"PE\0\0");
    v[0x44..0x46].copy_from_slice(&machine.to_le_bytes());
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_headers() {
        assert_eq!(parse_pe_arch(&fake_pe(0x14C)), Some(PeArch::X86));
        assert_eq!(parse_pe_arch(&fake_pe(0x8664)), Some(PeArch::X64));
        assert_eq!(parse_pe_arch(b"#!/bin/sh\n"), None);
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("g.exe");
        std::fs::write(&p, fake_pe(0x14C)).unwrap();
        assert_eq!(pe_arch(&p).unwrap(), Some(PeArch::X86));
    }

    #[test]
    fn real_wine_binaries_if_present() {
        for (path, want) in [
            ("/usr/lib/wine/i386-windows/notepad.exe", PeArch::X86),
            ("/usr/lib/wine/x86_64-windows/notepad.exe", PeArch::X64),
        ] {
            let p = Path::new(path);
            if p.exists() {
                assert_eq!(pe_arch(p).unwrap(), Some(want), "{path}");
            } else {
                eprintln!("SKIPPED: {path} not present");
            }
        }
    }
}

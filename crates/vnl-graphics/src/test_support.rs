//! Test helpers (fixture trees, fake executables).
#![cfg(test)]

use std::path::Path;

/// Write an executable `#!/bin/sh` script `name` into `dir`.
pub fn script(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

/// Fake `gamescope` answering `--version` and `--help` (with the given `-F` filter list).
pub fn fake_gamescope(dir: &std::path::Path, name: &str, filters: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(
            &path,
            format!(
                "#!/bin/sh\ncase \"$1\" in\n--version) echo 'gamescope version 3.17.0 (gcc 16)' >&2;;\n--help) echo 'usage: gamescope [options...] -- [command...]'; echo '  -F, --filter                   upscaler filter ({filters})';;\nesac\n"
            ),
        )
        .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// EDID captured from a real monitor (Samsung Odyssey G40B, 1920x1080@240).
pub const EDID_G40B: [u8; 128] = [
    0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x4c, 0x2d, 0x78, 0x72, 0x54, 0x5a, 0x4b, 0x30, 0x20, 0x23, 0x01,
    0x04, 0xb5, 0x36, 0x1e, 0x78, 0x3b, 0x54, 0x15, 0xac, 0x52, 0x4e, 0xa2, 0x25, 0x0e, 0x50, 0x54, 0x25, 0xcf, 0x00,
    0x81, 0xc0, 0x81, 0x00, 0x81, 0x80, 0x95, 0x00, 0xa9, 0xc0, 0xb3, 0x00, 0x71, 0x4f, 0x01, 0x01, 0xca, 0xdc, 0x80,
    0xc8, 0x70, 0x38, 0x20, 0x40, 0x44, 0x20, 0x28, 0x04, 0x20, 0x2f, 0x21, 0x00, 0x00, 0x1a, 0x00, 0x00, 0x00, 0xfd,
    0x00, 0x30, 0xf0, 0xff, 0xff, 0x39, 0x01, 0x0a, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x00, 0x00, 0x00, 0xfc, 0x00,
    0x4f, 0x64, 0x79, 0x73, 0x73, 0x65, 0x79, 0x20, 0x47, 0x34, 0x30, 0x42, 0x0a, 0x00, 0x00, 0x00, 0xff, 0x00, 0x48,
    0x4e, 0x4d, 0x59, 0x38, 0x30, 0x30, 0x31, 0x32, 0x35, 0x0a, 0x20, 0x20, 0x01, 0x36,
];

pub fn drm_fixture(root: &Path) {
    let drm = root.join("sys/class/drm");
    let mk = |name: &str, status: &str, modes: &str, edid: Option<&[u8]>| {
        let d = drm.join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("status"), format!("{status}\n")).unwrap();
        std::fs::write(d.join("modes"), modes).unwrap();
        if let Some(e) = edid {
            std::fs::write(d.join("edid"), e).unwrap();
        }
    };
    mk("card1-DP-1", "disconnected", "", None);
    mk("card1-Writeback-1", "unknown", "", None);
    mk("card2-DP-8", "connected", "1920x1080\n1920x1080\n1280x720\n", Some(&EDID_G40B));
    mk("card2-HDMI-A-2", "connected", "2560x1440\n1920x1080\n", None);
    for (card, vendor, device, slot, driver) in [
        ("card1", "0x1002", "0x15bf", "0000:44:00.0", "amdgpu"),
        ("card2", "0x10de", "0x2d04", "0000:01:00.0", "nvidia"),
    ] {
        let pci = root.join("sys/devices/pci").join(slot);
        std::fs::create_dir_all(&pci).unwrap();
        std::fs::write(pci.join("vendor"), format!("{vendor}\n")).unwrap();
        std::fs::write(pci.join("device"), format!("{device}\n")).unwrap();
        let drv = root.join("sys/bus/pci/drivers").join(driver);
        std::fs::create_dir_all(&drv).unwrap();
        std::os::unix::fs::symlink(&drv, pci.join("driver")).unwrap();
        let c = drm.join(card);
        std::fs::create_dir_all(&c).unwrap();
        std::os::unix::fs::symlink(&pci, c.join("device")).unwrap();
    }
    std::fs::create_dir_all(drm.join("renderD128")).unwrap();
}

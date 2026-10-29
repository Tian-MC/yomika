//! Library-level tests. Never touch real user directories: everything lives
//! in a tempdir via `AppPaths::under`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use vnl_core::{AppPaths, AppSettings, ErrorCode, GameQuery, GameSettings, GameSort, PlayStatus, PrefixChoice};

use crate::{ImportRequest, Library, SaveBackupKind, SaveLocationSource, VndbClient};

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn library_is_send_sync() {
    assert_send_sync::<Library>();
}

struct Env {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    paths: AppPaths,
    lib: Library,
}

fn env() -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let paths = AppPaths::under(&root.join("vnl"));
    paths.ensure().unwrap();
    let lib = Library::open(&paths).unwrap();
    Env { _tmp: tmp, root, paths, lib }
}

fn make_exe(root: &Path, rel: &str) -> PathBuf {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, b"MZ fake exe").unwrap();
    p
}

fn import(lib: &Library, title: &str, exe: &Path) -> vnl_core::Game {
    lib.import_game(ImportRequest { title: title.into(), exe_path: exe.to_path_buf(), ..Default::default() }).unwrap()
}

// --------------------------------------------------------------------- games

#[test]
fn import_get_update_remove() {
    let e = env();
    let exe = make_exe(&e.root, "games/My Game ゲーム/ゲーム.exe");
    let g = import(&e.lib, "  My Game  ", &exe);
    assert_eq!(g.title, "My Game");
    assert_eq!(g.install_dir, exe.parent().unwrap());
    assert_eq!(g.status, PlayStatus::Unplayed);
    assert!(g.added_at > 0);
    assert_eq!(e.lib.get_game(g.id).unwrap(), g);

    // persisted across reopen
    let reopened = Library::open(&e.paths).unwrap();
    assert_eq!(reopened.get_game(g.id).unwrap(), g);

    e.lib.record_play_session(g.id, 1000, 1600).unwrap();
    let mut edited = e.lib.get_game(g.id).unwrap();
    edited.title = "Renamed".into();
    edited.original_title = Some("名前".into());
    edited.launch_args = vec!["--a b".into(), "日本".into()];
    edited.favorite = true;
    edited.status = PlayStatus::Completed;
    edited.notes = Some("note".into());
    edited.metadata.tags = vec!["Mystery".into()];
    // fields that must NOT change through update
    edited.playtime_seconds = 0;
    edited.added_at = 1;
    edited.last_played_at = None;
    let updated = e.lib.update_game(&edited).unwrap();
    assert_eq!(updated.title, "Renamed");
    assert_eq!(updated.launch_args, vec!["--a b", "日本"]);
    assert_eq!(updated.status, PlayStatus::Completed);
    assert!(updated.favorite);
    assert_eq!(updated.metadata.tags, vec!["Mystery"]);
    assert_eq!(updated.playtime_seconds, 600);
    assert_eq!(updated.added_at, g.added_at);
    assert_eq!(updated.last_played_at, Some(1600));

    let mut missing = updated.clone();
    missing.id = 9999;
    assert_eq!(e.lib.update_game(&missing).unwrap_err().code, ErrorCode::NotFound);

    e.lib.remove_game(g.id).unwrap();
    assert_eq!(e.lib.get_game(g.id).unwrap_err().code, ErrorCode::NotFound);
    assert!(exe.is_file(), "removing a game must never delete files");
    assert_eq!(e.lib.remove_game(g.id).unwrap_err().code, ErrorCode::NotFound);
    // AUTOINCREMENT: ids are never reused (backup folders are keyed by id)
    let again = import(&e.lib, "Again", &exe);
    assert!(again.id > g.id);
}

#[test]
fn import_validation_and_duplicates() {
    let e = env();
    let exe = make_exe(&e.root, "g/a.exe");
    let err = e.lib.import_game(ImportRequest { title: " ".into(), exe_path: exe.clone(), ..Default::default() });
    assert_eq!(err.unwrap_err().code, ErrorCode::InvalidInput);
    let err =
        e.lib.import_game(ImportRequest { title: "x".into(), exe_path: "rel/a.exe".into(), ..Default::default() });
    assert_eq!(err.unwrap_err().code, ErrorCode::InvalidInput);
    let err = e.lib.import_game(ImportRequest {
        title: "x".into(),
        exe_path: e.root.join("missing.exe"),
        ..Default::default()
    });
    assert_eq!(err.unwrap_err().code, ErrorCode::NotFound);

    let g = import(&e.lib, "A", &exe);
    // Same file through a different spelling and through a symlink.
    let dotted = e.root.join("g/../g/./a.exe");
    let err =
        e.lib.import_game(ImportRequest { title: "dup".into(), exe_path: dotted, ..Default::default() }).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput);
    assert_eq!(err.details.as_deref(), Some(format!("existingGameId={}", g.id).as_str()));
    let link = e.root.join("link.exe");
    std::os::unix::fs::symlink(&exe, &link).unwrap();
    assert_eq!(e.lib.find_game_by_exe(&link).unwrap(), Some(g.id));

    // Update cannot steal another game's exe.
    let exe_b = make_exe(&e.root, "g/b.exe");
    let mut b = import(&e.lib, "B", &exe_b);
    b.exe_path = exe.clone();
    assert_eq!(e.lib.update_game(&b).unwrap_err().code, ErrorCode::InvalidInput);

    // Scan reports the existing game.
    let scan = e.lib.scan_path(&e.root.join("g")).unwrap();
    assert!(scan.existing_game_id.is_some());
}

#[test]
fn list_search_filter_sort() {
    let e = env();
    let a = import(&e.lib, "Ever17 -the out of infinity-", &make_exe(&e.root, "a/e17.exe"));
    let b = import(&e.lib, "夏色 ハイスクル", &make_exe(&e.root, "b/n.exe"));
    let c = import(&e.lib, "zeta", &make_exe(&e.root, "c/z.exe"));

    let mut ga = a.clone();
    ga.original_title = Some("エバーセブンティーン".into());
    ga.metadata.developers = vec!["KID".into()];
    ga.metadata.tags = vec!["Mystery".into(), "Underwater".into()];
    ga.metadata.release_date = Some("2002-08-29".into());
    ga.favorite = true;
    e.lib.update_game(&ga).unwrap();
    let mut gb = b.clone();
    gb.metadata.aliases = vec!["Natsuiro".into()];
    gb.metadata.release_date = Some("2010".into());
    gb.status = PlayStatus::Completed;
    e.lib.update_game(&gb).unwrap();
    e.lib.record_play_session(c.id, 100, 5100).unwrap();
    e.lib.record_play_session(a.id, 100, 200).unwrap();

    let ids = |q: GameQuery| e.lib.list_games(&q).unwrap().into_iter().map(|g| g.id).collect::<Vec<_>>();
    let search = |s: &str| ids(GameQuery { search: Some(s.into()), ..Default::default() });
    assert_eq!(search("EVER17"), vec![a.id]);
    assert_eq!(search("ｅｖｅｒ"), vec![a.id], "full-width input");
    assert_eq!(search("えばー"), vec![a.id], "hiragana matches katakana original title");
    assert_eq!(search("kid"), vec![a.id], "developer");
    assert_eq!(search("natsu"), vec![b.id], "alias");
    assert_eq!(search("夏色"), vec![b.id]);
    assert_eq!(search("   "), ids(GameQuery::default()), "blank search = no filter");
    assert!(search("nothing-matches").is_empty());

    assert_eq!(ids(GameQuery { favorite_only: true, ..Default::default() }), vec![a.id]);
    assert_eq!(ids(GameQuery { status: Some(PlayStatus::Completed), ..Default::default() }), vec![b.id]);
    assert_eq!(ids(GameQuery { status: Some(PlayStatus::Playing), ..Default::default() }), vec![a.id, c.id]);
    assert_eq!(ids(GameQuery { tag: Some("mystery".into()), ..Default::default() }), vec![a.id]);

    let sorted = |sort, descending| ids(GameQuery { sort, descending, ..Default::default() });
    assert_eq!(sorted(GameSort::Title, false), vec![a.id, c.id, b.id]);
    assert_eq!(sorted(GameSort::Title, true), vec![b.id, c.id, a.id]);
    assert_eq!(sorted(GameSort::Playtime, true), vec![c.id, a.id, b.id]);
    assert_eq!(sorted(GameSort::Playtime, false), vec![b.id, a.id, c.id]);
    assert_eq!(sorted(GameSort::LastPlayed, true), vec![c.id, a.id, b.id], "never played last");
    assert_eq!(sorted(GameSort::LastPlayed, false), vec![a.id, c.id, b.id], "never played last");
    assert_eq!(sorted(GameSort::AddedAt, false), vec![a.id, b.id, c.id]);
    assert_eq!(sorted(GameSort::AddedAt, true), vec![c.id, b.id, a.id]);
    assert_eq!(sorted(GameSort::ReleaseDate, false), vec![a.id, b.id, c.id], "unknown date last");
    assert_eq!(sorted(GameSort::ReleaseDate, true), vec![b.id, a.id, c.id], "unknown date last");
}

#[test]
fn playtime_recording() {
    let e = env();
    let g = import(&e.lib, "G", &make_exe(&e.root, "g.exe"));
    e.lib.record_play_session(g.id, 100, 100).unwrap(); // zero: ignored
    e.lib.record_play_session(g.id, 500, 100).unwrap(); // negative: ignored
    let same = e.lib.get_game(g.id).unwrap();
    assert_eq!((same.playtime_seconds, same.status, same.last_played_at), (0, PlayStatus::Unplayed, None));
    assert!(e.lib.play_sessions(g.id).unwrap().is_empty());

    e.lib.record_play_session(g.id, 1000, 1060).unwrap();
    e.lib.record_play_session(g.id, 200, 230).unwrap(); // older session recorded late
    let g2 = e.lib.get_game(g.id).unwrap();
    assert_eq!(g2.playtime_seconds, 90);
    assert_eq!(g2.status, PlayStatus::Playing);
    assert_eq!(g2.last_played_at, Some(1060), "last_played_at never goes backwards");
    let sessions = e.lib.play_sessions(g.id).unwrap();
    assert_eq!(sessions.len(), 2);
    assert_eq!((sessions[0].started_at, sessions[0].duration_seconds), (1000, 60));

    let mut done = g2.clone();
    done.status = PlayStatus::Completed;
    e.lib.update_game(&done).unwrap();
    e.lib.record_play_session(g.id, 2000, 2010).unwrap();
    assert_eq!(e.lib.get_game(g.id).unwrap().status, PlayStatus::Completed);

    assert_eq!(e.lib.record_play_session(4242, 0, 10).unwrap_err().code, ErrorCode::NotFound);
    assert_eq!(e.lib.play_sessions(4242).unwrap_err().code, ErrorCode::NotFound);
}

#[test]
fn hook_thread_selection_persists_per_game_without_migration() {
    let e = env();
    let first = import(&e.lib, "First", &make_exe(&e.root, "first.exe"));
    let second = import(&e.lib, "Second", &make_exe(&e.root, "second.exe"));
    let mut settings = e.lib.get_settings(first.id).unwrap();
    let original = settings.clone();
    let schema: i64 = e.lib.conn().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    settings.translation.hook.thread = Some(vnl_core::HookThreadSelection {
        name: "Engine".into(),
        hook_code: "HS4@0:Game.exe".into(),
        context: "FFFFFFFFFFFFFFFF".into(),
        context2: "0".into(),
    });
    e.lib.save_settings(first.id, &settings).unwrap();
    let reopened = Library::open(&e.paths).unwrap();
    assert_eq!(reopened.get_settings(first.id).unwrap(), settings);
    assert_eq!(reopened.get_settings(second.id).unwrap(), original);
    let after: i64 = reopened.conn().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(schema, after);
    // Legacy JSON without a selection loads as paused.
    let mut legacy = serde_json::to_value(&settings).unwrap();
    legacy["translation"]["hook"].as_object_mut().unwrap().remove("thread");
    e.lib
        .conn()
        .execute(
            "UPDATE game_settings SET settings = ?1 WHERE game_id = ?2",
            rusqlite::params![legacy.to_string(), first.id],
        )
        .unwrap();
    assert!(reopened.get_settings(first.id).unwrap().translation.hook.thread.is_none());
}

#[test]
fn settings_defaults_and_tolerance() {
    let e = env();
    let mut app = AppSettings::default();
    app.default_graphics.gamescope.enabled = true;
    app.default_graphics.gamescope.fps_limit = Some(60);
    app.vndb.enabled = false;
    app.ui_language = Some("ja".into());
    e.lib.save_app_settings(&app).unwrap();
    assert_eq!(e.lib.get_app_settings().unwrap(), app);
    assert_eq!(Library::open(&e.paths).unwrap().get_app_settings().unwrap(), app);

    let g = import(&e.lib, "G", &make_exe(&e.root, "g.exe"));
    let s = e.lib.get_settings(g.id).unwrap();
    assert!(s.graphics.gamescope.enabled);
    assert_eq!(s.graphics.gamescope.fps_limit, Some(60));
    assert_eq!(s.runtime.prefix, PrefixChoice::Dedicated);
    assert!(!s.translation.enabled);

    let mut custom = GameSettings::default();
    custom.runtime.prefix = PrefixChoice::Custom { path: "/pfx/私の prefix".into() };
    custom.translation.target_language = Some("en".into());
    assert_eq!(e.lib.save_settings(g.id, &custom).unwrap(), custom);
    assert_eq!(e.lib.get_settings(g.id).unwrap(), custom);

    // Explicit settings on import win over defaults.
    let g2 = e
        .lib
        .import_game(ImportRequest {
            title: "G2".into(),
            exe_path: make_exe(&e.root, "g2.exe"),
            settings: Some(custom.clone()),
            vndb_id: Some("v17".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(e.lib.get_settings(g2.id).unwrap(), custom);
    assert_eq!(g2.metadata.vndb_id.as_deref(), Some("v17"));

    // Older/partial JSON keeps deserializing thanks to serde defaults.
    e.lib
        .conn()
        .execute(
            "UPDATE game_settings SET settings = '{\"runtime\":{\"waitForWineserver\":true}}' WHERE game_id = ?1",
            [g.id],
        )
        .unwrap();
    let partial = e.lib.get_settings(g.id).unwrap();
    assert!(partial.runtime.wait_for_wineserver);
    assert!(partial.runtime.locale.japanese_locale);
    e.lib.conn().execute("UPDATE app_settings SET settings = '{\"theme\":\"dark\"}'", []).unwrap();
    let a = e.lib.get_app_settings().unwrap();
    assert_eq!(a.theme, vnl_core::Theme::Dark);
    assert!(a.vndb.enabled);

    // Missing settings row (e.g. legacy import) falls back to derived defaults.
    e.lib.conn().execute("DELETE FROM game_settings WHERE game_id = ?1", [g.id]).unwrap();
    assert_eq!(e.lib.get_settings(g.id).unwrap().runtime.prefix, PrefixChoice::Dedicated);
    assert_eq!(e.lib.get_settings(777).unwrap_err().code, ErrorCode::NotFound);
    assert_eq!(e.lib.save_settings(777, &custom).unwrap_err().code, ErrorCode::NotFound);

    // Corrupt metadata JSON does not hide the game.
    e.lib.conn().execute("UPDATE games SET metadata = 'not json' WHERE id = ?1", [g.id]).unwrap();
    assert_eq!(e.lib.get_game(g.id).unwrap().metadata, Default::default());
}

#[test]
fn scan_paths_with_spaces_and_japanese() {
    let e = env();
    let dir = e.root.join("ゲーム フォルダ/[KID] Ever17 (v1.0)");
    make_exe(&dir, "Ever17.exe");
    std::fs::write(dir.join("data.xp3"), b"x").unwrap();
    make_exe(&dir, "uninstall.exe");
    let scan = e.lib.scan_path(&dir).unwrap();
    assert_eq!(scan.suggested_title, "Ever17");
    assert_eq!(scan.candidates[0].path, dir.join("Ever17.exe"));
    assert_eq!(scan.existing_game_id, None);
    let g = import(&e.lib, &scan.suggested_title, &scan.candidates[0].path);
    let scan = e.lib.scan_path(&dir.join("Ever17.exe")).unwrap();
    assert_eq!(scan.existing_game_id, Some(g.id));
}

// ---------------------------------------------------------------------- VNDB

type Handler = dyn Fn(&str, &str) -> (u16, Vec<(String, String)>, Vec<u8>) + Send + Sync;

struct MockServer {
    url: String,
    requests: Arc<Mutex<Vec<(String, String, String)>>>,
}

/// Minimal HTTP/1.1 server: one request per connection, `Connection: close`.
async fn mock_server(handler: Arc<Handler>) -> MockServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let log = requests.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let handler = handler.clone();
            let log = log.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let header_end = loop {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break pos + 4;
                    }
                };
                let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
                let len = head
                    .lines()
                    .find_map(|l| {
                        let (k, v) = l.split_once(':')?;
                        k.eq_ignore_ascii_case("content-length").then(|| v.trim().parse::<usize>().ok())?
                    })
                    .unwrap_or(0);
                while buf.len() < header_end + len {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                let body = String::from_utf8_lossy(&buf[header_end..]).to_string();
                let mut first = head.lines().next().unwrap_or("").split_whitespace();
                let method = first.next().unwrap_or("").to_string();
                let path = first.next().unwrap_or("").to_string();
                let ua = head
                    .lines()
                    .find_map(|l| l.strip_prefix("user-agent: ").or_else(|| l.strip_prefix("User-Agent: ")))
                    .unwrap_or("")
                    .to_string();
                log.lock().unwrap().push((format!("{method} {path}"), body.clone(), ua));
                let (status, headers, payload) = handler(&path, &body);
                let mut resp =
                    format!("HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n", payload.len());
                for (k, v) in headers {
                    resp.push_str(&format!("{k}: {v}\r\n"));
                }
                resp.push_str("\r\n");
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.write_all(&payload).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    MockServer { url, requests }
}

const SEARCH_FIXTURE: &str = include_str!("../tests/fixtures/vndb_search_ever17.json");
const V17_FIXTURE: &str = include_str!("../tests/fixtures/vndb_vn_v17.json");
const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10, b'J', b'F', b'I', b'F', 0, 1, 2, 3];

fn json_ok(body: &str) -> (u16, Vec<(String, String)>, Vec<u8>) {
    (200, vec![("Content-Type".into(), "application/json".into())], body.as_bytes().to_vec())
}

/// VNDB mock: `/vn` answers search or v17 lookups, `/cover.jpg` serves an image.
async fn vndb_mock(cover_ok: bool) -> MockServer {
    let base: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let b2 = base.clone();
    let server = mock_server(Arc::new(move |path: &str, body: &str| {
        let base = b2.lock().unwrap().clone();
        match path {
            "/vn" if body.contains("\"search\"") => json_ok(SEARCH_FIXTURE),
            "/vn" if body.contains("\"v17\"") => {
                json_ok(&V17_FIXTURE.replace("https://t.vndb.org/cv/12/79412.jpg", &format!("{base}/cover.jpg")))
            }
            "/vn" => json_ok("{\"results\":[],\"more\":false}"),
            "/cover.jpg" if cover_ok => (200, vec![("Content-Type".into(), "image/jpeg".into())], JPEG.to_vec()),
            _ => (404, vec![], b"not found".to_vec()),
        }
    }))
    .await;
    *base.lock().unwrap() = server.url.clone();
    server
}

#[tokio::test]
async fn vndb_search_and_apply_against_mock() {
    let e = env();
    let server = vndb_mock(true).await;
    let lib = Library::open(&e.paths).unwrap().with_vndb_client(VndbClient::new(&server.url));
    let exe = make_exe(&e.root, "e17/e17.exe");
    let g = import(&lib, "e17", &exe);
    lib.record_play_session(g.id, 10, 70).unwrap();

    let results = lib.vndb_search("Ever17").await.unwrap();
    assert_eq!(results.len(), 3);
    assert!(results.iter().any(|r| r.id == "v17"));

    let applied = lib.vndb_apply(g.id, "v17").await.unwrap();
    assert_eq!(applied.title, "Ever17 -the out of infinity-");
    assert_eq!(applied.metadata.vndb_id.as_deref(), Some("v17"));
    assert_eq!(applied.metadata.developers, vec!["KID", "SDR Project"]);
    assert_eq!(applied.playtime_seconds, 60, "apply must not touch playtime");
    assert_eq!(applied.exe_path, exe);
    let cover = applied.metadata.cover_path.clone().expect("cover stored");
    assert!(cover.starts_with(e.paths.covers_dir()));
    assert_eq!(std::fs::read(&cover).unwrap(), JPEG);
    assert!(applied.metadata.fetched_at.is_some());
    assert_eq!(lib.get_game(g.id).unwrap(), applied);
    // No temp files left next to the cover.
    let leftovers: Vec<_> = std::fs::read_dir(e.paths.covers_dir()).unwrap().flatten().collect();
    assert_eq!(leftovers.len(), 1);

    let reqs = server.requests.lock().unwrap().clone();
    let (line, body, ua) = &reqs[0];
    assert_eq!(line, "POST /vn");
    let body: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(body["filters"], serde_json::json!(["search", "=", "Ever17"]));
    assert!(body["fields"].as_str().unwrap().contains("image{url,sexual}"));
    assert!(ua.starts_with("yomika/"), "user agent: {ua}");

    // Unknown id -> NotFound, game unchanged.
    let err = lib.vndb_apply(g.id, "v999999").await.unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    assert_eq!(lib.get_game(g.id).unwrap(), applied);
    assert_eq!(lib.vndb_apply(g.id, "garbage").await.unwrap_err().code, ErrorCode::InvalidInput);
    assert_eq!(lib.vndb_apply(4242, "v17").await.unwrap_err().code, ErrorCode::NotFound);
    assert_eq!(lib.vndb_search("  ").await.unwrap_err().code, ErrorCode::InvalidInput);
}

#[tokio::test]
async fn vndb_cover_failure_still_applies_metadata() {
    let e = env();
    let server = vndb_mock(false).await;
    let lib = Library::open(&e.paths).unwrap().with_vndb_client(VndbClient::new(&server.url));
    let g = import(&lib, "e17", &make_exe(&e.root, "e17.exe"));
    let applied = lib.vndb_apply(g.id, "17").await.unwrap();
    assert_eq!(applied.metadata.vndb_id.as_deref(), Some("v17"));
    assert_eq!(applied.metadata.cover_path, None);
    assert!(applied.metadata.cover_url.is_some());
}

#[tokio::test]
async fn vndb_disabled_offline_throttled_and_server_errors() {
    let e = env();
    let g = import(&e.lib, "e17", &make_exe(&e.root, "e17.exe"));

    // Disabled in settings: no request is made.
    let server = vndb_mock(true).await;
    let lib = Library::open(&e.paths).unwrap().with_vndb_client(VndbClient::new(&server.url));
    let mut app = lib.get_app_settings().unwrap();
    app.vndb.enabled = false;
    lib.save_app_settings(&app).unwrap();
    let err = lib.vndb_search("Ever17").await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Unsupported);
    assert!(err.message.contains("disabled"));
    assert_eq!(lib.vndb_apply(g.id, "v17").await.unwrap_err().code, ErrorCode::Unsupported);
    assert!(server.requests.lock().unwrap().is_empty());
    app.vndb.enabled = true;
    lib.save_app_settings(&app).unwrap();

    // Offline: nothing listens on the port.
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let offline =
        Library::open(&e.paths).unwrap().with_vndb_client(VndbClient::new(format!("http://127.0.0.1:{port}")));
    let err = offline.vndb_search("Ever17").await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Network);
    assert!(err.message.contains("offline") || err.message.contains("reach"), "{err:?}");
    let before = offline.get_game(g.id).unwrap();
    assert_eq!(offline.vndb_apply(g.id, "v17").await.unwrap_err().code, ErrorCode::Network);
    assert_eq!(offline.get_game(g.id).unwrap(), before, "failed apply must not modify the game");

    // 429 with a long Retry-After -> immediate friendly error.
    let throttled = mock_server(Arc::new(|_: &str, _: &str| {
        (429, vec![("Retry-After".into(), "60".into())], b"Throttled".to_vec())
    }))
    .await;
    let lib429 = Library::open(&e.paths).unwrap().with_vndb_client(VndbClient::new(&throttled.url));
    let err = lib429.vndb_search("Ever17").await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Network);
    assert!(err.message.contains("rate limiting"));
    assert_eq!(throttled.requests.lock().unwrap().len(), 1);

    // 429 with a short Retry-After -> one transparent retry.
    let count = Arc::new(Mutex::new(0));
    let c2 = count.clone();
    let flaky = mock_server(Arc::new(move |_: &str, _: &str| {
        let mut n = c2.lock().unwrap();
        *n += 1;
        if *n == 1 {
            (429, vec![("Retry-After".into(), "0".into())], b"Throttled".to_vec())
        } else {
            json_ok(SEARCH_FIXTURE)
        }
    }))
    .await;
    let lib_flaky = Library::open(&e.paths).unwrap().with_vndb_client(VndbClient::new(&flaky.url));
    assert_eq!(lib_flaky.vndb_search("Ever17").await.unwrap().len(), 3);
    assert_eq!(*count.lock().unwrap(), 2);

    // 502 and garbage JSON.
    let down = mock_server(Arc::new(|path: &str, _: &str| {
        if path == "/vn" { (502, vec![], b"Bad Gateway".to_vec()) } else { (200, vec![], b"{nope".to_vec()) }
    }))
    .await;
    let lib_down = Library::open(&e.paths).unwrap().with_vndb_client(VndbClient::new(&down.url));
    let err = lib_down.vndb_search("x").await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Network);
    assert!(err.message.contains("temporarily unavailable"));
    let garbage = mock_server(Arc::new(|_: &str, _: &str| json_ok("{nope"))).await;
    let lib_garbage = Library::open(&e.paths).unwrap().with_vndb_client(VndbClient::new(&garbage.url));
    assert_eq!(lib_garbage.vndb_apply(g.id, "v17").await.unwrap_err().code, ErrorCode::Network);
    assert_eq!(lib_garbage.get_game(g.id).unwrap(), before);
}

/// Real network. Run with: cargo test -p vnl-library live_vndb -- --ignored --nocapture
#[tokio::test]
#[ignore = "hits the real api.vndb.org"]
async fn live_vndb_search_and_apply() {
    let e = env();
    let lib = Library::open(&e.paths).unwrap().with_vndb_client(VndbClient::new(crate::vndb::DEFAULT_BASE_URL));
    let results = lib.vndb_search("Ever17").await.unwrap();
    println!("search results: {:#?}", results.iter().map(|r| (&r.id, &r.title, &r.released)).collect::<Vec<_>>());
    assert!(results.iter().any(|r| r.id == "v17"));
    let g = import(&lib, "e17", &make_exe(&e.root, "Ever17/Ever17.exe"));
    let applied = lib.vndb_apply(g.id, "v17").await.unwrap();
    println!("applied: title={:?} original={:?}", applied.title, applied.original_title);
    println!("metadata: {:#?}", applied.metadata);
    assert_eq!(applied.metadata.vndb_id.as_deref(), Some("v17"));
    assert!(applied.metadata.developers.iter().any(|d| d == "KID"));
    let cover = applied.metadata.cover_path.expect("cover downloaded");
    let size = std::fs::metadata(&cover).unwrap().len();
    println!("cover: {} ({size} bytes)", cover.display());
    assert!(size > 1000);
}

// --------------------------------------------------------------------- saves

#[test]
fn backup_modify_restore_cycle() {
    let e = env();
    let install = e.root.join("games/セーブ テスト");
    let exe = make_exe(&install, "game.exe");
    let save_dir = install.join("savedata");
    std::fs::create_dir_all(save_dir.join("sub dir")).unwrap();
    std::fs::write(save_dir.join("slot1.sav"), b"original-1").unwrap();
    std::fs::write(save_dir.join("sub dir/スロット2.sav"), b"original-2").unwrap();
    std::fs::create_dir_all(save_dir.join("empty")).unwrap();
    let g = import(&e.lib, "Save Test", &exe);

    let detected = e.lib.detect_saves(g.id, None).unwrap();
    assert_eq!(detected.len(), 1);
    assert_eq!(detected[0].path, save_dir);
    assert_eq!(detected[0].source, SaveLocationSource::Detected);
    assert!(detected[0].exists);

    let backup = e.lib.create_backup(g.id, Some("first".into()), None).unwrap();
    assert_eq!(backup.file_count, 2);
    assert_eq!(backup.size_bytes, 20);
    assert_eq!(backup.kind, SaveBackupKind::Manual);
    assert_eq!(backup.source_paths, vec![save_dir.clone()]);
    assert!(backup.archive_path.starts_with(e.paths.backups_dir().join(g.id.to_string())));
    assert!(backup.archive_path.is_file());
    assert_eq!(e.lib.list_backups(g.id).unwrap(), vec![backup.clone()]);

    // Modify saves.
    std::fs::write(save_dir.join("slot1.sav"), b"modified").unwrap();
    std::fs::remove_file(save_dir.join("sub dir/スロット2.sav")).unwrap();
    std::fs::remove_dir(save_dir.join("empty")).unwrap();
    std::fs::write(save_dir.join("new.sav"), b"new").unwrap();

    // Without confirmation nothing happens.
    let err = e.lib.restore_backup(backup.id, false).unwrap_err();
    assert_eq!(err.code, ErrorCode::ConfirmationRequired);
    assert_eq!(std::fs::read(save_dir.join("slot1.sav")).unwrap(), b"modified");
    assert_eq!(e.lib.list_backups(g.id).unwrap().len(), 1);

    let result = e.lib.restore_backup(backup.id, true).unwrap();
    assert_eq!(result.files_restored, 2);
    assert_eq!(std::fs::read(save_dir.join("slot1.sav")).unwrap(), b"original-1");
    assert_eq!(std::fs::read(save_dir.join("sub dir/スロット2.sav")).unwrap(), b"original-2");
    assert!(save_dir.join("empty").is_dir());
    assert_eq!(std::fs::read(save_dir.join("new.sav")).unwrap(), b"new", "restore never deletes files");

    // The safety backup holds the pre-restore state and can itself be restored.
    let safety = result.safety_backup.expect("safety backup");
    assert_eq!(safety.kind, SaveBackupKind::Safety);
    assert_eq!(safety.file_count, 2); // slot1 (modified) + new.sav
    assert_eq!(e.lib.list_backups(g.id).unwrap().len(), 2);
    e.lib.restore_backup(safety.id, true).unwrap();
    assert_eq!(std::fs::read(save_dir.join("slot1.sav")).unwrap(), b"modified");

    // No staging/partial leftovers.
    let leftovers: Vec<_> = std::fs::read_dir(e.paths.backups_dir().join(g.id.to_string()))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with('.'))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");

    assert_eq!(e.lib.restore_backup(9999, true).unwrap_err().code, ErrorCode::NotFound);
}

#[test]
fn manual_single_file_location_roundtrip() {
    let e = env();
    let exe = make_exe(&e.root, "g/game.exe");
    let g = import(&e.lib, "G", &exe);
    assert_eq!(e.lib.create_backup(g.id, None, None).unwrap_err().code, ErrorCode::NotFound);

    let file = e.root.join("elsewhere/save file.dat");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, b"v1").unwrap();
    let dir = e.root.join("other saves");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a"), b"a1").unwrap();
    assert_eq!(e.lib.set_save_locations(g.id, &["relative".into()]).unwrap_err().code, ErrorCode::InvalidInput);
    let locs = e.lib.set_save_locations(g.id, &[file.clone(), dir.clone(), file.clone()]).unwrap();
    assert_eq!(locs.len(), 2);
    assert!(locs.iter().all(|l| l.source == SaveLocationSource::Manual && l.exists));

    let b = e.lib.create_backup(g.id, None, None).unwrap();
    assert_eq!(b.source_paths, vec![file.clone(), dir.clone()]);
    std::fs::write(&file, b"v2").unwrap();
    std::fs::write(dir.join("a"), b"a2").unwrap();
    e.lib.restore_backup(b.id, true).unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), b"v1");
    assert_eq!(std::fs::read(dir.join("a")).unwrap(), b"a1");
    assert!(!e.root.join("elsewhere/save file.dat/save file.dat").exists());
}

#[test]
fn restore_rejects_path_traversal() {
    let e = env();
    let exe = make_exe(&e.root, "g/game.exe");
    let g = import(&e.lib, "G", &exe);
    let saves = e.root.join("g/save");
    std::fs::create_dir_all(&saves).unwrap();
    std::fs::write(saves.join("x"), b"keep").unwrap();
    let b = e.lib.create_backup(g.id, None, None).unwrap();

    // Overwrite the archive with a malicious one: "0/../../escape.txt".
    {
        let f = std::fs::File::create(&b.archive_path).unwrap();
        let gz = flate2::write::GzEncoder::new(f, flate2::Compression::default());
        let mut builder = tar::Builder::new(gz);
        let data = b"pwned";
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        // set_path refuses "..", so write the raw name bytes.
        let name = b"0/../../escape.txt";
        header.as_old_mut().name[..name.len()].copy_from_slice(name);
        header.set_cksum();
        builder.append(&header, &data[..]).unwrap();
        builder.into_inner().unwrap().finish().unwrap();
    }
    let err = e.lib.restore_backup(b.id, true).unwrap_err();
    assert!(err.message.contains("unsafe"), "{err:?}");
    assert!(!e.root.join("escape.txt").exists());
    assert!(!e.root.join("g/escape.txt").exists());
    assert_eq!(std::fs::read(saves.join("x")).unwrap(), b"keep");
    // Rejected before the safety backup: only the original backup is indexed.
    assert_eq!(e.lib.list_backups(g.id).unwrap().len(), 1);

    // Damaged archive.
    std::fs::write(&b.archive_path, b"not a gzip").unwrap();
    assert_eq!(e.lib.restore_backup(b.id, true).unwrap_err().code, ErrorCode::Io);
}

#[test]
fn prefix_based_detection() {
    let e = env();
    let exe = make_exe(&e.root, "games/ever17/Ever17PC_EN.exe");
    let mut g = import(&e.lib, "Ever17 -the out of infinity-", &exe);
    g.original_title = Some("エバー17".into());
    g.metadata.developers = vec!["KID".into()];
    g.metadata.aliases = vec!["E17".into()];
    let g = e.lib.update_game(&g).unwrap();

    let drive_c = e.root.join("pfx/drive_c");
    let user = drive_c.join("users/steamuser");
    for d in [
        "AppData/Roaming/KID/Ever17",       // developer/title
        "AppData/Roaming/Microsoft/Ever17", // noise dir
        "AppData/Local/SomethingElse",      // unrelated
        "AppData/LocalLow/KID/Unrelated",   // developer only, child not matching
        "Documents/エバー17 セーブ",        // original title
        "Saved Games/Other",
    ] {
        std::fs::create_dir_all(user.join(d)).unwrap();
    }
    let locs = e.lib.detect_saves(g.id, Some(&drive_c)).unwrap();
    let paths: Vec<_> = locs.iter().map(|l| l.path.clone()).collect();
    assert!(paths.contains(&user.join("AppData/Roaming/KID/Ever17")), "{locs:#?}");
    assert!(paths.contains(&user.join("Documents/エバー17 セーブ")), "{locs:#?}");
    assert!(paths.contains(&user.join("AppData/LocalLow/KID")), "{locs:#?}");
    assert!(!paths.contains(&user.join("AppData/Roaming/Microsoft/Ever17")));
    assert!(!paths.iter().any(|p| p.ends_with("SomethingElse") || p.ends_with("Other")));
    assert!(locs.iter().all(|l| l.reason.is_some()));

    // Without drive_c only install-dir detection runs (none here).
    assert!(e.lib.detect_saves(g.id, None).unwrap().is_empty());
    // Detected rows are replaced on each detection, manual ones kept.
    e.lib.set_save_locations(g.id, &[e.root.join("manual")]).unwrap();
    let stored = e.lib.save_locations(g.id).unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].source, SaveLocationSource::Manual);
    assert!(!stored[0].exists);
}

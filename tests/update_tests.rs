mod common;

use appnest::application::commands::ReplaceAppImageCommand;
use appnest::domain::appimage::{AppImage, AppImageError};
use appnest::domain::repository::AppImageRepository;
use appnest::infrastructure::filesystem_repository::FilesystemAppImageRepository;
use appnest::infrastructure::hash::sha1_file;
use appnest::services::launcher::AppImageLauncher;
use appnest::services::update_info::UpdateInfo;
use appnest::services::updater::{AppImageUpdater, UpdateCheck, UpdaterConfig};
use common::*;
use std::collections::HashMap;
use std::fs;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

const GH_INFO: &str = "gh-releases-zsync|owner|repo|latest|*.AppImage.zsync";

#[test]
fn update_information_comes_from_the_elf_section_only() {
    let env = TestEnv::new();
    let with = upd_info_appimage(&env.root.path().join("with.AppImage"), GH_INFO);
    assert!(matches!(
        UpdateInfo::read(&with),
        Some(UpdateInfo::GhReleasesZsync { .. })
    ));

    // The same text elsewhere in the file (not in .upd_info) must not be picked up
    let mut payload = b"hsqs".to_vec();
    payload.extend_from_slice(GH_INFO.as_bytes());
    let decoy = env.root.path().join("decoy.AppImage");
    write_executable(&decoy, &appimage_bytes(&payload));
    assert_eq!(UpdateInfo::read(&decoy), None);

    assert_eq!(UpdateInfo::read(&env.root.path().join("missing")), None);
}

struct Release {
    server: TestServer,
    updater: AppImageUpdater,
    new_file: std::path::PathBuf,
}

/// Publishes `new_file` as the latest release of owner/repo on a local server.
fn publish(env: &TestEnv, new_file: &std::path::Path, sha1: &str, length: u64) -> Release {
    let routes: Arc<Mutex<HashMap<String, Vec<u8>>>> = Arc::new(Mutex::new(HashMap::new()));
    let server = TestServer::start(routes.clone());
    let base = server.base.clone();
    let bytes = fs::read(new_file).unwrap();
    {
        let mut r = routes.lock().unwrap();
        r.insert(
            "/repos/owner/repo/releases/latest".into(),
            format!(
                r#"{{"tag_name":"v2.0","assets":[
                    {{"name":"other.txt","browser_download_url":"{b}/dl/other.txt"}},
                    {{"name":"tool.AppImage.zsync","browser_download_url":"{b}/dl/tool.AppImage.zsync"}}]}}"#,
                b = base
            )
            .into_bytes(),
        );
        r.insert(
            "/dl/tool.AppImage.zsync".into(),
            format!(
                "zsync: 0.6.2\nFilename: tool.AppImage\nMTime: now\nBlocksize: 2048\nLength: {}\nHash-Lengths: 1,2,3\nURL: tool.AppImage\nSHA-1: {}\n\nBINARYBODY",
                length, sha1
            )
            .into_bytes(),
        );
        r.insert("/dl/tool.AppImage".into(), bytes);
    }
    let _ = env;
    Release {
        updater: AppImageUpdater::with_config(UpdaterConfig {
            api_base: base,
            allow_loopback_http: true,
        }),
        server,
        new_file: new_file.to_path_buf(),
    }
}

fn progress_sink() -> impl FnMut(u64, u64) {
    |_, _| {}
}

#[test]
fn an_available_update_is_found_downloaded_and_verified() {
    if !have_tool("curl") {
        eprintln!("skipping: curl is not installed");
        return;
    }
    let env = TestEnv::new();
    let installed = upd_info_appimage(&env.root.path().join("tool-1.AppImage"), GH_INFO);
    let new_file = appimage_with_marker(&env.root.path().join("tool-2.AppImage"), "version two");
    let size = fs::metadata(&new_file).unwrap().len();
    let rel = publish(&env, &new_file, &sha1_file(&new_file).unwrap(), size);

    let UpdateCheck::Available(update) = rel.updater.check(&installed).unwrap() else {
        panic!("an update should be available");
    };
    assert_eq!(update.release.as_deref(), Some("v2.0"));
    assert_eq!(update.filename, "tool.AppImage");
    assert_eq!(update.length, size);
    assert_eq!(
        update.download_url,
        format!("{}/dl/tool.AppImage", rel.server.base)
    );

    let dest = AppImageUpdater::new_download_path(&env.root.path().join("dl")).unwrap();
    let mut calls = 0;
    rel.updater
        .download(
            &update,
            &dest,
            &mut |_, _| calls += 1,
            &AtomicBool::new(false),
        )
        .unwrap();
    assert_eq!(fs::read(&dest).unwrap(), fs::read(&rel.new_file).unwrap());
    assert!(calls >= 1, "progress is reported");
}

#[test]
fn an_up_to_date_appimage_is_reported_as_such() {
    if !have_tool("curl") {
        return;
    }
    let env = TestEnv::new();
    let installed = upd_info_appimage(&env.root.path().join("tool.AppImage"), GH_INFO);
    let size = fs::metadata(&installed).unwrap().len();
    let rel = publish(&env, &installed, &sha1_file(&installed).unwrap(), size);
    assert_eq!(
        rel.updater.check(&installed).unwrap(),
        UpdateCheck::UpToDate
    );
}

#[test]
fn a_tampered_or_mis_sized_download_is_rejected_and_removed() {
    if !have_tool("curl") {
        return;
    }
    let env = TestEnv::new();
    let installed = upd_info_appimage(&env.root.path().join("tool-1.AppImage"), GH_INFO);
    let new_file = appimage_with_marker(&env.root.path().join("tool-2.AppImage"), "genuine");
    let size = fs::metadata(&new_file).unwrap().len();

    // The published checksum belongs to a different file than the one served
    let other = appimage_with_marker(&env.root.path().join("other.AppImage"), "something else");
    let rel = publish(&env, &new_file, &sha1_file(&other).unwrap(), size);
    let UpdateCheck::Available(update) = rel.updater.check(&installed).unwrap() else {
        panic!()
    };
    let dest = AppImageUpdater::new_download_path(&env.root.path().join("dl")).unwrap();
    let err = rel
        .updater
        .download(
            &update,
            &dest,
            &mut progress_sink(),
            &AtomicBool::new(false),
        )
        .unwrap_err();
    assert!(err.to_string().contains("checksum"), "{}", err);
    assert!(
        !dest.exists(),
        "a rejected download must not be left behind"
    );

    // Wrong size
    let rel = publish(&env, &new_file, &sha1_file(&new_file).unwrap(), size + 10);
    let UpdateCheck::Available(update) = rel.updater.check(&installed).unwrap() else {
        panic!()
    };
    let dest = AppImageUpdater::new_download_path(&env.root.path().join("dl")).unwrap();
    assert!(rel
        .updater
        .download(
            &update,
            &dest,
            &mut progress_sink(),
            &AtomicBool::new(false)
        )
        .is_err());
    assert!(!dest.exists());
}

#[test]
fn a_download_that_is_not_an_appimage_is_rejected() {
    if !have_tool("curl") {
        return;
    }
    let env = TestEnv::new();
    let installed = upd_info_appimage(&env.root.path().join("tool-1.AppImage"), GH_INFO);
    let junk = env.root.path().join("junk.bin");
    fs::write(&junk, vec![7u8; 5000]).unwrap();
    let rel = publish(&env, &junk, &sha1_file(&junk).unwrap(), 5000);
    let UpdateCheck::Available(update) = rel.updater.check(&installed).unwrap() else {
        panic!()
    };
    let dest = AppImageUpdater::new_download_path(&env.root.path().join("dl")).unwrap();
    let err = rel
        .updater
        .download(
            &update,
            &dest,
            &mut progress_sink(),
            &AtomicBool::new(false),
        )
        .unwrap_err();
    assert!(err.to_string().contains("not a valid AppImage"), "{}", err);
    assert!(!dest.exists());
}

#[test]
fn appimages_without_usable_update_information_are_reported_clearly() {
    let env = TestEnv::new();
    let plain = simple_appimage(&env.root.path().join("plain.AppImage"));
    let updater = AppImageUpdater::new();
    assert_eq!(
        updater.check(&plain).unwrap(),
        UpdateCheck::NoUpdateInformation
    );

    let odd = upd_info_appimage(
        &env.root.path().join("odd.AppImage"),
        "bintray-zsync|a|b|c|d",
    );
    assert_eq!(
        updater.check(&odd).unwrap(),
        UpdateCheck::Unsupported("bintray-zsync".into())
    );
}

#[test]
fn plain_http_is_refused_unless_the_test_override_is_set() {
    let rel_routes: Arc<Mutex<HashMap<String, Vec<u8>>>> = Arc::new(Mutex::new(HashMap::new()));
    let server = TestServer::start(rel_routes);
    let env = TestEnv::new();
    let installed = upd_info_appimage(&env.root.path().join("tool.AppImage"), GH_INFO);
    let strict = AppImageUpdater::with_config(UpdaterConfig {
        api_base: server.base.clone(),
        allow_loopback_http: false,
    });
    let err = strict.check(&installed).unwrap_err();
    assert!(err.to_string().contains("non-https"), "{}", err);
}

// ---- replacing an application's AppImage ----

fn replace_fixture(
    env: &TestEnv,
    managed: bool,
) -> (
    Arc<FilesystemAppImageRepository>,
    ReplaceAppImageCommand,
    AppImage,
) {
    let repo = Arc::new(FilesystemAppImageRepository::new(env.paths.clone()));
    let launcher = Arc::new(AppImageLauncher::new(env.paths.clone()));
    let cmd = ReplaceAppImageCommand::new(repo.clone(), launcher, env.paths.clone());

    let old_path = if managed {
        appimage_with_marker(&env.paths.applications_dir.join("tool.AppImage"), "old")
    } else {
        appimage_with_marker(&env.root.path().join("tool.AppImage"), "old")
    };
    let mut app = AppImage::new("tool", "Tool", old_path);
    app.managed = managed;
    app.version = Some("1.0".into());
    app.runtime_method = appnest::domain::appimage::RuntimeMethod::Native;
    app.desktop_integrated = true;
    app.set_sha256("deadbeef".into());
    repo.save(&app).unwrap();
    (repo, cmd, app)
}

#[test]
fn replacing_a_managed_app_keeps_its_identity_and_settings() {
    let env = TestEnv::new();
    let (repo, cmd, old) = replace_fixture(&env, true);
    let new_file = appimage_with_marker(&env.root.path().join("tool-2.AppImage"), "new");
    fs::create_dir_all(env.paths.app_extracted_dir("tool")).unwrap();
    fs::write(env.paths.app_extracted_dir("tool").join("AppRun"), "stale").unwrap();

    let updated = cmd.execute("tool", &new_file).unwrap();

    assert_eq!(updated.id, "tool");
    assert_eq!(updated.name, "Tool");
    assert_eq!(updated.runtime_method, old.runtime_method);
    assert!(updated.desktop_integrated && updated.managed);
    assert_ne!(updated.path, old.path);
    assert_eq!(
        fs::read(&updated.path).unwrap(),
        fs::read(&new_file).unwrap()
    );
    assert!(
        !old.path.exists(),
        "the previous managed file is removed after success"
    );
    assert!(new_file.exists(), "the user's file is kept");
    assert!(
        !env.paths.app_extracted_dir("tool").exists(),
        "stale extraction is invalidated"
    );
    assert!(updated.sha256.is_none());
    assert!(updated
        .desktop_entry_path
        .as_ref()
        .is_none_or(|p| p.exists()));
    assert_eq!(repo.list().unwrap().len(), 1);
    assert_eq!(repo.get_by_id("tool").unwrap().unwrap().path, updated.path);
}

#[test]
fn replacing_a_referenced_app_points_at_the_new_file_and_leaves_the_old_one() {
    let env = TestEnv::new();
    let (_repo, cmd, old) = replace_fixture(&env, false);
    let new_file = appimage_with_marker(&env.root.path().join("tool-new.AppImage"), "new");
    let updated = cmd.execute("tool", &new_file).unwrap();
    assert_eq!(updated.path, new_file);
    assert!(old.path.exists(), "an unmanaged original is never deleted");
}

#[test]
fn replacement_refuses_bad_input_and_changes_nothing() {
    let env = TestEnv::new();
    let (repo, cmd, old) = replace_fixture(&env, true);

    let fake = env.root.path().join("fake.AppImage");
    fs::copy("/bin/sh", &fake).unwrap();
    assert!(matches!(
        cmd.execute("tool", &fake),
        Err(AppImageError::InvalidAppImage(_))
    ));
    assert!(matches!(
        cmd.execute("tool", &old.path),
        Err(AppImageError::UpdateError(_))
    ));
    assert!(matches!(
        cmd.execute("nope", &old.path),
        Err(AppImageError::NotFound(_))
    ));

    assert!(old.path.exists());
    assert_eq!(repo.get_by_id("tool").unwrap().unwrap(), old);
    assert_eq!(
        fs::read_dir(&env.paths.applications_dir).unwrap().count(),
        1
    );
}

struct SaveFails {
    inner: FilesystemAppImageRepository,
}
impl AppImageRepository for SaveFails {
    fn list(&self) -> Result<Vec<AppImage>, AppImageError> {
        self.inner.list()
    }
    fn get_by_id(&self, id: &str) -> Result<Option<AppImage>, AppImageError> {
        self.inner.get_by_id(id)
    }
    fn save(&self, _: &AppImage) -> Result<(), AppImageError> {
        Err(AppImageError::StorageError("disk full".into()))
    }
    fn remove(&self, id: &str) -> Result<(), AppImageError> {
        self.inner.remove(id)
    }
}

#[test]
fn a_failed_replacement_rolls_back_completely() {
    let env = TestEnv::new();
    let (real, _, old) = replace_fixture(&env, true);
    let failing = Arc::new(SaveFails {
        inner: FilesystemAppImageRepository::new(env.paths.clone()),
    });
    let launcher = Arc::new(AppImageLauncher::new(env.paths.clone()));
    let cmd = ReplaceAppImageCommand::new(failing, launcher, env.paths.clone());
    let new_file = appimage_with_marker(&env.root.path().join("tool-2.AppImage"), "new");

    assert!(cmd.execute("tool", &new_file).is_err());

    assert!(
        old.path.exists(),
        "the previous version must still be there"
    );
    assert_eq!(
        fs::read_dir(&env.paths.applications_dir).unwrap().count(),
        1,
        "no stray copy"
    );
    assert_eq!(real.get_by_id("tool").unwrap().unwrap(), old);
}

#[test]
fn a_running_application_cannot_be_replaced() {
    let env = TestEnv::new();
    let (_repo, cmd, old) = replace_fixture(&env, true);
    let mut child = Command::new("sleep")
        .arg("30")
        .env("APPIMAGE", &old.path)
        .spawn()
        .unwrap();
    let new_file = appimage_with_marker(&env.root.path().join("tool-2.AppImage"), "new");
    let res = cmd.execute("tool", &new_file);
    let _ = child.kill();
    let _ = child.wait();
    let err = res.unwrap_err();
    assert!(err.to_string().contains("Close the application"), "{}", err);
    assert!(old.path.exists());
}

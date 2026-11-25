mod common;

use appnest::cli::{self, CliContext, Io};
use appnest::services::updater::AppImageUpdater;
use common::*;
use std::ffi::OsString;
use std::fs;
use std::io::Cursor;

struct Cli {
    env: TestEnv,
    ctx: CliContext,
}

struct Output {
    code: i32,
    out: String,
    err: String,
}

fn cli() -> Cli {
    let env = TestEnv::new();
    let ctx = CliContext {
        paths: env.paths.clone(),
        updater: AppImageUpdater::new(),
    };
    Cli { env, ctx }
}

impl Cli {
    fn run_with(&self, args: &[&str], input: &str, interactive: bool) -> Output {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut stdin = Cursor::new(input.as_bytes().to_vec());
        let code = cli::run(
            &self.ctx,
            &args,
            &mut Io {
                out: &mut out,
                err: &mut err,
                input: &mut stdin,
                interactive,
            },
        );
        Output {
            code,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_with(args, "", false)
    }

    fn appimage(&self, name: &str, marker: &str) -> String {
        appimage_with_marker(&self.env.root.path().join(name), marker)
            .to_string_lossy()
            .to_string()
    }
}

fn field(out: &str, key: &str) -> String {
    out.lines()
        .find_map(|l| l.strip_prefix(key))
        .map(|v| v.trim_start_matches(':').trim().to_string())
        .unwrap_or_else(|| panic!("no field {} in:\n{}", key, out))
}

#[test]
fn help_and_version() {
    let c = cli();
    let help = c.run(&["--help"]);
    assert_eq!(help.code, 0);
    assert!(help.out.contains("appnest launch") || help.out.contains("launch <id>"));
    let ver = c.run(&["--version"]);
    assert_eq!(
        ver.out.trim(),
        format!("appnest {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn usage_errors_exit_with_status_two() {
    let c = cli();
    assert_eq!(c.run(&["frobnicate"]).code, 2);
    assert_eq!(c.run(&["import"]).code, 2);
    assert_eq!(c.run(&["import", "--bogus", "x"]).code, 2);
    assert_eq!(c.run(&["info"]).code, 2);
    assert_eq!(c.run(&["launch"]).code, 2);
    assert_eq!(c.run(&["replace", "only-one"]).code, 2);
}

#[test]
fn dispatch_ignores_files_and_empty_arguments() {
    assert_eq!(cli::dispatch(&[]), None);
    assert_eq!(
        cli::dispatch(&[OsString::from("/some/file.AppImage")]),
        None
    );
    assert_eq!(
        cli::dispatch(&[OsString::from("--gapplication-service")]),
        None
    );
}

#[test]
fn import_list_and_info() {
    let c = cli();
    assert!(c.run(&["list"]).out.contains("No applications."));

    let file = c.appimage("my-tool.AppImage", "a");
    let imported = c.run(&["import", &file]);
    assert_eq!(imported.code, 0, "{}{}", imported.out, imported.err);
    assert!(
        imported.out.contains("Imported my-tool as 'my-tool'"),
        "{}",
        imported.out
    );

    let list = c.run(&["list"]);
    assert!(
        list.out.contains("my-tool") && list.out.contains("ready"),
        "{}",
        list.out
    );

    let info = c.run(&["info", "my-tool"]);
    assert_eq!(info.code, 0);
    assert_eq!(field(&info.out, "Copied into library"), "yes");
    assert_eq!(field(&info.out, "Menu entry"), "yes");
    assert_eq!(c.run(&["info", "unknown"]).code, 1);
}

#[test]
fn import_options_control_copy_menu_and_shortcut() {
    let c = cli();
    let file = c.appimage("opts.AppImage", "a");
    assert_eq!(
        c.run(&["import", "--no-copy", "--no-menu", "--no-shortcut", &file])
            .code,
        0
    );
    let info = c.run(&["info", "opts"]).out;
    assert_eq!(field(&info, "Copied into library"), "no");
    assert_eq!(field(&info, "Menu entry"), "no");
    assert_eq!(field(&info, "Desktop shortcut"), "no");
}

#[test]
fn invalid_and_duplicate_imports_fail_with_status_one() {
    let c = cli();
    let fake = c.env.root.path().join("fake.AppImage");
    fs::copy("/bin/sh", &fake).unwrap();
    let res = c.run(&["import", fake.to_str().unwrap()]);
    assert_eq!(res.code, 1);
    assert!(res.err.contains("Not an AppImage"), "{}", res.err);

    let file = c.appimage("dup.AppImage", "a");
    assert_eq!(c.run(&["import", "--no-copy", &file]).code, 0);
    let again = c.run(&["import", "--no-copy", &file]);
    assert_eq!(again.code, 1);
    assert!(again.err.contains("already in AppNest"), "{}", again.err);
}

#[test]
fn launch_reports_success_and_failure_and_forwards_arguments() {
    let c = cli();
    let file = c.appimage("runner.AppImage", "a");
    assert_eq!(
        c.run(&["import", "--no-copy", "--no-menu", "--no-shortcut", &file])
            .code,
        0
    );

    let ok = c.run(&["launch", "runner", "--", "sh", "-c", "sleep 30; :"]);
    assert_eq!(ok.code, 0, "{}{}", ok.out, ok.err);
    assert!(ok.out.contains("Started runner (PID"), "{}", ok.out);
    assert!(c.run(&["list"]).out.contains("running"));

    // removing a running application closes it first
    let removed = c.run(&["remove", "-y", "runner"]);
    assert_eq!(removed.code, 0, "{}", removed.err);

    let file = c.appimage("broken.AppImage", "b");
    assert_eq!(
        c.run(&["import", "--no-copy", "--no-menu", "--no-shortcut", &file])
            .code,
        0
    );
    let bad = c.run(&[
        "launch",
        "broken",
        "--",
        "sh",
        "-c",
        "echo kaboom >&2; exit 3",
    ]);
    assert_eq!(bad.code, 1);
    assert!(bad.err.contains("kaboom"), "{}", bad.err);
}

#[test]
fn run_starts_a_file_without_adding_it() {
    let c = cli();
    let file = c.appimage("once.AppImage", "a");
    let res = c.run(&["run", &file, "--", "sh", "-c", "exit 0"]);
    assert_eq!(res.code, 0, "{}{}", res.out, res.err);
    assert!(c.run(&["list"]).out.contains("No applications."));

    let fake = c.env.root.path().join("fake.AppImage");
    fs::copy("/bin/sh", &fake).unwrap();
    assert_eq!(c.run(&["run", fake.to_str().unwrap()]).code, 1);
}

#[test]
fn remove_asks_for_confirmation() {
    let c = cli();
    let file = c.appimage("victim.AppImage", "a");
    assert_eq!(c.run(&["import", &file]).code, 0);

    // Not a terminal and no --yes: refuse
    assert_eq!(c.run(&["remove", "victim"]).code, 2);
    assert!(c.run(&["list"]).out.contains("victim"));

    // Interactive: "n" cancels, "y" removes
    assert_eq!(c.run_with(&["remove", "victim"], "n\n", true).code, 1);
    assert!(c.run(&["list"]).out.contains("victim"));
    let info = c.run(&["info", "victim"]).out;
    let copy = field(&info, "Path");
    assert!(std::path::Path::new(&copy).exists());

    assert_eq!(
        c.run_with(&["remove", "--delete-file", "victim"], "y\n", true)
            .code,
        0
    );
    assert!(c.run(&["list"]).out.contains("No applications."));
    assert!(
        !std::path::Path::new(&copy).exists(),
        "--delete-file removes the managed copy"
    );
    assert!(std::path::Path::new(&file).exists(), "the original is kept");
}

#[test]
fn scan_lists_only_valid_unadded_appimages() {
    let c = cli();
    let dir = c.env.dir("found");
    appimage_with_marker(&dir.join("new.AppImage"), "n");
    let known = appimage_with_marker(&dir.join("known.AppImage"), "k");
    fs::copy("/bin/sh", dir.join("fake.AppImage")).unwrap();
    assert_eq!(
        c.run(&["import", "--no-copy", known.to_str().unwrap()])
            .code,
        0
    );

    let res = c.run(&["scan", dir.to_str().unwrap()]);
    assert_eq!(res.code, 0);
    assert!(res.out.contains("new.AppImage"), "{}", res.out);
    assert!(
        !res.out.contains("known.AppImage") && !res.out.contains("fake.AppImage"),
        "{}",
        res.out
    );
}

#[test]
fn update_commands_explain_missing_update_information_and_replace_works() {
    let c = cli();
    let file = c.appimage("app.AppImage", "one");
    assert_eq!(c.run(&["import", "--no-copy", &file]).code, 0);

    let check = c.run(&["check-update", "app"]);
    assert_eq!(check.code, 0);
    assert!(check.out.contains("no update information"), "{}", check.out);
    assert_eq!(c.run(&["update", "-y", "app"]).code, 0);

    let newer = c.appimage("app-2.AppImage", "two");
    let replaced = c.run(&["replace", "app", &newer]);
    assert_eq!(replaced.code, 0, "{}{}", replaced.out, replaced.err);
    assert!(c.run(&["info", "app"]).out.contains("app-2.AppImage"));
    assert_eq!(
        c.run(&["replace", "app", &newer]).code,
        1,
        "same file again is refused"
    );
}

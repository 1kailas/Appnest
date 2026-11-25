//! Command-line interface. Runs without initializing GTK and shares all logic with the GUI
//! through the application layer.

use crate::application::commands::{
    ImportAppImageCommand, LaunchAppImageCommand, RemoveAppImageCommand, ReplaceAppImageCommand,
};
use crate::application::queries::ListAppImagesQuery;
use crate::config::paths::AppPaths;
use crate::domain::appimage::{AppImage, AppImageError};
use crate::domain::repository::AppImageRepository;
use crate::infrastructure::config_store::ConfigStore;
use crate::infrastructure::filesystem_repository::FilesystemAppImageRepository;
use crate::services::importer::AppImageImporter;
use crate::services::launcher::AppImageLauncher;
use crate::services::update_info::UpdateInfo;
use crate::services::updater::{AppImageUpdater, UpdateCheck};
use crate::services::validator::AppImageValidator;
use std::ffi::OsString;
use std::io::{BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

const COMMANDS: [&str; 10] = [
    "list",
    "info",
    "import",
    "launch",
    "run",
    "remove",
    "scan",
    "check-update",
    "update",
    "replace",
];

pub const USAGE: &str = "\
Usage: appnest [FILE...]            open the window (files are offered for import)
       appnest <command> [options]

Commands:
  list                              list applications and their state
  info <id>                         show details of an application
  import [options] <file>...        add AppImages to the library
      --no-copy                       reference the file instead of copying it
      --no-menu                       do not create an application menu entry
      --no-shortcut                   do not create a desktop shortcut
  launch <id> [-- args...]          start an application
  run <file> [-- args...]           run an AppImage once without adding it
  remove [options] <id>             remove an application
      --delete-file                   also delete the copied AppImage
      -y, --yes                       do not ask for confirmation
  scan [dir...]                     list AppImages found in the scan folders
  check-update <id>                 check the publisher's update information
  update [-y] <id>                  download and install an available update
  replace <id> <file>               replace an application with a newer AppImage file

Options:
  -h, --help                        show this help
  -V, --version                     show the version

Exit status: 0 success, 1 failure, 2 usage error.
";

pub struct Io<'a> {
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
    pub input: &'a mut dyn BufRead,
    pub interactive: bool,
}

pub struct CliContext {
    pub paths: AppPaths,
    pub updater: AppImageUpdater,
}

/// Runs a command-line invocation if `args` is one. Returns `None` when the arguments should
/// start the graphical interface instead (no arguments, or only files and GTK options).
pub fn dispatch(args: &[OsString]) -> Option<i32> {
    let first = args.first()?.to_str()?;
    let is_cli = COMMANDS.contains(&first)
        || matches!(
            first,
            "-h" | "--help" | "help" | "-V" | "--version" | "version"
        );
    if !is_cli {
        return None;
    }

    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let stdin = std::io::stdin();
    let interactive = stdin.is_terminal();
    let (mut out, mut err, mut input) = (stdout.lock(), stderr.lock(), stdin.lock());
    let mut io = Io {
        out: &mut out,
        err: &mut err,
        input: &mut input,
        interactive,
    };

    if matches!(
        first,
        "-h" | "--help" | "help" | "-V" | "--version" | "version"
    ) {
        return Some(run_info(first, &mut io));
    }
    let Some(paths) = AppPaths::try_new() else {
        let _ = writeln!(io.err, "appnest: cannot determine the home directory");
        return Some(1);
    };
    let ctx = CliContext {
        paths,
        updater: AppImageUpdater::new(),
    };
    Some(run(&ctx, args, &mut io))
}

fn run_info(first: &str, io: &mut Io) -> i32 {
    let _ = match first {
        "-V" | "--version" | "version" => writeln!(io.out, "appnest {}", env!("CARGO_PKG_VERSION")),
        _ => write!(io.out, "{}", USAGE),
    };
    0
}

fn usage_error(io: &mut Io, msg: &str) -> i32 {
    let _ = writeln!(io.err, "appnest: {}\nTry 'appnest --help'.", msg);
    2
}

/// Splits arguments at `--`: (before, after).
fn split_forwarded(args: &[OsString]) -> (&[OsString], &[OsString]) {
    match args.iter().position(|a| a == "--") {
        Some(i) => (&args[..i], &args[i + 1..]),
        None => (args, &[]),
    }
}

struct Services {
    repo: Arc<dyn AppImageRepository>,
    launcher: Arc<AppImageLauncher>,
    config: ConfigStore,
}

impl Services {
    fn new(ctx: &CliContext) -> Self {
        let _ = ctx.paths.ensure_dirs();
        Self {
            repo: Arc::new(FilesystemAppImageRepository::new(ctx.paths.clone())),
            launcher: Arc::new(AppImageLauncher::new(ctx.paths.clone())),
            config: ConfigStore::new(ctx.paths.clone()),
        }
    }

    fn find(&self, id: &str) -> Result<AppImage, AppImageError> {
        self.repo
            .get_by_id(id)?
            .ok_or_else(|| AppImageError::NotFound(PathBuf::from(id)))
    }
}

pub fn run(ctx: &CliContext, args: &[OsString], io: &mut Io) -> i32 {
    let Some(command) = args.first().and_then(|a| a.to_str()) else {
        return usage_error(io, "missing command");
    };
    let rest = &args[1..];
    if matches!(
        command,
        "-h" | "--help" | "help" | "-V" | "--version" | "version"
    ) {
        return run_info(command, io);
    }

    let services = Services::new(ctx);
    let result = match command {
        "list" => cmd_list(&services, io),
        "info" => cmd_info(&services, rest, io),
        "import" => cmd_import(ctx, &services, rest, io),
        "launch" => cmd_launch(&services, rest, io),
        "run" => cmd_run(&services, rest, io),
        "remove" => cmd_remove(ctx, &services, rest, io),
        "scan" => cmd_scan(ctx, &services, rest, io),
        "check-update" => cmd_check_update(ctx, &services, rest, io),
        "update" => cmd_update(ctx, &services, rest, io),
        "replace" => cmd_replace(ctx, &services, rest, io),
        other => return usage_error(io, &format!("unknown command '{}'", other)),
    };
    match result {
        Ok(code) => code,
        Err(Failure::Usage(msg)) => usage_error(io, &msg),
        Err(Failure::Error(e)) => {
            let _ = writeln!(io.err, "appnest: {}", e);
            1
        }
    }
}

enum Failure {
    Usage(String),
    Error(AppImageError),
}

impl From<AppImageError> for Failure {
    fn from(e: AppImageError) -> Self {
        Failure::Error(e)
    }
}

type CmdResult = Result<i32, Failure>;

fn usage(msg: &str) -> Failure {
    Failure::Usage(msg.to_string())
}

fn one_id(rest: &[OsString], what: &str) -> Result<String, Failure> {
    match rest {
        [id] => id
            .to_str()
            .map(str::to_string)
            .ok_or_else(|| usage("invalid application id")),
        [] => Err(usage(&format!("{} requires an application id", what))),
        _ => Err(usage(&format!("{} takes exactly one application id", what))),
    }
}

fn cmd_list(s: &Services, io: &mut Io) -> CmdResult {
    let apps = s.repo.list()?;
    if apps.is_empty() {
        let _ = writeln!(io.out, "No applications.");
        return Ok(0);
    }
    let running = s.launcher.refresh_running(&apps);
    let id_w = apps.iter().map(|a| a.id.len()).max().unwrap_or(2).max(2);
    let name_w = apps
        .iter()
        .map(|a| a.name.chars().count())
        .max()
        .unwrap_or(4)
        .max(4);
    let _ = writeln!(
        io.out,
        "{:<id_w$}  {:<name_w$}  {:<10}  STATUS",
        "ID", "NAME", "VERSION"
    );
    for a in &apps {
        let status = if !a.exists() {
            "missing"
        } else if running.contains(&a.id) {
            "running"
        } else {
            "ready"
        };
        let _ = writeln!(
            io.out,
            "{:<id_w$}  {:<name_w$}  {:<10}  {}",
            a.id,
            a.name,
            a.version.as_deref().unwrap_or("-"),
            status
        );
    }
    Ok(0)
}

fn yes_no(v: bool) -> &'static str {
    if v {
        "yes"
    } else {
        "no"
    }
}

fn cmd_info(s: &Services, rest: &[OsString], io: &mut Io) -> CmdResult {
    let id = one_id(rest, "info")?;
    let app = s.find(&id)?;
    let running = s
        .launcher
        .refresh_running(std::slice::from_ref(&app))
        .contains(&app.id);
    let updates = match UpdateInfo::read(&app.path) {
        None => "none".to_string(),
        Some(UpdateInfo::Unsupported(kind)) => format!("unsupported ({})", kind),
        Some(_) => "available".to_string(),
    };
    let rows = [
        ("ID", app.id.clone()),
        ("Name", app.name.clone()),
        ("Version", app.version.clone().unwrap_or_else(|| "-".into())),
        ("Path", app.path.display().to_string()),
        ("Size", app.formatted_size()),
        ("Type", app.appimage_type.to_string()),
        ("Architecture", app.architecture.to_string()),
        ("Launch method", app.runtime_method.to_string()),
        ("Copied into library", yes_no(app.managed).to_string()),
        ("Menu entry", yes_no(app.desktop_integrated).to_string()),
        ("Desktop shortcut", yes_no(app.desktop_shortcut).to_string()),
        ("Extracted AppDir", yes_no(app.is_extracted()).to_string()),
        (
            "SHA-256",
            app.cached_sha256().unwrap_or("not computed").to_string(),
        ),
        ("Update information", updates),
        (
            "Status",
            if !app.exists() {
                "file missing"
            } else if running {
                "running"
            } else {
                "ready"
            }
            .to_string(),
        ),
    ];
    for (k, v) in rows {
        let _ = writeln!(io.out, "{:<20} {}", format!("{}:", k), v);
    }
    Ok(0)
}

fn cmd_import(ctx: &CliContext, s: &Services, rest: &[OsString], io: &mut Io) -> CmdResult {
    let mut settings = s.config.load();
    let mut files = Vec::new();
    for arg in rest {
        match arg.to_str() {
            Some("--no-copy") => settings.copy_on_import = false,
            Some("--no-menu") => settings.auto_integrate_desktop = false,
            Some("--no-shortcut") => settings.auto_desktop_shortcut = false,
            Some(o) if o.starts_with('-') => return Err(usage(&format!("unknown option '{}'", o))),
            _ => files.push(PathBuf::from(arg)),
        }
    }
    if files.is_empty() {
        return Err(usage("import requires at least one file"));
    }

    let importer = Arc::new(AppImageImporter::new(ctx.paths.clone()));
    let cmd = ImportAppImageCommand::new(Arc::clone(&s.repo), importer);
    let mut failed = false;
    for file in files {
        match cmd.execute(&file, &settings) {
            Ok(app) => {
                let _ = writeln!(io.out, "Imported {} as '{}'", app.name, app.id);
            }
            Err(e) => {
                failed = true;
                let _ = writeln!(io.err, "appnest: {}: {}", file.display(), e);
            }
        }
    }
    Ok(i32::from(failed))
}

fn cmd_launch(s: &Services, rest: &[OsString], io: &mut Io) -> CmdResult {
    let (opts, forwarded) = split_forwarded(rest);
    let id = one_id(opts, "launch")?;
    let cmd = LaunchAppImageCommand::new(Arc::clone(&s.repo), Arc::clone(&s.launcher));
    let app = s.find(&id)?;
    let pid = cmd.execute(&id, forwarded)?;
    let _ = writeln!(io.out, "Started {} (PID {})", app.name, pid);
    Ok(0)
}

fn cmd_run(s: &Services, rest: &[OsString], io: &mut Io) -> CmdResult {
    let (opts, forwarded) = split_forwarded(rest);
    let [file] = opts else {
        return Err(usage("run requires exactly one AppImage file"));
    };
    let path = std::path::absolute(file).map_err(AppImageError::from)?;
    AppImageValidator::validate(&path)?;
    let name = path
        .file_stem()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "AppImage".into());
    let mut app = AppImage::new(
        format!("run-once-{}", AppImage::sanitize_id(&name)),
        &name,
        path,
    );
    let pid = s.launcher.launch(&mut app, forwarded)?;
    let _ = writeln!(io.out, "Started {} (PID {})", name, pid);
    Ok(0)
}

fn confirm(io: &mut Io, question: &str) -> Result<bool, Failure> {
    if !io.interactive {
        return Err(usage(
            "this needs confirmation; pass --yes to proceed non-interactively",
        ));
    }
    let _ = write!(io.out, "{} [y/N] ", question);
    let _ = io.out.flush();
    let mut line = String::new();
    io.input.read_line(&mut line).map_err(AppImageError::from)?;
    Ok(matches!(line.trim().to_lowercase().as_str(), "y" | "yes"))
}

fn cmd_remove(ctx: &CliContext, s: &Services, rest: &[OsString], io: &mut Io) -> CmdResult {
    let (mut delete_file, mut yes) = (false, false);
    let mut ids = Vec::new();
    for arg in rest {
        match arg.to_str() {
            Some("--delete-file") => delete_file = true,
            Some("-y") | Some("--yes") => yes = true,
            Some(o) if o.starts_with('-') => return Err(usage(&format!("unknown option '{}'", o))),
            _ => ids.push(arg.clone()),
        }
    }
    let id = one_id(&ids, "remove")?;
    let app = s.find(&id)?;
    if !yes {
        let extra = if delete_file && app.managed {
            " and delete its AppImage file"
        } else {
            ""
        };
        if !confirm(io, &format!("Remove {}{}?", app.name, extra))? {
            let _ = writeln!(io.out, "Cancelled.");
            return Ok(1);
        }
    }
    RemoveAppImageCommand::new(
        Arc::clone(&s.repo),
        ctx.paths.clone(),
        Arc::clone(&s.launcher),
    )
    .execute(&id, delete_file)?;
    let _ = writeln!(io.out, "Removed {}", app.name);
    Ok(0)
}

fn cmd_scan(ctx: &CliContext, s: &Services, rest: &[OsString], io: &mut Io) -> CmdResult {
    let dirs: Vec<PathBuf> = if rest.is_empty() {
        let configured = s.config.load().scan_directories;
        if configured.is_empty() {
            ctx.paths.default_scan_directories()
        } else {
            configured
        }
    } else {
        rest.iter().map(PathBuf::from).collect()
    };
    let listing = ListAppImagesQuery::new(Arc::clone(&s.repo)).execute(&dirs)?;
    if listing.unmanaged_discovered.is_empty() {
        let _ = writeln!(io.out, "No new AppImages found.");
    }
    for p in listing.unmanaged_discovered {
        let _ = writeln!(io.out, "{}", p.display());
    }
    Ok(0)
}

fn describe(check: &UpdateCheck, name: &str) -> String {
    match check {
        UpdateCheck::NoUpdateInformation => {
            format!(
                "{} has no update information. Use 'appnest replace' with a newer file.",
                name
            )
        }
        UpdateCheck::Unsupported(kind) => {
            format!("{} uses an unsupported update method ({}).", name, kind)
        }
        UpdateCheck::UpToDate => format!("{} is up to date.", name),
        UpdateCheck::Available(u) => format!(
            "Update available for {}: {}{} ({})",
            name,
            if u.filename.is_empty() {
                "new version"
            } else {
                &u.filename
            },
            u.release
                .as_ref()
                .map(|r| format!(", release {}", r))
                .unwrap_or_default(),
            if u.length > 0 {
                format!("{:.1} MB", u.length as f64 / 1_048_576.0)
            } else {
                "size unknown".into()
            }
        ),
    }
}

fn cmd_check_update(ctx: &CliContext, s: &Services, rest: &[OsString], io: &mut Io) -> CmdResult {
    let app = s.find(&one_id(rest, "check-update")?)?;
    let check = ctx.updater.check(&app.path)?;
    let _ = writeln!(io.out, "{}", describe(&check, &app.name));
    Ok(0)
}

fn cmd_update(ctx: &CliContext, s: &Services, rest: &[OsString], io: &mut Io) -> CmdResult {
    let mut yes = false;
    let mut ids = Vec::new();
    for arg in rest {
        match arg.to_str() {
            Some("-y") | Some("--yes") => yes = true,
            Some(o) if o.starts_with('-') => return Err(usage(&format!("unknown option '{}'", o))),
            _ => ids.push(arg.clone()),
        }
    }
    let app = s.find(&one_id(&ids, "update")?)?;
    let check = ctx.updater.check(&app.path)?;
    let _ = writeln!(io.out, "{}", describe(&check, &app.name));
    let UpdateCheck::Available(update) = check else {
        return Ok(0);
    };
    if !yes && !confirm(io, "Download and install it?")? {
        let _ = writeln!(io.out, "Cancelled.");
        return Ok(1);
    }

    let dest = AppImageUpdater::new_download_path(&ctx.paths.cache_dir.join("updates"))
        .map_err(AppImageError::from)?;
    let mut last_pct = u64::MAX;
    let cancel = AtomicBool::new(false);
    let downloaded = ctx.updater.download(
        &update,
        &dest,
        &mut |done, total| {
            if total > 0 && io.interactive {
                let pct = done * 100 / total;
                if pct != last_pct {
                    last_pct = pct;
                    let _ = write!(io.err, "\rDownloading... {}%", pct);
                    let _ = io.err.flush();
                }
            }
        },
        &cancel,
    );
    if io.interactive {
        let _ = writeln!(io.err);
    }
    downloaded?;

    let replace = ReplaceAppImageCommand::new(
        Arc::clone(&s.repo),
        Arc::clone(&s.launcher),
        ctx.paths.clone(),
    );
    let result = replace.execute(&app.id, &dest);
    let _ = std::fs::remove_file(&dest);
    let updated = result?;
    let _ = writeln!(
        io.out,
        "Updated {} ({})",
        updated.name,
        updated.version.as_deref().unwrap_or("new version")
    );
    Ok(0)
}

fn cmd_replace(ctx: &CliContext, s: &Services, rest: &[OsString], io: &mut Io) -> CmdResult {
    let [id, file] = rest else {
        return Err(usage("replace requires an application id and a file"));
    };
    let id = id.to_str().ok_or_else(|| usage("invalid application id"))?;
    let updated = ReplaceAppImageCommand::new(
        Arc::clone(&s.repo),
        Arc::clone(&s.launcher),
        ctx.paths.clone(),
    )
    .execute(id, &PathBuf::from(file))?;
    let _ = writeln!(io.out, "Replaced {}", updated.name);
    Ok(0)
}

//! LaTeX compilation: engine detection plus the `LatexCompile` job runner
//! (latexmk orchestration with live log streaming, progress and cancel).

use serde::Deserialize;
use serde::Serialize;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Emitter;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;

use crate::core::error::ApiResult;
use crate::features::jobs::emit_job_changed;
use crate::features::jobs::JobCenter;
use crate::features::jobs::JobKind;
use crate::features::jobs::RunOutcome;
use crate::features::jobs::StartedJob;

/// A detected LaTeX rendering engine.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LatexEngine {
    pub id: String,
    pub label: String,
    pub path: Option<String>,
}

/// Directories that ship TeX Live binaries on a default install. Searched in
/// addition to `$PATH` so engines like MacTeX (which install to
/// `/Library/TeX/texbin` but do NOT put that on user `$PATH` by default) still
/// surface to the picker.
fn tex_extra_paths() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    // macOS — MacTeX default symlink (covers the active TeX Live year).
    out.push(PathBuf::from("/Library/TeX/texbin"));
    // macOS — older MacTeX releases expose a year-suffixed tree.
    if let Ok(entries) = std::fs::read_dir("/usr/local/texlive") {
        for entry in entries.flatten() {
            let bin = entry.path().join("bin").join("universal-darwin");
            if bin.is_dir() {
                out.push(bin);
            }
            let bin_arm = entry.path().join("bin").join("arm64-darwin");
            if bin_arm.is_dir() {
                out.push(bin_arm);
            }
            let bin_x86 = entry.path().join("bin").join("x86_64-darwin");
            if bin_x86.is_dir() {
                out.push(bin_x86);
            }
        }
    }
    // Linux / cross-platform fallbacks.
    out.push(PathBuf::from("/usr/bin"));
    out.push(PathBuf::from("/usr/local/bin"));
    // Common Homebrew locations on Apple Silicon / Intel.
    out.push(PathBuf::from("/opt/homebrew/bin"));
    out.push(PathBuf::from("/usr/local/bin"));
    out
}

/// Resolve `command` by combining `$PATH` with platform-specific TeX locations.
fn resolve_engine(command: &str) -> Option<PathBuf> {
    if let Ok(path) = which::which(command) {
        return Some(path);
    }
    for dir in tex_extra_paths() {
        let candidate = dir.join(command);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// The engine picker: latexmk orchestrates every engine (bibtex/biber and the
/// rerun loop run automatically), so the entries are the latexmk flags.
const LATEX_ENGINES: [(&str, &str); 3] = [
    ("pdflatex", "PDFLaTeX"),
    ("xelatex", "XeLaTeX"),
    ("lualatex", "LuaLaTeX"),
];

fn latexmk_engine_flag(engine: &str) -> Option<&'static str> {
    match engine {
        "pdflatex" => Some("-pdf"),
        "xelatex" => Some("-xelatex"),
        "lualatex" => Some("-lualatex"),
        _ => None,
    }
}

/// Detect available LaTeX engines on the system.
/// Returns the engines that can actually compile — latexmk (the orchestrator)
/// and the engine binary must both exist. Engines not present on the host are
/// omitted (not greyed out); without latexmk the list is empty and the compile
/// button stays hidden.
#[tauri::command]
#[specta::specta]
pub async fn detect_latex_engines() -> ApiResult<Vec<LatexEngine>> {
    // latexmk drives the whole build (engine, bibtex, reruns); without it
    // there is nothing to offer.
    let Some(latexmk) = resolve_engine("latexmk") else {
        return ApiResult::ok(Vec::new());
    };

    let engines = LATEX_ENGINES
        .iter()
        .filter(|(engine, _)| resolve_engine(engine).is_some())
        .map(|(engine, label)| LatexEngine {
            id: (*engine).to_string(),
            label: (*label).to_string(),
            path: Some(latexmk.to_string_lossy().to_string()),
        })
        .collect::<Vec<_>>();

    log::debug!("detected LaTeX engines: {:?}", engines);
    ApiResult::ok(engines)
}

/// `params` payload of a `LatexCompile` job.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LatexCompileParams {
    tex_path: String,
    engine: String,
}

/// Register the `LatexCompile` runner with the JobCenter (app assembly).
pub fn register_job_runners(center: &JobCenter) {
    center.register_runner(JobKind::LatexCompile, Arc::new(latex_compile_runner));
}

/// Runner for [`JobKind::LatexCompile`]: run latexmk on the .tex source and
/// stream its output live. Milestone lines (rule / run-number banners) become
/// `job:changed` progress so the background-tasks row advances; every line is
/// also emitted on `compile:log` for a future log view.
fn latex_compile_runner(
    center: JobCenter,
    app: tauri::AppHandle,
    started: StartedJob,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    center.run_job(app, started, |center, app, started| async move {
        let params = started
            .snapshot
            .params
            .clone()
            .and_then(|value| serde_json::from_value::<LatexCompileParams>(value).ok());
        let Some(params) = params else {
            return RunOutcome::Failed(Some("latex compile job is missing its params".into()));
        };
        run_latexmk(&center, &app, &started, params).await
    })
}

/// Monotonic progress filter: milestones only surface when they move the bar
/// or rename the phase, so `job:changed` stays quiet on ordinary log lines.
#[derive(Default)]
struct CompileProgress {
    max: f32,
}

impl CompileProgress {
    fn advance(&mut self, progress: f32, phase: String) -> Option<(f32, String)> {
        if progress <= self.max {
            return None;
        }
        self.max = progress;
        Some((self.max, phase))
    }
}

/// Map a latexmk banner line to a (progress, phase) milestone, if it is one.
/// Engine runs advance 0.35 → 0.85; bibliography rules sit at 0.5; the
/// terminal 100 is set by `run_job` on success.
fn line_milestone(line: &str) -> Option<(f32, String)> {
    if let Some(rest) = line.strip_prefix("Run number ") {
        // "Run number 2 of rule 'pdflatex'"
        let mut parts = rest.splitn(2, " of rule '");
        let runs: u32 = parts.next()?.trim().parse().ok()?;
        let rule = parts.next()?.trim_end_matches('\'');
        let progress = (0.1 + 0.25 * runs as f32).min(0.85);
        return Some((progress, format!("{rule} · run {runs}")));
    }
    if let Some(rest) = line.strip_prefix("Latexmk: applying rule '") {
        let rule = rest.trim_end_matches("'...");
        let progress = match rule {
            "bibtex" | "biber" | "makeindex" => 0.5,
            _ => 0.15,
        };
        return Some((progress, rule.to_string()));
    }
    None
}

/// Last few output lines, kept to build a useful failure message (LaTeX errors
/// start with `! `; those win over the plain tail).
const ERROR_TAIL_LINES: usize = 24;

/// Pump a child pipe into the shared line stream until it ends.
async fn forward_lines<S>(stream: S, tx: tokio::sync::mpsc::UnboundedSender<String>)
where
    S: tokio::io::AsyncRead + Unpin,
{
    let mut lines = BufReader::new(stream).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if tx.send(line).is_err() {
            break;
        }
    }
}

async fn run_latexmk(
    center: &JobCenter,
    app: &tauri::AppHandle,
    started: &StartedJob,
    params: LatexCompileParams,
) -> RunOutcome {
    let tex_path = PathBuf::from(&params.tex_path);
    if !tex_path.is_file() {
        return RunOutcome::Failed(Some(format!("tex file not found: {}", tex_path.display())));
    }
    let Some(cwd) = tex_path.parent().map(Path::to_path_buf) else {
        return RunOutcome::Failed(Some("cannot determine parent directory".into()));
    };
    let Some(basename) = tex_path
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
    else {
        return RunOutcome::Failed(Some("invalid tex file name".into()));
    };

    log::info!(
        "compiling tex: {} with engine {} in {}",
        basename,
        params.engine,
        cwd.display()
    );

    let Some(engine_flag) = latexmk_engine_flag(&params.engine) else {
        return RunOutcome::Failed(Some(format!("unknown latex engine: {}", params.engine)));
    };
    // GUI apps inherit launchd's minimal PATH (no /Library/TeX/texbin), so a
    // bare latexmk would fail to spawn even though detection found it. Resolve
    // the same way detect_latex_engines does and spawn the absolute path.
    let Some(latexmk) = resolve_engine("latexmk") else {
        return RunOutcome::Failed(Some("latexmk not found on this system".into()));
    };

    let mut cmd = tokio::process::Command::new(&latexmk);
    cmd.current_dir(&cwd)
        .arg(engine_flag)
        .arg("-interaction=nonstopmode")
        .arg("-halt-on-error")
        .arg(format!("-outdir={}", cwd.to_string_lossy()))
        .arg(&basename)
        .kill_on_drop(true)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    // latexmk locates the engine and bibtex via the child $PATH, which under a
    // GUI app is launchd's minimal one. Put latexmk's own bin dir (TeX Live
    // keeps every engine there) in front of the inherited PATH.
    if let Some(bin_dir) = latexmk.parent() {
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        let mut path_env = std::ffi::OsString::from(bin_dir);
        path_env.push(":");
        path_env.push(inherited);
        cmd.env("PATH", path_env);
    }

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            return RunOutcome::Failed(Some(format!("failed to run {}: {}", params.engine, e)))
        }
    };

    // Drain stdout and stderr concurrently into one line stream so neither
    // pipe can wedge the child while the main loop consumes lines.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    if let Some(stdout) = child.stdout.take() {
        let tx = tx.clone();
        tokio::spawn(forward_lines(stdout, tx));
    }
    if let Some(stderr) = child.stderr.take() {
        let tx = tx.clone();
        tokio::spawn(forward_lines(stderr, tx));
    }
    drop(tx);

    let job_id = started.snapshot.id.clone();
    let cancel_token = started.cancel_token.clone();
    let mut progress = CompileProgress::default();
    let mut tail: Vec<String> = Vec::new();
    let mut cancelled = false;

    loop {
        tokio::select! {
            biased;
            _ = cancel_token.cancelled() => {
                cancelled = true;
                break;
            }
            line = rx.recv() => {
                let Some(line) = line else { break };
                let _ = app.emit("compile:log", serde_json::json!({ "line": line }));
                if tail.len() == ERROR_TAIL_LINES {
                    tail.remove(0);
                }
                tail.push(line.clone());
                if let Some((progress_value, phase)) = line_milestone(&line)
                    .and_then(|(p, phase)| progress.advance(p, phase))
                {
                    if let Some(snapshot) = center
                        .job_report(&job_id, Some(progress_value), Some(phase), None, None)
                        .await
                    {
                        emit_job_changed(app, snapshot);
                    }
                }
            }
        }
    }

    if cancelled {
        let _ = child.kill().await;
        return RunOutcome::Cancelled;
    }

    let status = match child.wait().await {
        Ok(status) => status,
        Err(e) => return RunOutcome::Failed(Some(format!("failed to run latexmk: {e}"))),
    };

    let pdf_path = tex_path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|stem| cwd.join(format!("{stem}.pdf")))
        .unwrap_or_else(|| cwd.join("output.pdf"));

    if status.success() && pdf_path.exists() {
        return RunOutcome::Succeeded;
    }

    log::warn!(
        "compile failed for {}: exit={:?}",
        tex_path.display(),
        status.code()
    );
    let errors: Vec<String> = tail
        .iter()
        .filter(|l| l.starts_with('!'))
        .cloned()
        .collect();
    let detail = if errors.is_empty() {
        tail.join("\n")
    } else {
        errors.join("\n")
    };
    let detail = if detail.len() > 2000 {
        format!("{}…", detail[detail.len() - 2000..].trim_start())
    } else if detail.is_empty() {
        format!("latexmk exited with {status}")
    } else {
        detail
    };
    RunOutcome::Failed(Some(detail))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_run_number_banners_to_monotonic_milestones() {
        let mut progress = CompileProgress::default();
        let first = line_milestone("Run number 1 of rule 'pdflatex'").unwrap();
        assert_eq!(first, (0.35, "pdflatex · run 1".to_string()));
        assert_eq!(
            progress.advance(first.0, first.1.clone()),
            Some(first.clone())
        );

        // Same run again (rerun banner) must not move the bar.
        assert_eq!(progress.advance(first.0, first.1), None);

        let second = line_milestone("Run number 2 of rule 'pdflatex'").unwrap();
        assert_eq!(second.0, 0.6);
        assert_eq!(
            progress.advance(second.0, second.1.clone()),
            Some(second.clone())
        );
        // Bibliography rules sit below engine run 2 — monotonic filter drops them.
        let bib = line_milestone("Latexmk: applying rule 'bibtex'...").unwrap();
        assert_eq!(bib, (0.5, "bibtex".to_string()));
        assert_eq!(progress.advance(bib.0, bib.1), None);
    }

    #[test]
    fn caps_engine_run_progress_below_terminal() {
        let (progress, phase) = line_milestone("Run number 9 of rule 'lualatex'").unwrap();
        assert_eq!(progress, 0.85);
        assert_eq!(phase, "lualatex · run 9");
    }

    #[test]
    fn ordinary_log_lines_are_not_milestones() {
        assert!(line_milestone("This is pdfTeX, Version 3.141592653").is_none());
        assert!(line_milestone("[1] [2] [3]").is_none());
        assert!(line_milestone("Latexmk: All targets () are up-to-date").is_none());
    }
}

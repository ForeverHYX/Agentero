use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::Emitter;

use crate::core::error::ApiResult;

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

/// LaTeX engines offered in the picker. Every entry compiles through latexmk,
/// which orchestrates the engine runs plus bibtex/biber and reruns until the
/// document settles (engine → bibtex → engine → engine).
const LATEX_ENGINES: &[(&str, &str)] = &[
    ("pdflatex", "PDFLaTeX"),
    ("xelatex", "XeLaTeX"),
    ("lualatex", "LuaLaTeX"),
];

/// Map a picker engine id to its latexmk engine flag.
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
    // Everything compiles through latexmk, so it gates the whole feature.
    let Some(latexmk) = resolve_engine("latexmk") else {
        log::debug!("latexmk not found; tex compile unavailable");
        return ApiResult::ok(Vec::new());
    };

    let mut engines = Vec::new();

    for (id, label) in LATEX_ENGINES {
        if resolve_engine(id).is_some() {
            engines.push(LatexEngine {
                id: id.to_string(),
                label: label.to_string(),
                // The executable actually spawned for every entry is latexmk.
                path: Some(latexmk.to_string_lossy().to_string()),
            });
        }
    }

    log::debug!("detected LaTeX engines: {:?}", engines);
    ApiResult::ok(engines)
}

/// Result of a TeX compilation.
#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CompileResult {
    pub ok: bool,
    pub pdf_path: Option<String>,
    pub log: String,
}

/// Compile a .tex file to PDF using the specified engine.
#[tauri::command]
#[specta::specta]
pub async fn compile_tex(
    tex_path: String,
    engine: String,
    app_handle: tauri::AppHandle,
) -> ApiResult<CompileResult> {
    let tex_path = Path::new(&tex_path);
    if !tex_path.exists() {
        return ApiResult::err(crate::core::error::AppError::message(format!(
            "tex file not found: {}",
            tex_path.display()
        )));
    }

    let cwd = match tex_path.parent() {
        Some(p) => p,
        None => {
            return ApiResult::err(crate::core::error::AppError::message(
                "cannot determine parent directory",
            ))
        }
    };
    let basename = match tex_path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => {
            return ApiResult::err(crate::core::error::AppError::message(
                "invalid tex file name",
            ))
        }
    };

    log::info!(
        "compiling tex: {} with engine {} in {}",
        basename,
        engine,
        cwd.display()
    );

    let Some(engine_flag) = latexmk_engine_flag(&engine) else {
        return ApiResult::err(crate::core::error::AppError::message(format!(
            "unknown latex engine: {}",
            engine
        )));
    };

    // GUI apps inherit launchd's minimal PATH (no /Library/TeX/texbin), so a
    // bare latexmk would fail to spawn even though detection found it. Resolve
    // the same way detect_latex_engines does and spawn the absolute path.
    // latexmk runs the selected engine, bibtex/biber, and reruns automatically
    // until cross-references and citations settle.
    let latexmk = match resolve_engine("latexmk") {
        Some(p) => p,
        None => {
            return ApiResult::err(crate::core::error::AppError::message(
                "latexmk not found on this system",
            ))
        }
    };

    let mut cmd = tokio::process::Command::new(&latexmk);
    cmd.current_dir(cwd)
        .arg(engine_flag)
        .arg("-interaction=nonstopmode")
        .arg("-halt-on-error")
        .arg(format!("-outdir={}", cwd.to_string_lossy()))
        .arg(basename);

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

    let output = match cmd.output().await {
        Ok(o) => o,
        Err(e) => {
            return ApiResult::err(crate::core::error::AppError::message(format!(
                "failed to run {}: {}",
                engine, e
            )))
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined_log = format!("{}\n{}", stdout, stderr);

    for line in combined_log.lines() {
        let _ = app_handle.emit("compile:log", serde_json::json!({ "line": line }));
    }

    let pdf_basename = tex_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let pdf_path = cwd.join(format!("{}.pdf", pdf_basename));

    let ok = output.status.success() && pdf_path.exists();

    if !ok {
        log::warn!(
            "compile failed for {}: exit={:?}",
            tex_path.display(),
            output.status
        );
    }

    ApiResult::ok(CompileResult {
        ok,
        pdf_path: if ok {
            Some(pdf_path.to_string_lossy().to_string())
        } else {
            None
        },
        log: combined_log,
    })
}

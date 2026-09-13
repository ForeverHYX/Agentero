//! `agentero describe` — curated op introspection for agents.

use crate::error::CliError;
use crate::output::to_value;
use crate::resolve::GlobalOpts;
use agentero_core::ops;
use serde_json::{json, Value};

pub fn run(op: Option<&str>, globals: &GlobalOpts) -> Result<Value, CliError> {
    match op.map(str::trim).filter(|s| !s.is_empty()) {
        None => {
            let summaries = ops::summaries();
            let style = globals.style;
            let mut v = json!({ "ops": to_value(&summaries)? });
            if let Some(obj) = v.as_object_mut() {
                let mut lines = vec![format!(
                    "{} {} curated ops — pass an id for full input/output schema",
                    style.key("describe"),
                    summaries.len()
                )];
                for s in &summaries {
                    let mcp = s
                        .mcp_tool
                        .as_deref()
                        .map(|t| format!(" mcp:{t}"))
                        .unwrap_or_default();
                    lines.push(format!(
                        "  {}  {}{}",
                        style.bright_blue(&s.id),
                        s.summary,
                        style.dim(&mcp)
                    ));
                }
                obj.insert("lines".into(), json!(lines));
            }
            Ok(v)
        }
        Some(id) => match ops::get(id) {
            Some(spec) => {
                let style = globals.style;
                let mut v = to_value(spec)?;
                if let Some(obj) = v.as_object_mut() {
                    let mut lines = vec![format!(
                        "{} {}",
                        style.key("op"),
                        style.bright_blue(&spec.id)
                    )];
                    lines.push(format!("  {}", spec.summary));
                    if let Some(cli) = &spec.cli {
                        lines.push(format!("  {} {}", style.key("cli"), cli));
                    }
                    if let Some(mcp) = &spec.mcp_tool {
                        lines.push(format!("  {} {}", style.key("mcp"), mcp));
                    }
                    if !spec.examples.is_empty() {
                        lines.push(format!("  {}", style.key("examples")));
                        for ex in &spec.examples {
                            lines.push(format!("    {ex}"));
                        }
                    }
                    obj.insert("lines".into(), json!(lines));
                }
                Ok(v)
            }
            None => {
                let suggestions = ops::suggest(id);
                let hint = if suggestions.is_empty() {
                    format!("unknown op '{id}' — run `agentero describe --json` for the catalog")
                } else {
                    format!(
                        "unknown op '{id}' — did you mean: {}?",
                        suggestions.join(", ")
                    )
                };
                Err(CliError::usage(hint))
            }
        },
    }
}

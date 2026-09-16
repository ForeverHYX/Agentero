# AGENTS.md

This file is the L0 map for agents working in this Agentero research vault.

## Layout

- `papers/` — paper folders (any depth). A **paper folder** is the minimal unit:

```text
papers/<id>/
├── NOTES.md          # human / agent working notes
├── metadata.json     # catalog row projection (do not invent by hand)
├── <id>.pdf          # optional main PDF
├── PAPER.md          # derived body when no TeX (regenerable)
├── source/           # TeX / e-print (do not dump extras here)
├── marks/            # reading annotations (prefer CLI `mark`, not hand-edits)
├── assets/           # images embedded from NOTES.md
└── attachments/      # supporting materials only (supplements, slides, code)
```

  Create `attachments/` only when adding files. Do not put extras at the paper
  root or into `source/`. Do not invent empty `attachments/` folders.

- `notes/` — free-form concept notes (`[[wikilinks]]`, embeds, Mermaid, callouts).
- `data/` — datasets and artifacts synced from a remote server; keep them out of `papers/`.
- `thesis/` — LaTeX manuscript workspace; `thesis/main.tex` is a minimal starter.
- `.agents/` — vault-local skills (`skills/<id>/SKILL.md`).

## Paper reading order

For a paper folder, use the richest available source in this order:

1. `source/**/*.{tex,ltx}` — prefer for structure, equations, citations, experiments
2. `{paper}/PAPER.md` - text parsed by agentero
3. If neither, use agentero CLI to parse paper, then read the generated `PAPER.md`
4. Local PDF under the paper folder — last resort. If methods above work, DO NOT use tools like `pdftotext`

- `NOTES.md` is the user's working note, not the paper body. Read it for context;
- preserve user-written content; never treat it as a substitute for the source
- When the user already gives a paper path, start from that folder (NOTES → body).Do not list the whole catalog first

## Chat rules

- Math (KaTeX): inline `$…$`, display `$$…$$` on their own lines; prefer `$`/`$$`
  over `\(...\)` / bare TeX in prose; escape a literal dollar as `\$`.
  - DO NOT USE custom `\b`-prefixed macros (e.g., `\bx`, `\bmu`); Use standard `\boldsymbol{...}` / `\mathbf{...}` instead.
- Structured vault/catalog changes (import, move, download, parse, layout, marks,
  tags): use the `agentero` CLI with `--json` and **vault-relative paths**.
  Exact flags: skill **`agentero-cli`** (`$agentero-cli` / `/agentero-cli` /
  `/skill:agentero-cli`). Prefer files for ordinary reading/Q&A.
- Cite sources inline **without wrapping parentheses**. citation **hrefs should target the local PDF**
  `[Section 2.3](papers/<id>/<id>.pdf#section=2.3)`,
  `[Figure 1](papers/<id>/<id>.pdf#figure=1)`,
  `[p.11](papers/<id>/<id>.pdf#page=11)`,
  or notes `[[papers/<id>/NOTES]]` / `[[papers/<id>/NOTES|short title]]`.For web pages use `[domain](https://...)`.

## Rules

- Do not invent facts, numbers, citations, or experimental conclusions. Mark uncertainty.
- Keep `[[wikilinks]]` and `![[embeds]]` as written; preserve ` ```mermaid ` fences.
- Never overwrite user notes without an explicit draft + confirmation path.

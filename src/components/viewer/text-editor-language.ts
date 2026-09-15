import type {
	Completion,
	CompletionContext,
	CompletionResult,
} from "@codemirror/autocomplete";
import { json } from "@codemirror/lang-json";
import { python } from "@codemirror/lang-python";
import { StreamLanguage } from "@codemirror/language";
import { stex } from "@codemirror/legacy-modes/mode/stex";
import { yaml } from "@codemirror/legacy-modes/mode/yaml";
import type { Extension } from "@codemirror/state";
import { textLanguageIdForPath } from "@/lib/workspace/viewer";

/**
 * Language support for the plain-text editor, keyed off the path extension
 * (routing lives in `textLanguageIdForPath`). Unknown extensions get no
 * highlighting — plain text is the fallback viewer, not a gate.
 */

const stexLanguage = StreamLanguage.define(stex);
const yamlLanguage = StreamLanguage.define(yaml);

/**
 * Minimal BibTeX tokenizer (no upstream legacy mode ships one):
 * `%{ … %}` comments, `@type` keywords, entry keys and bare words as
 * variables, `field =` names as properties, quoted/braced values as strings.
 */
const bibtexLanguage = StreamLanguage.define<{ inComment: boolean }>({
	startState: () => ({ inComment: false }),
	token(stream, state) {
		if (state.inComment) {
			if (stream.match(/^.*?%}/)) state.inComment = false;
			else stream.skipToEnd();
			return "comment";
		}
		if (stream.eatSpace()) return null;
		if (stream.match(/^%{/)) {
			state.inComment = true;
			return "comment";
		}
		if (stream.eat("@")) {
			stream.match(/^[A-Za-z]+/);
			return "keyword";
		}
		if (stream.match(/^[A-Za-z][A-Za-z0-9_-]*(?=\s*=)/)) return "property";
		if (stream.match(/^"[^"]*"?/)) return "string";
		if (stream.match(/^[{}(),=]/)) return "operator";
		if (stream.match(/^[^{}\s,"=@%]+/)) return "variable";
		stream.next();
		return null;
	},
});

const BIBTEX_ENTRY_TYPES: Completion[] = (
	[
		["article", "Journal article"],
		["inproceedings", "Conference paper"],
		["incollection", "Chapter in a book"],
		["book", "Book"],
		["inbook", "Part of a book"],
		["phdthesis", "PhD thesis"],
		["mastersthesis", "Master's thesis"],
		["techreport", "Technical report"],
		["proceedings", "Proceedings volume"],
		["unpublished", "Unpublished work"],
		["misc", "Miscellaneous"],
		["online", "Online resource"],
	] as const
).map(([type, detail]) => ({
	label: `@${type}`,
	detail,
	type: "keyword",
}));

const BIBTEX_FIELDS: Completion[] = (
	[
		"title",
		"author",
		"editor",
		"year",
		"month",
		"journal",
		"booktitle",
		"volume",
		"number",
		"pages",
		"publisher",
		"address",
		"edition",
		"series",
		"institution",
		"school",
		"organization",
		"howpublished",
		"doi",
		"url",
		"isbn",
		"issn",
		"keywords",
		"abstract",
		"note",
	] as const
).map((field) => ({ label: field, type: "property" }));

function bibtexCompletion(context: CompletionContext): CompletionResult | null {
	// Entry type right after `@` (also auto-opens the menu while typing).
	const entryType = context.matchBefore(/@[A-Za-z]*/);
	if (entryType) {
		return {
			from: entryType.from,
			options: BIBTEX_ENTRY_TYPES,
			validFor: /^@?[A-Za-z]*$/,
		};
	}
	// Field name at the start of a fresh line inside an entry.
	const line = context.state.doc.lineAt(context.pos);
	const before = line.text.slice(0, context.pos - line.from);
	const word = before.match(/^\s*([A-Za-z-]*)$/)?.[1];
	if (
		word != null &&
		/@[A-Za-z]+\s*[({]/.test(context.state.sliceDoc(0, line.from))
	) {
		return {
			from: line.from + before.length - word.length,
			options: BIBTEX_FIELDS,
			validFor: /^[A-Za-z-]*$/,
		};
	}
	return null;
}

const LATEX_COMMANDS: Completion[] = (
	[
		["\\section{", "Section heading"],
		["\\subsection{", "Subsection heading"],
		["\\subsubsection{", "Subsubsection heading"],
		["\\textbf{", "Bold text"],
		["\\textit{", "Italic text"],
		["\\emph{", "Emphasis"],
		["\\underline{", "Underlined text"],
		["\\texttt{", "Monospaced text"],
		["\\cite{", "Citation key"],
		["\\ref{", "Reference label"],
		["\\eqref{", "Equation reference"],
		["\\label{", "Define label"],
		["\\frac{}{}", "Fraction"],
		["\\sqrt{", "Square root"],
		["\\begin{itemize}", "Bulleted list"],
		["\\begin{enumerate}", "Numbered list"],
		["\\begin{equation}", "Numbered equation"],
		["\\begin{align}", "Aligned equations"],
		["\\begin{figure}", "Figure environment"],
		["\\begin{table}", "Table environment"],
		["\\begin{center}", "Centered block"],
		["\\item", "List item"],
		["\\documentclass{article}", "Document class"],
		["\\usepackage{", "Load package"],
		["\\title{", "Document title"],
		["\\author{", "Document author"],
		["\\maketitle", "Render title block"],
	] as const
).map(([label, detail]) => ({ label, detail, type: "function" }));

function latexCompletion(context: CompletionContext): CompletionResult | null {
	const command = context.matchBefore(/\\[a-zA-Z]*/);
	if (!command) return null;
	return {
		from: command.from,
		options: LATEX_COMMANDS,
		validFor: /^\\[a-zA-Z]*$/,
	};
}

/**
 * Extensions for one file: the language plus (for TeX / BibTeX) a custom
 * completion source attached as language data, so `basicSetup`'s
 * autocompletion picks it up without overriding anything.
 */
export function textLanguageExtensions(path: string): Extension[] {
	switch (textLanguageIdForPath(path)) {
		case "json":
			return [json()];
		case "python":
			return [python()];
		case "yaml":
			return [yamlLanguage];
		case "tex":
			return [
				stexLanguage,
				stexLanguage.data.of({ autocomplete: latexCompletion }),
			];
		case "bib":
			return [
				bibtexLanguage,
				bibtexLanguage.data.of({ autocomplete: bibtexCompletion }),
			];
		default:
			return [];
	}
}

/**
 * Markdown → Plate deserialize prep.
 *
 * 1. Preserve extra blank lines that CommonMark would otherwise collapse.
 * 2. Prevent an unclosed block-math fence from consuming the rest of a document.
 * 3. Escape a bare `<` that would crash the MDX JSX tokenizer (e.g. `<0.5B`).
 */

const BLOCK_MATH_FENCE = /^ {0,3}\$\$[ \t]*\r?$/;
const CODE_FENCE = /^ {0,3}(`{3,}|~{3,})/;
/** Matches Plate's empty-paragraph serialize placeholder. */
const BLANK_PARAGRAPH_PLACEHOLDER = "\u200B";

/**
 * `remark` / CommonMark collapse 2+ blank lines between blocks into one break.
 * Plate keeps extra empty paragraphs on disk as ZWSP-only lines; rewrite raw
 * extra blanks into that shape before deserialize so external edits and
 * Agent writes round-trip with the same spacing.
 *
 * Skips fenced code and `$$` math blocks. Already-placeholder lines are left
 * alone so Agentero-saved files are not double-wrapped.
 */
export function preserveExtraBlankLines(source: string): string {
	const lines = source.split("\n");
	const out: string[] = [];
	let codeFence: { character: string; length: number } | null = null;
	let inMath = false;
	let i = 0;

	while (i < lines.length) {
		const line = lines[i] ?? "";
		const codeMatch = line.match(CODE_FENCE);

		if (!inMath && codeMatch) {
			const marker = codeMatch[1];
			if (!codeFence) {
				codeFence = { character: marker[0], length: marker.length };
			} else if (
				marker[0] === codeFence.character &&
				marker.length >= codeFence.length
			) {
				codeFence = null;
			}
			out.push(line);
			i += 1;
			continue;
		}

		if (!codeFence && BLOCK_MATH_FENCE.test(line)) {
			inMath = !inMath;
			out.push(line);
			i += 1;
			continue;
		}

		if (codeFence || inMath || !isCollapsibleBlankLine(line)) {
			out.push(line);
			i += 1;
			continue;
		}

		let j = i;
		while (j < lines.length && isCollapsibleBlankLine(lines[j] ?? "")) {
			j += 1;
		}
		const blankCount = j - i;
		if (blankCount <= 1) {
			for (let k = i; k < j; k += 1) out.push(lines[k] ?? "");
		} else {
			// One blank separates blocks; each extra blank → empty paragraph.
			out.push("");
			for (let extra = 0; extra < blankCount - 1; extra += 1) {
				out.push(BLANK_PARAGRAPH_PLACEHOLDER);
				out.push("");
			}
		}
		i = j;
	}

	return out.join("\n");
}

/** Empty / whitespace-only; ZWSP placeholders count as content. */
function isCollapsibleBlankLine(line: string): boolean {
	return line.replace(/\r$/, "").trim() === "";
}

/**
 * `remark-math` treats a standalone `$$` as a block fence. When its closing
 * fence is missing, the parser legitimately puts all following Markdown into
 * the equation node, which makes unrelated content appear broken in Plate.
 */
function escapeUnclosedBlockMath(source: string): string {
	const fences: number[] = [];
	let codeFence: { character: string; length: number } | null = null;
	let offset = 0;

	for (const line of source.split("\n")) {
		const codeMatch = line.match(CODE_FENCE);
		if (codeMatch) {
			const marker = codeMatch[1];
			if (!codeFence) {
				codeFence = { character: marker[0], length: marker.length };
			} else if (
				marker[0] === codeFence.character &&
				marker.length >= codeFence.length
			) {
				codeFence = null;
			}
		} else if (!codeFence && BLOCK_MATH_FENCE.test(line)) {
			fences.push(offset + line.search(/\$\$/));
		}
		offset += line.length + 1;
	}

	if (fences.length % 2 === 0) return source;

	const dollarOffset = fences.at(-1);
	if (dollarOffset === undefined) return source;
	return `${source.slice(0, dollarOffset)}\\${source.slice(dollarOffset)}`;
}

export function prepareMarkdownForDeserialize(source: string): string {
	// After the block-math pass so an already-repaired `\$` fence cannot
	// desync the math tracking below.
	return escapeStrayLessThan(
		escapeUnclosedBlockMath(preserveExtraBlankLines(source)),
	);
}

/** Characters allowed to start a JSX name (`remarkMdx` tag or fragment). */
const JSX_NAME_START = /[\p{ID_Start}$_]/u;
/** CommonMark whitespace: a `<` before it is plain text, not a tag. */
const MARKDOWN_SPACE = /[ \t\v\f\r]/;
/** Inline regions where `<` is already literal: math spans and code spans. */
const INLINE_LITERAL = /\$\$[^$\n]*\$\$|\$(?:\\\$|[^$\n])+\$|`+[^`\n]*?`+/g;

/**
 * `remarkMdx` hard-crashes on a `<` that is neither text-adjacent whitespace
 * nor a plausible tag start: prose comparisons such as `<0.5B`, `p<0.05`,
 * `<3` or arrows like `<-` throw `Unexpected character … before name`. Plate's
 * recovery then cuts the document at that `<` and silently drops every block
 * after the first line of the remainder (#533). Rewrite such `<` as `&lt;`:
 * neither the MDX tokenizer nor Plate's `htmlToJsx` can mistake an entity for
 * a tag, and remark still decodes it back to a literal `<` in the editor.
 */
export function escapeStrayLessThan(source: string): string {
	if (!source.includes("<")) return source;
	const out: string[] = [];
	let codeFence: { character: string; length: number } | null = null;
	let inMath = false;

	for (const line of source.split("\n")) {
		const codeMatch = line.match(CODE_FENCE);
		if (!inMath && codeMatch) {
			const marker = codeMatch[1];
			if (!codeFence) {
				codeFence = { character: marker[0], length: marker.length };
			} else if (
				marker[0] === codeFence.character &&
				marker.length >= codeFence.length
			) {
				codeFence = null;
			}
			out.push(line);
			continue;
		}
		if (!codeFence && BLOCK_MATH_FENCE.test(line)) {
			inMath = !inMath;
			out.push(line);
			continue;
		}
		out.push(
			codeFence || inMath || !line.includes("<") ? line : escapeLine(line),
		);
	}
	return out.join("\n");
}

/** Apply {@link escapeSegment} outside inline code/math spans only. */
function escapeLine(line: string): string {
	let out = "";
	let last = 0;
	for (const match of line.matchAll(INLINE_LITERAL)) {
		const start = match.index ?? 0;
		out += escapeSegment(line.slice(last, start)) + match[0];
		last = start + match[0].length;
	}
	return out + escapeSegment(line.slice(last));
}

function escapeSegment(text: string): string {
	let out = "";
	for (let i = 0; i < text.length; i++) {
		if (text[i] === "<" && !isEscaped(text, i) && !canStartJsxTag(text, i)) {
			out += "&lt;";
			continue;
		}
		out += text[i];
	}
	return out;
}

/** True when the character at `i` is preceded by an odd number of `\`. */
function isEscaped(text: string, i: number): boolean {
	let backslashes = 0;
	for (let j = i - 1; j >= 0 && text[j] === "\\"; j -= 1) backslashes += 1;
	return backslashes % 2 === 1;
}

/** False when the `<` at `i` would make `remarkMdx` throw immediately. */
function canStartJsxTag(text: string, i: number): boolean {
	if (text.startsWith("<!--", i)) return true;
	const next = text[i + 1];
	if (next === undefined) return false;
	if (next === "/") {
		// Closing tag: only `</name`, `</>` shape; `</3` crashes too.
		const after = text[i + 2];
		return after !== undefined && (after === ">" || JSX_NAME_START.test(after));
	}
	return next === ">" || JSX_NAME_START.test(next) || MARKDOWN_SPACE.test(next);
}

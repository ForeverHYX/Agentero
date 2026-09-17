import { Compartment, EditorState, type Extension } from "@codemirror/state";
import { oneDark } from "@codemirror/theme-one-dark";
import { EditorView } from "@codemirror/view";
import { basicSetup } from "codemirror";
import { useTheme } from "next-themes";
import { useCallback, useEffect, useMemo, useRef } from "react";
import { useTextEditorFontSize } from "@/components/viewer/hooks/use-text-editor-font-size";
import { textLanguageExtensions } from "@/components/viewer/text-editor-language";
import { TextEditorToolbar } from "@/components/viewer/text-editor-toolbar";

interface TextEditorProps {
	seed: string;
	path: string;
	reloadKey: number;
	onPersist: (
		path: string,
		content: string,
		lastSaved: string,
	) => Promise<boolean>;
	onDirtyChange: (dirty: boolean) => void;
	className?: string;
}

const AUTOSAVE_DELAY_MS = 800;

// Base chrome rides the app's shadcn design tokens so light/dark both blend
// with the surrounding surface; syntax colors come from the default light
// highlight style / oneDark (swapped via compartment). Placed after oneDark
// so the transparent background wins over its solid panel color.
const baseTheme = EditorView.theme({
	"&": {
		height: "100%",
		backgroundColor: "transparent",
		color: "var(--foreground)",
	},
	".cm-scroller": {
		fontFamily: "var(--font-mono)",
		lineHeight: "1.65",
	},
	".cm-gutters": {
		backgroundColor: "transparent",
		color: "var(--muted-foreground)",
		borderRight: "1px solid var(--border)",
	},
	".cm-activeLine": {
		backgroundColor: "color-mix(in oklab, var(--muted) 45%, transparent)",
	},
	".cm-activeLineGutter": {
		backgroundColor: "color-mix(in oklab, var(--muted) 60%, transparent)",
	},
	".cm-content": { caretColor: "var(--foreground)" },
	".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--foreground)" },
	"&.cm-focused": { outline: "none" },
});

/**
 * Plain-text / code editor (CodeMirror 6) for the `text` fallback mode.
 * Same lifecycle contract as ExcalidrawViewer: React props seed the document
 * and signal external reloads (`reloadKey`); live content, dirty state and
 * debounced autosave are owned here and flow out via callbacks. Unlike the
 * Excalidraw viewer a reload swaps the document in place — view, scroll and
 * undo history are not thrown away.
 */
export function TextEditor({
	seed,
	path,
	reloadKey,
	onPersist,
	onDirtyChange,
	className,
}: TextEditorProps) {
	// Font size managed by hook with process-wide persistence (like PDF paper tone).
	const { fontSize, setFontSize } = useTextEditorFontSize();

	// Follow the app theme (system / light / dark preference via next-themes).
	const { resolvedTheme } = useTheme();
	const hostRef = useRef<HTMLDivElement | null>(null);
	const viewRef = useRef<EditorView | null>(null);
	const themeCompartment = useRef(new Compartment());
	const languageCompartment = useRef(new Compartment());
	const fontSizeCompartment = useRef(new Compartment());
	const lastSavedRef = useRef(seed);
	const pendingContentRef = useRef<string | null>(null);
	const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
	const dirtyRef = useRef(false);

	// Latest seed for the reload effect without re-mounting the editor.
	const seedRef = useRef(seed);
	seedRef.current = seed;
	// Initial language/theme are read at mount; later changes go through the
	// reconfigure effects below (mounting must stay once-per-tab).
	const pathRef = useRef(path);
	pathRef.current = path;
	const themeRef = useRef(resolvedTheme);
	themeRef.current = resolvedTheme;
	const fontSizeRef = useRef(fontSize);
	fontSizeRef.current = fontSize;

	// Keep callbacks stable across parent re-renders (doc-view passes inline
	// arrows for `onDirtyChange`).
	const onDirtyChangeRef = useRef(onDirtyChange);
	onDirtyChangeRef.current = onDirtyChange;

	const flush = useCallback(async () => {
		const content = pendingContentRef.current;
		if (content == null) return;
		pendingContentRef.current = null;
		const ok = await onPersist(path, content, lastSavedRef.current);
		if (ok) {
			lastSavedRef.current = content;
			dirtyRef.current = false;
			onDirtyChangeRef.current(false);
		}
	}, [onPersist, path]);

	const schedulePersist = useCallback(
		(content: string) => {
			pendingContentRef.current = content;
			if (debounceRef.current) clearTimeout(debounceRef.current);
			debounceRef.current = setTimeout(() => {
				debounceRef.current = null;
				void flush();
			}, AUTOSAVE_DELAY_MS);
		},
		[flush],
	);

	// The update listener is registered once at mount; route it through a ref
	// so a path change (rename) keeps saving to the current file.
	const schedulePersistRef = useRef(schedulePersist);
	schedulePersistRef.current = schedulePersist;

	// Editor owns its state: mount once, reconfigure language/theme in place.
	useEffect(() => {
		const host = hostRef.current;
		if (!host) return;
		const language = textLanguageExtensions(pathRef.current);
		const extensions: Extension[] = [
			basicSetup,
			EditorView.lineWrapping,
			languageCompartment.current.of(language),
			themeCompartment.current.of(themeRef.current === "dark" ? oneDark : []),
			fontSizeCompartment.current.of(
				EditorView.theme({ "&": { fontSize: `${fontSizeRef.current}px` } }),
			),
			baseTheme,
			EditorView.updateListener.of((update) => {
				if (!update.docChanged) return;
				if (!dirtyRef.current) {
					dirtyRef.current = true;
					onDirtyChangeRef.current(true);
				}
				schedulePersistRef.current(update.state.doc.toString());
			}),
		];
		const view = new EditorView({
			parent: host,
			state: EditorState.create({ doc: seedRef.current, extensions }),
		});
		viewRef.current = view;
		return () => {
			view.destroy();
			viewRef.current = null;
		};
		// Mount-once; seed/path/theme changes are handled by the effects below.
	}, []);

	// Follow theme switches without rebuilding the editor.
	useEffect(() => {
		viewRef.current?.dispatch({
			effects: themeCompartment.current.reconfigure(
				resolvedTheme === "dark" ? oneDark : [],
			),
		});
	}, [resolvedTheme]);

	// Follow fontSize changes without rebuilding the editor.
	useEffect(() => {
		viewRef.current?.dispatch({
			effects: fontSizeCompartment.current.reconfigure(
				EditorView.theme({ "&": { fontSize: `${fontSize}px` } }),
			),
		});
	}, [fontSize]);

	// A renamed path only swaps the language support.
	const language = useMemo(() => textLanguageExtensions(path), [path]);
	useEffect(() => {
		viewRef.current?.dispatch({
			effects: languageCompartment.current.reconfigure(language),
		});
	}, [language]);

	// The seed is authoritative: after our own save it equals what we wrote
	// (idempotent), after an external change it is the new disk snapshot.
	useEffect(() => {
		lastSavedRef.current = seed;
	}, [seed]);

	// Flush pending autosave on unmount (tab close / LRU eviction).
	useEffect(() => {
		return () => {
			if (debounceRef.current) {
				clearTimeout(debounceRef.current);
				debounceRef.current = null;
			}
			if (pendingContentRef.current) {
				void flush();
			}
		};
	}, [flush]);

	// A `reloadKey` bump means the file was reloaded from disk (external
	// change the user accepted). Drop any pending autosave from the superseded
	// session — its flush would pass the conflict check (disk === lastSaved)
	// and overwrite the reloaded content — and swap the document in place.
	useEffect(() => {
		if (reloadKey === 0) return; // mount: document was seeded directly
		if (debounceRef.current) {
			clearTimeout(debounceRef.current);
			debounceRef.current = null;
		}
		pendingContentRef.current = null;
		const view = viewRef.current;
		const next = seedRef.current;
		lastSavedRef.current = next;
		if (view && view.state.doc.toString() !== next) {
			view.dispatch({
				changes: { from: 0, to: view.state.doc.length, insert: next },
			});
		}
		dirtyRef.current = false;
		onDirtyChangeRef.current(false);
	}, [reloadKey]);

	// An unreadable file still opens an empty buffer — the next autosave
	// creates/repairs the file on disk.
	return (
		<div className="group relative h-full w-full overflow-hidden">
			<TextEditorToolbar fontSize={fontSize} onFontSizeChange={setFontSize} />
			<div
				ref={hostRef}
				className={`h-full w-full overflow-hidden ${className ?? ""}`}
			/>
		</div>
	);
}

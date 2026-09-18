/** Ephemeral Add to chat draft, independent of the surface's live selection. */
import { createStore } from "zustand/vanilla";
import {
	createSelectionContext,
	pinSelection,
	type SelectionContext,
	type SelectionInput,
} from "@/lib/agent/selection-store";
import { vaultStore } from "@/lib/vault/store";

export type SelectionChatDraft = {
	selection: SelectionContext;
	screen: { x: number; y: number };
	stage: "menu" | "comment";
};
export const selectionChatStore = createStore<{
	draft: SelectionChatDraft | null;
}>(() => ({ draft: null }));

export function openSelectionChat(
	input: SelectionInput,
	screen: SelectionChatDraft["screen"],
	stage: SelectionChatDraft["stage"] = "comment",
): void {
	const selection = createSelectionContext(input);
	if (selection)
		selectionChatStore.setState({
			draft: { selection, screen: { ...screen }, stage },
		});
}
export function dismissSelectionChat(): void {
	selectionChatStore.setState({ draft: null });
}
export function beginSelectionComment(): void {
	const { draft } = selectionChatStore.getState();
	if (draft)
		selectionChatStore.setState({ draft: { ...draft, stage: "comment" } });
}
export function confirmSelectionChat(comment: string): boolean {
	const { draft } = selectionChatStore.getState();
	if (!draft) return false;
	pinSelection({ ...draft.selection, comment: comment.trim() || undefined });
	dismissSelectionChat();
	return true;
}
// Quotes must never cross vault boundaries.
vaultStore.subscribe((state, previous) => {
	if (state.vaultPath !== previous.vaultPath) dismissSelectionChat();
});

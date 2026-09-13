import { normalizeMarkdownMath } from "@/lib/markdown/math-normalize";

import { linkifyBareUrls, linkifyBareVaultCitations } from "./bare-url-link";
import { stripCitationStatusTags } from "./citation-href";
import { linkifyWikilinks } from "./wikilink-citation";

/** Prepare agent text consistently before Streamdown renders it. */
export function prepareAgentMessageMarkdown(text: string): string {
	return linkifyBareUrls(
		linkifyBareVaultCitations(
			linkifyWikilinks(stripCitationStatusTags(normalizeMarkdownMath(text))),
		),
	);
}

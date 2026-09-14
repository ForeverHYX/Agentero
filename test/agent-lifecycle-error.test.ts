import { describe, expect, it } from "vitest";
import { lifecycleErrorMessage } from "@/lib/agent/lifecycle-error";

const t = (key: string, options?: Record<string, string>) =>
	`${key}:${options?.cacheCommand ?? ""}`;

describe("lifecycleErrorMessage", () => {
	it("explains an npm cache EPERM failure", () => {
		const raw =
			"npm error EPERM: operation not permitted\nnpm error Log files were not written due to an error writing to the directory: D:\\program\\node_cache\\_logs";
		expect(lifecycleErrorMessage(raw, t)).toContain(
			"agent.npmCacheWriteFailed",
		);
	});
	it("recognizes legacy npm ERR output", () => {
		const raw =
			"npm ERR! code EPERM\nnpm ERR! Log files were not written due to an error writing to the directory: C:\\npm-cache\\_logs";
		expect(lifecycleErrorMessage(raw, t)).toContain(
			"agent.npmCacheWriteFailed",
		);
	});
	it("keeps unrelated installer output", () => {
		expect(lifecycleErrorMessage("npm error network timeout", t)).toBe(
			"npm error network timeout",
		);
	});
});

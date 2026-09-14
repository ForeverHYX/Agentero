type Translate = (
	key: "agent.npmCacheWriteFailed",
	options: Record<string, string>,
) => string;

export function lifecycleErrorMessage(message: string, t: Translate): string {
	const normalized = message.toLowerCase();
	const npmCacheWriteFailure =
		(normalized.includes("npm error") || normalized.includes("npm err!")) &&
		(normalized.includes("eperm") ||
			normalized.includes("operation was rejected by your operating system")) &&
		(normalized.includes("cache") ||
			normalized.includes("log files were not written") ||
			normalized.includes("error writing to the directory"));

	if (npmCacheWriteFailure) {
		return t("agent.npmCacheWriteFailed", {
			cacheCommand: 'npm config set cache "%LOCALAPPDATA%\\npm-cache"',
		});
	}
	return message;
}

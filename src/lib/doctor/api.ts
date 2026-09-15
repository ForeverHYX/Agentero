import {
	type AgentAcpDiagnostic_Serialize,
	type AliasRepairCandidate_Serialize,
	commands,
	type DoctorIssue_Serialize,
	type DoctorReport_Serialize,
	type DoctorVaultState,
	type DuplicateRepairResult,
	type NetworkDoctorReport_Serialize,
	type NetworkEndpointDiagnostic_Serialize,
	type VisualMarkCandidate,
	type WikiCheckIssue_Serialize,
	type WikilinkRepairPlan_Serialize,
	type WikilinkRepairResidual_Serialize,
	type WikilinkRepairSuggestion_Serialize,
} from "@/lib/core/bindings";
import { callApi, callApiResult } from "@/lib/core/ipc";

export type HostToolStatus = "available" | "missing" | "unusable";
export type HostToolDiagnostic = {
	status: HostToolStatus;
	resolvedPath?: string | null;
	version?: string | null;
	detail?: string | null;
};
export type HostDoctorReport = {
	node: HostToolDiagnostic;
	npm: HostToolDiagnostic;
	npmPrefix?: string | null;
};
export type NodeInstallOutcome = "installed" | "failed" | "no-package-manager";
export type NodeInstallResult = {
	outcome: NodeInstallOutcome;
	installer?: string | null;
	error?: string | null;
	report: HostDoctorReport;
};

/** Read models come straight from the generated wire contract. */
export type DoctorIssue = DoctorIssue_Serialize;
export type AgentAcpDiagnostic = AgentAcpDiagnostic_Serialize;
export type AliasRepairCandidate = AliasRepairCandidate_Serialize;
export type WikiCheckIssue = WikiCheckIssue_Serialize;
export type { VisualMarkCandidate };
export type DoctorReport = DoctorReport_Serialize;
export type WikilinkRepairSuggestion = WikilinkRepairSuggestion_Serialize;
export type WikilinkRepairResidual = WikilinkRepairResidual_Serialize;
export type WikilinkRepairPlan = WikilinkRepairPlan_Serialize;
export type { DoctorVaultState, DuplicateRepairResult };
export type NetworkEndpointDiagnostic = NetworkEndpointDiagnostic_Serialize;
export type NetworkDoctorReport = NetworkDoctorReport_Serialize;
export type { NetworkStatus } from "@/lib/core/bindings";

type AliasRepairChange = {
	path: string;
	titleAlias: string;
	shortAlias: string;
	expectedHash: string;
};

type WikilinkRepairChange = {
	source: string;
	rangeStart: number;
	rangeEnd: number;
	expected: string;
	replacement: string;
	expectedHash: string;
};

type VisualMarkRepairChange = {
	path: string;
};

export function doctorCheck(vaultPath: string): Promise<DoctorReport> {
	return callApi(() => commands.doctorCheck({ vaultPath }));
}

export function doctorCheckHost(): Promise<HostDoctorReport> {
	// Must go through specta `typedError` (same as doctorCheckAgents). Raw
	// `invoke` returns bare `ApiResult`, which `callApiResult` misreads as a
	// failed TypedResult and surfaces the "Host command failed" fallback.
	return callApiResult(() => commands.doctorCheckHost());
}

/** One-click install of Node.js via the host package manager, then re-probe. */
export function doctorInstallNode(): Promise<NodeInstallResult> {
	return callApiResult(() => commands.doctorInstallNode());
}

/** Re-probe every registered Agent over ACP; may take up to ~30s per agent. */
export function doctorCheckAgents(): Promise<AgentAcpDiagnostic[]> {
	return callApiResult(() => commands.doctorCheckAgents());
}

/** Open a trusted template-owned CLI login command in a confirm-to-run terminal. */
export async function doctorOpenAgentLoginTerminal(
	templateId: string,
): Promise<void> {
	// Envelope 1 (plain ApiResult): the command bypasses specta typedError,
	// so callApiResult would misread the bare envelope as an IPC error.
	await callApi(() => commands.doctorOpenAgentLoginTerminal(templateId));
}

/** Probe network connectivity to paper sources and common hosts. */
export function doctorCheckNetwork(): Promise<NetworkDoctorReport> {
	return callApiResult(() => commands.doctorCheckNetwork());
}

export function doctorApplyAliases(
	vaultPath: string,
	changes: AliasRepairChange[],
): Promise<{ updatedPaths: string[] }> {
	return callApiResult(() =>
		commands.doctorApplyAliases({ vaultPath, changes }),
	);
}

/** Persist ignore/restore for paper-alias candidates (vault `.agentero/doctor.json`). */
export function doctorIgnoreAliases(
	vaultPath: string,
	paths: string[],
	ignore: boolean,
): Promise<DoctorVaultState> {
	return callApi(() =>
		commands.doctorIgnoreAliases({ vaultPath, paths, ignore }),
	);
}

export function doctorPlanWikilinks(
	vaultPath: string,
): Promise<WikilinkRepairPlan> {
	return callApi(() => commands.doctorPlanWikilinks({ vaultPath }));
}

export function doctorApplyWikilinks(
	vaultPath: string,
	changes: WikilinkRepairChange[],
): Promise<{ updatedPaths: string[] }> {
	return callApiResult(() =>
		commands.doctorApplyWikilinks({ vaultPath, changes }),
	);
}

export async function doctorSetDirtyPaths(
	vaultPath: string,
	dirtyPaths: string[],
): Promise<void> {
	await callApi(() => commands.doctorSetDirtyPaths({ vaultPath, dirtyPaths }));
}

export function doctorApplyVisualMarks(
	vaultPath: string,
	changes: VisualMarkRepairChange[],
): Promise<{ updatedPaths: string[] }> {
	return callApiResult(() =>
		commands.doctorApplyVisualMarks({ vaultPath, changes }),
	);
}

export function doctorFixCatalogDuplicates(
	vaultPath: string,
): Promise<DuplicateRepairResult> {
	return callApi(() => commands.doctorFixCatalogDuplicates({ vaultPath }));
}

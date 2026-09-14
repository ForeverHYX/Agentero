import { Loader2, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { DoctorNetworkSection } from "@/components/settings/panes/doctor-network-section";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { commands } from "@/lib/core/bindings";
import { errorText } from "@/lib/core/error";
import { callApi } from "@/lib/core/ipc";
import { notifyError } from "@/lib/core/notify";
import { isTauri } from "@/lib/core/tauri";
import { doctorCheckNetwork, type NetworkDoctorReport } from "@/lib/doctor/api";
import { type AppSettings, saveSettingsAsync } from "@/lib/settings";
import { DEFAULT_NETWORK_PROXY_URL } from "@/lib/settings/defaults";

export function ProxyStep({
	settings,
	patch,
}: {
	settings: AppSettings;
	patch: (p: Partial<AppSettings>) => void;
}) {
	const { t } = useTranslation(["onboarding", "settings"]);
	const [proxyUrlDraft, setProxyUrlDraft] = useState(settings.networkProxyUrl);
	const [systemProxy, setSystemProxy] = useState<string | null>(null);
	const [report, setReport] = useState<NetworkDoctorReport | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [loading, setLoading] = useState(() => isTauri());
	const [saving, setSaving] = useState(false);
	const [autoProbed, setAutoProbed] = useState(false);

	useEffect(() => {
		setProxyUrlDraft(settings.networkProxyUrl);
	}, [settings.networkProxyUrl]);

	useEffect(() => {
		if (!isTauri()) return;
		let cancelled = false;
		void callApi(() => commands.networkSystemProxy())
			.then((proxy) => {
				if (!cancelled) setSystemProxy(proxy ?? null);
			})
			.catch(() => undefined);
		return () => {
			cancelled = true;
		};
	}, []);

	const normalizedProxyUrl = useMemo(
		() => proxyUrlDraft.trim() || DEFAULT_NETWORK_PROXY_URL,
		[proxyUrlDraft],
	);

	const commitProxy = useCallback(
		async (enabled?: boolean) => {
			const next = {
				...settings,
				networkProxyEnabled: enabled ?? settings.networkProxyEnabled,
				networkProxyUrl: normalizedProxyUrl,
			};
			setSaving(true);
			try {
				const saved = await saveSettingsAsync(next);
				patch({
					networkProxyEnabled: saved.networkProxyEnabled,
					networkProxyUrl: saved.networkProxyUrl,
				});
				setProxyUrlDraft(saved.networkProxyUrl);
				return true;
			} catch (e) {
				const message = errorText(e);
				notifyError(message);
				setError(message);
				return false;
			} finally {
				setSaving(false);
			}
		},
		[normalizedProxyUrl, patch, settings],
	);

	const runProbe = useCallback(
		async (enabled?: boolean) => {
			if (!isTauri()) return;
			setLoading(true);
			setError(null);
			const committed = await commitProxy(enabled);
			if (!committed) {
				setLoading(false);
				return;
			}
			try {
				setReport(await doctorCheckNetwork());
			} catch (e) {
				const message = errorText(e);
				notifyError(message);
				setError(message);
			} finally {
				setLoading(false);
			}
		},
		[commitProxy],
	);

	useEffect(() => {
		if (autoProbed) return;
		setAutoProbed(true);
		void runProbe();
	}, [autoProbed, runProbe]);

	const disabled = !isTauri() || loading || saving;
	const description =
		!settings.networkProxyEnabled && systemProxy
			? t("settings:general.networkProxy.systemDetected", { url: systemProxy })
			: t("proxy.hint");

	return (
		<div className="space-y-4">
			<div className="rounded-lg border bg-background p-3">
				<div className="flex items-center gap-2">
					<Input
						id="onboarding-network-proxy-url"
						aria-label={t("proxy.urlLabel")}
						value={proxyUrlDraft}
						onChange={(e) => setProxyUrlDraft(e.currentTarget.value)}
						onBlur={() => void commitProxy()}
						onKeyDown={(e) => {
							if (e.key === "Enter") {
								e.currentTarget.blur();
								void runProbe();
							}
						}}
						placeholder={DEFAULT_NETWORK_PROXY_URL}
						spellCheck={false}
						autoComplete="off"
						disabled={!settings.networkProxyEnabled || !isTauri()}
						className="h-8 min-w-0 flex-1 font-mono text-xs"
					/>
					<Switch
						id="onboarding-network-proxy-enabled"
						aria-label={t("proxy.enabledLabel")}
						checked={settings.networkProxyEnabled}
						disabled={!isTauri()}
						onCheckedChange={(networkProxyEnabled) => {
							patch({ networkProxyEnabled });
							void runProbe(networkProxyEnabled);
						}}
					/>
					<Button
						type="button"
						variant="outline"
						size="sm"
						disabled={disabled}
						onClick={() => void runProbe()}
						aria-label={t("proxy.probe")}
					>
						{loading || saving ? (
							<Loader2 className="animate-spin" aria-hidden />
						) : (
							<RefreshCw aria-hidden />
						)}
						{t("proxy.probe")}
					</Button>
				</div>
				<p className="mt-2 text-muted-foreground text-xs leading-relaxed">
					{description}
				</p>
			</div>

			<DoctorNetworkSection report={report} loading={loading} error={error} />
		</div>
	);
}

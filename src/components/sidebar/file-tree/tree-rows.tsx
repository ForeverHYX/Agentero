import {
	ChevronDown,
	ChevronRight,
	Download,
	Globe,
	Library,
	Loader2,
	ScrollText,
	Trash2,
	Zap,
} from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
	FileTreeActions,
	FileTreeDisclosureIcon,
	FileTreeFile,
	FileTreeFolderRow,
	FileTreeIcon,
	FileTreeName,
	useFileTree,
} from "@/components/ai-elements/file-tree";
import { PLAZA_SOURCE_ICONS } from "@/components/plaza/source-icons";
import { Button } from "@/components/ui/button";
import { MathText } from "@/components/ui/math-text";
import {
	Popover,
	PopoverContent,
	PopoverTrigger,
} from "@/components/ui/popover";
import { Skeleton } from "@/components/ui/skeleton";
import {
	Tooltip,
	TooltipContent,
	TooltipTrigger,
} from "@/components/ui/tooltip";
import { contextPathIcon } from "@/lib/agent/context-path-icon";
import { cn } from "@/lib/core/utils";
import { LIBRARY_VIRTUAL_PATH, TRASH_VIRTUAL_PATH } from "@/lib/paper/api";
import {
	PLAZA_VIRTUAL_PATH,
	type PlazaSource,
	plazaSourceLabel,
} from "@/lib/plaza";
import type { FileNode } from "@/lib/vault";
import type { TexCompileActions as TexCompileHookActions } from "./hooks/use-tex-compile";
import { DOWNLOAD_REASON_KEYS } from "./tree-helpers";

type PaperTreeRowProps = {
	node: FileNode;
	isCut: boolean;
	label: string;
	downloadReasons: Array<keyof typeof DOWNLOAD_REASON_KEYS>;
	isDownloading: boolean;
	isReading: boolean;
	rowBusy: boolean;
	expandable: boolean;
	expanded: boolean;
	onDownload?: () => void;
	onRead?: () => void;
};

export function PaperTreeRow({
	node,
	isCut,
	label,
	downloadReasons,
	isDownloading,
	isReading,
	rowBusy,
	expandable,
	expanded,
	onDownload,
	onRead,
}: PaperTreeRowProps) {
	const { t } = useTranslation("sidebar");
	const { togglePath } = useFileTree();
	const expandLabel = expanded
		? t("fileTree.collapseAttachments")
		: t("fileTree.expandAttachments");
	const showDownload = Boolean(onDownload) && downloadReasons.length > 0;
	const showRead = Boolean(onRead) && !showDownload;
	const reasonTip = downloadReasons.length
		? downloadReasons.map((r) => t(DOWNLOAD_REASON_KEYS[r])).join(" · ")
		: t("fileTree.downloadAssets");
	const showActions = showDownload || showRead;
	return (
		<FileTreeFile
			path={node.path}
			name={label}
			className={cn(isCut && "opacity-50")}
		>
			{expandable ? (
				<Tooltip disableHoverableContent>
					<TooltipTrigger asChild>
						<button
							type="button"
							aria-expanded={expanded}
							aria-label={expandLabel}
							className={cn(
								"group/attachment relative flex size-4 shrink-0 items-center justify-center rounded-sm",
								"text-muted-foreground hover:bg-muted/80",
								"focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
							)}
							onClick={(e) => {
								e.stopPropagation();
								e.preventDefault();
								togglePath(node.path);
							}}
							onPointerDown={(e) => e.stopPropagation()}
							onKeyDown={(e) => e.stopPropagation()}
						>
							<ScrollText
								className="size-4 transition-opacity group-hover/attachment:opacity-0 group-focus-visible/attachment:opacity-0"
								aria-hidden
							/>
							<ChevronRight
								className={cn(
									"pointer-events-none absolute size-4 opacity-0 transition-[opacity,transform] group-hover/attachment:opacity-100 group-focus-visible/attachment:opacity-100",
									expanded && "rotate-90",
								)}
								aria-hidden
							/>
						</button>
					</TooltipTrigger>
					<TooltipContent side="right" className="select-none cursor-default">
						{expandLabel}
					</TooltipContent>
				</Tooltip>
			) : (
				<ScrollText
					className="size-4 shrink-0 text-muted-foreground"
					aria-hidden
				/>
			)}
			<FileTreeName className="min-w-0 flex-1 truncate" title={label}>
				<MathText text={label} />
			</FileTreeName>
			{showActions ? (
				<FileTreeActions
					className="shrink-0"
					onClick={(e) => {
						e.stopPropagation();
					}}
					onKeyDown={(e) => e.stopPropagation()}
				>
					{showDownload ? (
						<Tooltip disableHoverableContent>
							<TooltipTrigger asChild>
								<Button
									type="button"
									variant="ghost"
									size="icon-xs"
									className="size-5"
									aria-label={reasonTip}
									disabled={rowBusy}
									onClick={(e) => {
										e.stopPropagation();
										onDownload?.();
									}}
								>
									{isDownloading ? (
										<Loader2 className="size-3.5 animate-spin" />
									) : (
										<Download className="size-3.5" />
									)}
								</Button>
							</TooltipTrigger>
							<TooltipContent
								side="right"
								className="max-w-xs select-none cursor-default flex-col items-start gap-0"
							>
								<p className="font-medium">{t("fileTree.downloadAssets")}</p>
								<ul className="mt-1 list-disc space-y-0.5 pl-3 text-xs opacity-90">
									{downloadReasons.map((r) => (
										<li key={r}>{t(DOWNLOAD_REASON_KEYS[r])}</li>
									))}
								</ul>
							</TooltipContent>
						</Tooltip>
					) : null}
					{showRead ? (
						<Tooltip disableHoverableContent>
							<TooltipTrigger asChild>
								<Button
									type="button"
									variant="ghost"
									size="icon-xs"
									className="size-5"
									data-read-paper
									aria-label={t("fileTree.readPaper")}
									disabled={rowBusy}
									onClick={(e) => {
										e.stopPropagation();
										onRead?.();
									}}
								>
									{isReading ? (
										<Loader2 className="size-3.5 animate-spin" />
									) : (
										<Zap className="size-3.5" />
									)}
								</Button>
							</TooltipTrigger>
							<TooltipContent
								side="right"
								className="max-w-xs select-none cursor-default"
							>
								<p className="font-medium">{t("fileTree.readPaper")}</p>
							</TooltipContent>
						</Tooltip>
					) : null}
				</FileTreeActions>
			) : null}
		</FileTreeFile>
	);
}

type NodeTreeRowProps = {
	node: FileNode;
	isCut: boolean;
	pendingLoad: boolean;
	expanded: boolean;
	texCompile?: TexCompileHookActions;
	vaultPath?: string | null;
};

export function NodeTreeRow({
	node,
	isCut,
	pendingLoad,
	expanded,
	texCompile,
	vaultPath,
}: NodeTreeRowProps) {
	if (node.kind === "directory") {
		return (
			<div
				className={cn(
					"relative flex w-full items-center",
					isCut && "opacity-50",
				)}
			>
				<div className="min-w-0 flex-1">
					<FileTreeFolderRow path={node.path} name={node.name} />
				</div>
				{pendingLoad && expanded ? (
					<Loader2
						className="pointer-events-none absolute right-2 size-3.5 shrink-0 animate-spin text-muted-foreground"
						aria-hidden
					/>
				) : null}
			</div>
		);
	}

	const Icon = contextPathIcon(node.name);
	const isTex = texCompile?.isTexFile(node.path) ?? false;
	const isCompiling = texCompile?.compilingPath === node.path;

	return (
		<FileTreeFile
			path={node.path}
			name={node.name}
			className={cn(isCut && "opacity-50")}
		>
			<FileTreeIcon>
				<Icon className="size-4 text-muted-foreground" />
			</FileTreeIcon>
			<FileTreeName className="min-w-0 flex-1 truncate" title={node.name}>
				{node.name}
			</FileTreeName>
			{isTex && texCompile ? (
				<TexCompileActions
					actions={texCompile}
					texPath={node.path}
					vaultPath={vaultPath ?? null}
					isCompiling={isCompiling}
				/>
			) : null}
		</FileTreeFile>
	);
}

/** Renders the compile button + engine selector for a .tex file row. */
function TexCompileActions({
	actions,
	texPath,
	vaultPath,
	isCompiling,
}: {
	actions: TexCompileHookActions;
	texPath: string;
	vaultPath: string | null;
	isCompiling: boolean;
}) {
	const { t } = useTranslation("sidebar");
	const [pickerOpen, setPickerOpen] = useState(false);
	const hasEngines = !actions.enginesLoading && actions.engines.length > 0;

	if (isCompiling) {
		return (
			<FileTreeActions
				className="shrink-0"
				onClick={(e) => e.stopPropagation()}
				onKeyDown={(e) => e.stopPropagation()}
			>
				<Loader2
					className="size-3.5 animate-spin text-muted-foreground"
					aria-label={t("fileTree.compileTex")}
				/>
			</FileTreeActions>
		);
	}

	const runCompile = (e: React.MouseEvent) => {
		e.stopPropagation();
		if (vaultPath && hasEngines) {
			void actions.compileTex(texPath, vaultPath);
		}
	};

	return (
		<FileTreeActions
			className="shrink-0"
			onClick={(e) => e.stopPropagation()}
			onKeyDown={(e) => e.stopPropagation()}
		>
			{/*
			 * Single pill-shaped control: left = "编译" (kicks off compile),
			 * right = chevron (opens engine picker). Whole thing shares one
			 * muted background so the row reads as one UI affordance.
			 */}
			<Popover open={pickerOpen} onOpenChange={setPickerOpen}>
				<div
					className={cn(
						"inline-flex h-6 items-stretch overflow-hidden rounded-md border bg-muted text-xs",
						"hover:bg-muted/80",
					)}
				>
					<button
						type="button"
						disabled={!hasEngines}
						className={cn(
							"flex items-center px-2 font-normal",
							hasEngines
								? "text-foreground hover:bg-background/60"
								: "cursor-not-allowed text-muted-foreground",
						)}
						aria-label={t("fileTree.compileTex")}
						onClick={runCompile}
					>
						{t("fileTree.compileTex")}
					</button>
					<PopoverTrigger asChild>
						<button
							type="button"
							className={cn(
								"flex w-5 items-center justify-center border-l border-border/60",
								"hover:bg-background/60",
							)}
							aria-label={t("fileTree.latexEngine")}
							onClick={(e) => e.stopPropagation()}
						>
							<ChevronDown className="size-3" />
						</button>
					</PopoverTrigger>
				</div>
				<PopoverContent
					align="end"
					sideOffset={4}
					className="w-44 p-1"
					onClick={(e) => e.stopPropagation()}
				>
					{actions.enginesLoading ? (
						<div className="px-2 py-1.5 text-muted-foreground text-xs">
							{t("fileTree.detectingEngines")}
						</div>
					) : hasEngines ? (
						<div role="listbox" className="flex flex-col">
							{actions.engines.map((engine) => (
								<button
									key={engine.id}
									type="button"
									role="option"
									aria-selected={actions.selectedEngine === engine.id}
									className={cn(
										"flex w-full items-center rounded-sm px-2 py-1.5 text-left text-xs",
										"hover:bg-accent hover:text-accent-foreground",
										"focus:bg-accent focus:text-accent-foreground focus:outline-none",
									)}
									onClick={(e) => {
										e.stopPropagation();
										actions.selectEngine(engine.id);
										setPickerOpen(false);
									}}
								>
									{engine.label}
								</button>
							))}
						</div>
					) : (
						<div className="px-2 py-1.5 text-muted-foreground text-xs">
							{t("fileTree.noLatexEngine")}
						</div>
					)}
				</PopoverContent>
			</Popover>
		</FileTreeActions>
	);
}

type LibraryRowProps = {
	showDownload: boolean;
	busy: boolean;
	downloadingAll: boolean;
	onDownloadAll: () => void;
};

export function LibraryRow({
	showDownload,
	busy,
	downloadingAll,
	onDownloadAll,
}: LibraryRowProps) {
	const { t } = useTranslation("sidebar");
	return (
		<FileTreeFile
			path={LIBRARY_VIRTUAL_PATH}
			name={t("papersLibrary.title")}
			data-library-row
		>
			<FileTreeIcon>
				<Library className="size-4 text-muted-foreground" />
			</FileTreeIcon>
			<FileTreeName className="min-w-0 flex-1 truncate">
				{t("papersLibrary.title")}
			</FileTreeName>
			{showDownload ? (
				<FileTreeActions
					className="shrink-0"
					onClick={(e) => e.stopPropagation()}
					onKeyDown={(e) => e.stopPropagation()}
				>
					<Tooltip disableHoverableContent>
						<TooltipTrigger asChild>
							<Button
								type="button"
								variant="ghost"
								size="icon-xs"
								className="size-5"
								aria-label={t("fileTree.downloadAllMissing")}
								disabled={busy}
								onClick={(e) => {
									e.stopPropagation();
									onDownloadAll();
								}}
							>
								{downloadingAll ? (
									<Loader2 className="size-3.5 animate-spin" />
								) : (
									<Download className="size-3.5" />
								)}
							</Button>
						</TooltipTrigger>
						<TooltipContent
							side="right"
							className="max-w-xs select-none cursor-default"
						>
							{t("fileTree.downloadAllMissing")}
						</TooltipContent>
					</Tooltip>
				</FileTreeActions>
			) : null}
		</FileTreeFile>
	);
}

export function TrashRow() {
	const { t } = useTranslation("sidebar");
	return (
		<FileTreeFile path={TRASH_VIRTUAL_PATH} name={t("recycleBin.title")}>
			<FileTreeIcon>
				<Trash2 className="size-4 text-muted-foreground" />
			</FileTreeIcon>
			<FileTreeName className="min-w-0 flex-1 truncate">
				{t("recycleBin.title")}
			</FileTreeName>
		</FileTreeFile>
	);
}

/** 广场 parent row — collapsible, with the discovery sources as children. */
export function PlazaRow({ expanded }: { expanded: boolean }) {
	const { t } = useTranslation("sidebar");
	return (
		<FileTreeFile
			path={PLAZA_VIRTUAL_PATH}
			name={t("plaza.plaza")}
			aria-expanded={expanded}
		>
			<FileTreeDisclosureIcon
				isExpanded={expanded}
				icon={<Globe className="size-4 text-muted-foreground" aria-hidden />}
			/>
			<FileTreeName className="min-w-0 flex-1 truncate">
				{t("plaza.plaza")}
			</FileTreeName>
		</FileTreeFile>
	);
}

export function PlazaSourceRow({ source }: { source: PlazaSource }) {
	const Icon = PLAZA_SOURCE_ICONS[source.icon];
	const label = plazaSourceLabel(source);
	return (
		<FileTreeFile
			path={source.path}
			name={label}
			{...(source.id === "cool-papers" ? { "data-cool-papers": "" } : {})}
		>
			<FileTreeIcon>
				<Icon className="size-4" />
			</FileTreeIcon>
			<FileTreeName className="min-w-0 flex-1 truncate" title={label}>
				{label}
			</FileTreeName>
		</FileTreeFile>
	);
}

export function LoadingRows() {
	return (
		<div className="space-y-1 px-2 py-1.5" aria-hidden>
			{["one", "two", "three", "four", "five"].map((key, index) => (
				<div key={key} className="flex h-7 items-center gap-2 rounded px-2">
					<Skeleton className="size-4 shrink-0 library-shimmer" />
					<Skeleton
						className={cn(
							"library-shimmer h-3",
							index === 0 ? "w-32" : index === 1 ? "w-24" : "w-28",
						)}
					/>
				</div>
			))}
		</div>
	);
}

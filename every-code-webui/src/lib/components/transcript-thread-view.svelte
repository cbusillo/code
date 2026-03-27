<script lang="ts">
	import { browser } from '$app/environment';
	import { onDestroy, onMount } from 'svelte';

	import {
		highlightCodeBlock,
		highlightLanguageLabel,
		highlightTerminalOutput,
		highlightUnifiedDiff,
		type HighlightedDiffLine,
		type HighlightTone,
	} from '$lib/code-highlight';
	import { summarizeUnifiedDiff, type DiffReviewSummary } from '$lib/diff-review';
	import { TranscriptBehaviorController } from '$lib/transcript-behavior';
	import type { ResponderDraft } from '$lib/responder-drafts';
	import {
		buildTranscriptReviewArtifacts,
		loadReviewNotes,
		persistReviewNotes,
		pruneReviewNotes,
		type ReviewArtifact
	} from '$lib/transcript-review-artifacts';
	import {
		buildTranscriptLiveSignature,
		summarizeTranscriptLiveBacklog
	} from '$lib/transcript-live-backlog';
	import { transcriptSessionTone } from '$lib/transcript-session-state';
	import type {
		ApprovalCell,
		DiffCell,
		QuestionCell,
		ThinkingCell,
		TranscriptSessionBanner,
		ToolCallCell,
		TranscriptCell
	} from '$lib/transcript-model';

	let {
		threadId = '',
		title,
		description = '',
		badges = [],
		cells = [],
		emptyMessage = 'Hydrate a thread to render the transcript.',
		sessionBanner = null,
		canApprove = false,
		questionDraftsById = {},
		onSessionAction = undefined,
		onApprovalAction = undefined,
		onQuestionOptionSelect = undefined,
		onQuestionCustomValue = undefined,
		onQuestionSubmit = undefined,
		onQuestionFocus = undefined
	}: {
		threadId?: string;
		title: string;
		description?: string;
		badges?: string[];
		cells?: TranscriptCell[];
		emptyMessage?: string;
		sessionBanner?: TranscriptSessionBanner | null;
		canApprove?: boolean;
		questionDraftsById?: Record<string, ResponderDraft>;
		onSessionAction?: ((action: NonNullable<TranscriptSessionBanner['primaryAction']>) => void | Promise<void>) | undefined;
		onApprovalAction?: ((cellId: string, action: string) => void | Promise<void>) | undefined;
		onQuestionOptionSelect?: ((cellId: string, option: string) => void | Promise<void>) | undefined;
		onQuestionCustomValue?: ((cellId: string, value: string) => void | Promise<void>) | undefined;
		onQuestionSubmit?: ((cellId: string) => void | Promise<void>) | undefined;
		onQuestionFocus?: ((cellId: string) => void | Promise<void>) | undefined;
	} = $props();

	let transcriptRoot = $state<HTMLElement | null>(null);
	let showAllReasoning = $state(false);
	let autoCompactTools = $state(true);
	let behaviorVersion = $state(0);
	let followsLiveRegion = $state(true);
	let lastAutoScrolledSignature = $state('');
	let lastFollowedCellCount = $state(0);
	let lastFollowedLiveSignature = $state('');
	let copiedToken = $state('');
	let reviewNotes = $state<Record<string, string>>({});
	let loadedReviewThreadId = $state('');
	let copiedResetTimeout: ReturnType<typeof setTimeout> | null = null;
	const behavior = new TranscriptBehaviorController(() => {
		behaviorVersion += 1;
	});

	$effect(() => {
		cells;
		behavior.sync(cells);
	});

	onDestroy(() => {
		if (copiedResetTimeout) {
			clearTimeout(copiedResetTimeout);
		}
		behavior.destroy();
	});

	$effect(() => {
		if (!browser || !threadId || loadedReviewThreadId === threadId) {
			return;
		}

		loadedReviewThreadId = threadId;
		reviewNotes = loadReviewNotes(window.localStorage, threadId);
	});

		onMount(() => {
		if (!browser) {
			return;
		}

		const updateFollowState = () => refreshFollowState();
		const handleKeydown = (event: KeyboardEvent) => {
			if (!canApprove || !pendingApprovalCells.length) {
				return;
			}

			const target = event.target as HTMLElement | null;
			if (
				event.metaKey ||
				event.ctrlKey ||
				event.altKey ||
				(target instanceof HTMLInputElement ||
					target instanceof HTMLTextAreaElement ||
					target instanceof HTMLSelectElement ||
					target?.isContentEditable)
			) {
				return;
			}

			const firstPendingApproval = pendingApprovalCells[0];
			if (!firstPendingApproval) {
				return;
			}

			if (event.key === 'y' || event.key === 'Y' || event.key === 'Enter') {
				event.preventDefault();
				void onApprovalAction?.(firstPendingApproval.id, firstPendingApproval.primary);
			}

			if (event.key === 'n' || event.key === 'N') {
				event.preventDefault();
				void onApprovalAction?.(firstPendingApproval.id, firstPendingApproval.secondary);
			}
		};
		window.addEventListener('scroll', updateFollowState, { passive: true });
		window.addEventListener('resize', updateFollowState);
		window.addEventListener('keydown', handleKeydown);
		queueMicrotask(updateFollowState);

		return () => {
			window.removeEventListener('scroll', updateFollowState);
			window.removeEventListener('resize', updateFollowState);
			window.removeEventListener('keydown', handleKeydown);
		};
	});

	$effect(() => {
		latestLiveCellId;
		activeTurn;
		queueMicrotask(() => refreshFollowState());
	});

	$effect(() => {
		cells.length;
		latestLiveCellId;
		latestLiveSignature;
		if (!followsLiveRegion) {
			return;
		}

		lastFollowedCellCount = cells.length;
		lastFollowedLiveSignature = latestLiveSignature;
	});

	$effect(() => {
		const signature = `${latestLiveCellId}:${cells.length}:${activeTurn}:${behaviorVersion}`;
		if (!browser || !activeTurn || !latestLiveCellId || !followsLiveRegion) {
			return;
		}

		if (lastAutoScrolledSignature === signature) {
			return;
		}

		lastAutoScrolledSignature = signature;
		queueMicrotask(() => jumpToCell(latestLiveCellId));
	});

	const activeTurn = $derived.by(() =>
		cells.some(
			(cell) =>
				(cell.kind === 'thinking' && cell.state === 'active') ||
				(cell.kind === 'tool_call' && cell.state === 'running')
		)
	);
	const latestLiveCellId = $derived.by(() => {
		const latest = [...cells].reverse().find((cell) => isLiveOrActionableCell(cell));

		return latest?.id ?? cells.at(-1)?.id ?? '';
	});
	const latestDiffId = $derived.by(() => {
		const latestDiff = [...cells].reverse().find((cell): cell is DiffCell => cell.kind === 'diff');

		return latestDiff?.id ?? '';
	});
	const pendingApprovalCells = $derived.by(() =>
		cells.filter(
			(cell): cell is ApprovalCell => cell.kind === 'approval' && cell.status === 'pending'
		)
	);
	const pendingQuestionCells = $derived.by(() =>
		cells.filter((cell): cell is QuestionCell => cell.kind === 'question' && cell.status === 'pending')
	);
	const firstPendingCellId = $derived.by(
		() => pendingApprovalCells[0]?.id ?? pendingQuestionCells[0]?.id ?? ''
	);
	const diffSummaries = $derived.by(() => {
		const summaries = new Map<string, DiffReviewSummary>();

		for (const cell of cells) {
			if (cell.kind !== 'diff') {
				continue;
			}

			summaries.set(cell.id, summarizeUnifiedDiff(cell.patch, cell.previewFile));
		}

		return summaries;
	});
	const totalDiffFiles = $derived.by(() =>
		[...diffSummaries.values()].reduce((sum, summary) => sum + summary.files.length, 0)
	);
	const totalDiffAdditions = $derived.by(() =>
		[...diffSummaries.values()].reduce((sum, summary) => sum + summary.totalAdditions, 0)
	);
	const totalDiffDeletions = $derived.by(() =>
		[...diffSummaries.values()].reduce((sum, summary) => sum + summary.totalDeletions, 0)
	);
	const totalDiffHunks = $derived.by(() =>
		[...diffSummaries.values()].reduce((sum, summary) => sum + summary.totalHunks, 0)
	);
	const latestLiveSignature = $derived.by(() => buildTranscriptLiveSignature(cells, latestLiveCellId));
	const liveBacklog = $derived.by(() =>
		summarizeTranscriptLiveBacklog(cells, lastFollowedCellCount)
	);
	const unseenCellCount = $derived.by(() => {
		if (followsLiveRegion) {
			return 0;
		}

		return liveBacklog.total;
	});
	const hasBufferedLiveChange = $derived.by(() => {
		if (followsLiveRegion) {
			return false;
		}

		return latestLiveSignature !== lastFollowedLiveSignature;
	});
	const liveBacklogBadges = $derived.by(() => {
		if (followsLiveRegion) {
			return [];
		}

		const badges: string[] = [];
		if (liveBacklog.blockerCount) {
			badges.push(
				`${liveBacklog.blockerCount} blocker${liveBacklog.blockerCount === 1 ? '' : 's'}`
			);
		}
		if (liveBacklog.liveCount) {
			badges.push(`${liveBacklog.liveCount} live step${liveBacklog.liveCount === 1 ? '' : 's'}`);
		}
		if (liveBacklog.diffCount) {
			badges.push(`${liveBacklog.diffCount} diff${liveBacklog.diffCount === 1 ? '' : 's'}`);
		}
		if (liveBacklog.messageCount) {
			badges.push(
				`${liveBacklog.messageCount} message${liveBacklog.messageCount === 1 ? '' : 's'}`
			);
		}
		if (!badges.length && hasBufferedLiveChange) {
			badges.push('Live output changed');
		}

		return badges;
	});
	const highlightedCodeBlocks = $derived.by(() => {
		const blocks = new Map<string, ReturnType<typeof highlightCodeBlock>>();

		for (const cell of cells) {
			if (cell.kind !== 'assistant_message') {
				continue;
			}

			for (const [blockIndex, block] of cell.blocks.entries()) {
				if (block.type !== 'code') {
					continue;
				}

				blocks.set(codeBlockKey(cell.id, blockIndex), highlightCodeBlock(block.language, block.text));
			}
		}

		return blocks;
	});
	const highlightedToolOutput = $derived.by(() => {
		const blocks = new Map<string, ReturnType<typeof highlightTerminalOutput>>();

		for (const cell of cells) {
			if (cell.kind !== 'tool_call') {
				continue;
			}

			blocks.set(cell.id, highlightTerminalOutput(cell.output));
		}

		return blocks;
	});
	const highlightedDiffs = $derived.by(() => {
		const blocks = new Map<string, HighlightedDiffLine[]>();

		for (const cell of cells) {
			if (cell.kind !== 'diff') {
				continue;
			}

			blocks.set(cell.id, highlightUnifiedDiff(cell.patch));
		}

		return blocks;
	});
	const reviewArtifacts = $derived.by(() => buildTranscriptReviewArtifacts(cells));

	$effect(() => {
		const next = pruneReviewNotes(reviewNotes, reviewArtifacts);
		if (!sameReviewNotes(reviewNotes, next)) {
			reviewNotes = next;
		}
	});

	$effect(() => {
		if (!browser || !threadId) {
			return;
		}

		persistReviewNotes(window.localStorage, threadId, reviewNotes);
	});

	function shouldExpandThinking(cell: ThinkingCell) {
		behaviorVersion;
		return behavior.isThinkingExpanded(cell, showAllReasoning);
	}

	function isThinkingFresh(cell: ThinkingCell) {
		behaviorVersion;
		return behavior.isThinkingAutoExpanded(cell.id);
	}

	function shouldExpandTool(cell: ToolCallCell) {
		behaviorVersion;
		return behavior.isToolExpanded(cell, autoCompactTools);
	}

	function isToolFresh(cell: ToolCallCell) {
		behaviorVersion;
		return behavior.isToolAutoExpanded(cell.id);
	}

	function diffView(cell: DiffCell) {
		behaviorVersion;
		return behavior.diffView(cell);
	}

	function isCompactDiff(cell: DiffCell) {
		return diffView(cell) === 'compact';
	}

	function isFullDiff(cell: DiffCell) {
		return diffView(cell) === 'full';
	}

	function isLiveOrActionableCell(cell: TranscriptCell) {
		return (
			(cell.kind === 'thinking' && cell.state === 'active') ||
			(cell.kind === 'tool_call' && cell.state === 'running') ||
			(cell.kind === 'approval' && cell.status === 'pending') ||
			(cell.kind === 'question' && cell.status === 'pending') ||
			(cell.kind === 'error' && cell.status === 'interrupted')
		);
	}

	function toggleThinkingExpanded(cell: ThinkingCell) {
		behavior.toggleThinking(cell, showAllReasoning);
	}

	function toggleToolExpanded(cell: ToolCallCell) {
		behavior.toggleTool(cell, autoCompactTools);
	}

	function cycleDiffView(cell: DiffCell) {
		behavior.cycleDiffView(cell.id);
	}

	function jumpToCell(cellId: string) {
		if (!browser || !cellId || !transcriptRoot) {
			return;
		}

		const node = transcriptRoot.querySelector<HTMLElement>(`[data-transcript-cell-id="${cellId}"]`);
		node?.scrollIntoView({ behavior: 'smooth', block: 'center' });
	}

	function jumpToLatest() {
		followsLiveRegion = true;
		jumpToCell(latestLiveCellId);
	}

	function openLatestDiff() {
		if (!latestDiffId) {
			return;
		}

		behavior.setDiffView(latestDiffId, 'full');
		jumpToCell(latestDiffId);
	}

	function jumpToPending() {
		if (!firstPendingCellId) {
			return;
		}

		jumpToCell(firstPendingCellId);
	}

	function jumpToReviewArtifact(artifact: ReviewArtifact) {
		if (artifact.kind === 'diff') {
			behavior.setDiffView(artifact.cellId, 'full');
		}

		jumpToCell(artifact.cellId);
	}

	function refreshFollowState() {
		if (!browser || !transcriptRoot || !latestLiveCellId) {
			followsLiveRegion = true;
			lastFollowedCellCount = cells.length;
			lastFollowedLiveSignature = latestLiveSignature;
			return;
		}

		const node = transcriptRoot.querySelector<HTMLElement>(`[data-transcript-cell-id="${latestLiveCellId}"]`);
		if (!node) {
			return;
		}

		const rect = node.getBoundingClientRect();
		followsLiveRegion = rect.bottom <= window.innerHeight + 120;
		if (followsLiveRegion) {
			lastFollowedCellCount = cells.length;
		}
	}

	function questionDraft(cell: QuestionCell) {
		return questionDraftsById[cell.id] ?? { selectedOption: '', customValue: '' };
	}

	function questionAnswerReady(cell: QuestionCell) {
		const draft = questionDraft(cell);
		if (draft.selectedOption && draft.selectedOption !== '__other') {
			return true;
		}

		if ((cell.allowOther || cell.allowSecret || !cell.options.length) && draft.customValue.trim()) {
			return true;
		}

		return false;
	}

	function responseOriginLabel(respondedBy: string | undefined, fallback = 'another device') {
		return respondedBy === 'This browser' ? 'Answered here' : `Answered remotely${respondedBy ? ` by ${respondedBy}` : ` by ${fallback}`}`;
	}

	async function copyText(token: string, text: string) {
		if (!browser || !navigator.clipboard) {
			return;
		}

		await navigator.clipboard.writeText(text);
		copiedToken = token;
		if (copiedResetTimeout) {
			clearTimeout(copiedResetTimeout);
		}
		copiedResetTimeout = setTimeout(() => {
			copiedToken = '';
		}, 1800);
	}

	function copyLabel(token: string, fallback = 'Copy') {
		return copiedToken === token ? 'Copied' : fallback;
	}

	function reviewNoteValue(artifactId: string) {
		return reviewNotes[artifactId] ?? '';
	}

	function updateReviewNote(artifactId: string, value: string) {
		reviewNotes = {
			...reviewNotes,
			[artifactId]: value
		};
	}

	function codeBlockKey(cellId: string, blockIndex: number) {
		return `${cellId}:${blockIndex}`;
	}

	function codeBlockLines(cellId: string, blockIndex: number) {
		return highlightedCodeBlocks.get(codeBlockKey(cellId, blockIndex)) ?? [];
	}

	function toolOutputLines(cell: ToolCallCell) {
		return highlightedToolOutput.get(cell.id) ?? [];
	}

	function diffLines(cell: DiffCell) {
		return highlightedDiffs.get(cell.id) ?? [];
	}

	function diffSummary(cell: DiffCell) {
		return (
			diffSummaries.get(cell.id) ?? {
				files: [],
				totalAdditions: 0,
				totalDeletions: 0,
				totalHunks: 0
			}
		);
	}

	function diffLineTone(kind: HighlightedDiffLine['kind']) {
		switch (kind) {
			case 'meta':
				return 'border-amber-300/25 bg-amber-300/8 text-amber-50/90';
			case 'hunk':
				return 'border-sky-300/30 bg-sky-300/10 text-sky-50';
			case 'addition':
				return 'border-emerald-300/32 bg-emerald-300/11 text-emerald-50';
			case 'deletion':
				return 'border-rose-300/32 bg-rose-300/11 text-rose-50';
			default:
				return 'border-transparent text-stone-200';
		}
	}

	function assistantTitleClass(cell: { title: string; blocks: Array<unknown> }) {
		const compactHeading = cell.title.length < 42 && cell.blocks.length <= 1;

		return compactHeading
			? 'mt-3 text-[1.45rem] leading-tight font-[var(--font-display)] tracking-[-0.03em] text-stone-950'
			: 'mt-3 text-[1.85rem] leading-none font-[var(--font-display)] tracking-[-0.04em] text-stone-950';
	}

	function highlightToneClass(tone: HighlightTone) {
		switch (tone) {
			case 'comment':
				return 'text-stone-400 italic';
			case 'keyword':
				return 'text-cyan-300';
			case 'string':
				return 'text-amber-300';
			case 'number':
				return 'text-sky-300';
			case 'type':
				return 'text-emerald-300';
			case 'property':
				return 'text-orange-200';
			case 'command':
				return 'text-sky-200';
			case 'flag':
				return 'text-rose-200';
			case 'path':
				return 'text-lime-300';
			case 'url':
				return 'text-teal-300 underline decoration-teal-300/40 underline-offset-2';
			case 'meta':
				return 'text-stone-400';
			case 'warning':
				return 'text-amber-300';
			case 'error':
				return 'text-rose-300';
			case 'success':
				return 'text-emerald-300';
			case 'addition':
				return 'text-emerald-200';
			case 'deletion':
				return 'text-rose-200';
			default:
				return '';
		}
	}

	function reviewArtifactTone(artifact: ReviewArtifact) {
		switch (artifact.severity) {
			case 'attention':
				return 'border-rose-200/75 bg-rose-50';
			case 'review':
				return 'border-emerald-200/75 bg-emerald-50/80';
			default:
				return 'border-stone-900/10 bg-white/72';
		}
	}

	function reviewArtifactKindLabel(artifact: ReviewArtifact) {
		switch (artifact.kind) {
			case 'diff':
				return 'Diff';
			case 'code':
				return 'Code';
			case 'tool':
				return 'Tool output';
			default:
				return 'Error';
		}
	}

	function liveFollowLabel() {
		if (followsLiveRegion) {
			return activeTurn ? 'Following live updates' : 'Transcript is settled';
		}

		if (liveBacklog.blockerCount > 0) {
			return `${liveBacklog.blockerCount} blocker${liveBacklog.blockerCount === 1 ? '' : 's'} arrived below`;
		}

		if (unseenCellCount > 0) {
			return `${unseenCellCount} new update${unseenCellCount === 1 ? '' : 's'} below`;
		}

		return hasBufferedLiveChange
			? 'Live output changed below'
			: 'Live updates moved while you were reading earlier context';
	}

	function latestJumpLabel() {
		if (liveBacklog.blockerCount > 0) {
			return `Jump to latest (${liveBacklog.blockerCount} blocker${liveBacklog.blockerCount === 1 ? '' : 's'})`;
		}

		if (unseenCellCount > 0) {
			return `Jump to latest (${unseenCellCount} new)`;
		}

		return hasBufferedLiveChange ? 'Return to latest output' : 'Jump to latest';
	}

	function sameReviewNotes(left: Record<string, string>, right: Record<string, string>) {
		const leftEntries = Object.entries(left);
		const rightEntries = Object.entries(right);
		if (leftEntries.length !== rightEntries.length) {
			return false;
		}

		return leftEntries.every(([key, value]) => right[key] === value);
	}
</script>

<section
	class="rounded-[28px] border border-stone-900/8 bg-[linear-gradient(180deg,_rgba(250,247,242,0.97),_rgba(242,234,223,0.95))] text-stone-900 shadow-[0_24px_90px_rgba(0,0,0,0.22)]"
>
	<div class="border-b border-stone-900/8 px-5 py-5 md:px-7">
		<div class="flex flex-col gap-4 xl:flex-row xl:items-end xl:justify-between">
			<div>
				<p class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] text-stone-500 uppercase">
					Shared thread transcript
				</p>
				<h3
					class="mt-2 text-4xl leading-none font-[var(--font-display)] tracking-[-0.04em] text-stone-950"
				>
					{title}
				</h3>
				{#if description}
					<p class="mt-3 max-w-3xl text-sm leading-7 font-[var(--font-ui)] text-stone-600">
						{description}
					</p>
				{/if}
			</div>
			<details class="rounded-[22px] border border-stone-900/10 bg-white/55 px-4 py-3 text-xs font-[var(--font-ui)] text-stone-700">
				<summary class="flex cursor-pointer list-none items-center justify-between gap-3 [&::-webkit-details-marker]:hidden">
					<div>
						<p class="text-[0.68rem] tracking-[0.18em] text-stone-500 uppercase">View options</p>
						<p class="mt-1 text-xs text-stone-600">Reasoning, tool density, and jump links stay here when you need them.</p>
					</div>
					<span class="rounded-full border border-stone-900/10 bg-white/75 px-3 py-1.5 text-[11px] text-stone-700">
						{showAllReasoning ? 'Reasoning: all' : 'Reasoning: auto'}
					</span>
				</summary>
				<div class="-mx-1 mt-4 flex gap-2 overflow-x-auto px-1 pb-1 sm:mx-0 sm:flex-wrap sm:overflow-visible sm:px-0 sm:pb-0">
					<button
						type="button"
						onclick={() => (showAllReasoning = !showAllReasoning)}
						class={`rounded-full px-3 py-2 transition ${showAllReasoning ? 'bg-stone-950 text-stone-50' : 'bg-stone-900/6 text-stone-700 hover:bg-stone-900/10'}`}
					>
						{showAllReasoning ? 'Show all reasoning' : 'Auto-hide reasoning'}
					</button>
					<button
						type="button"
						onclick={() => (autoCompactTools = !autoCompactTools)}
						class={`rounded-full px-3 py-2 transition ${autoCompactTools ? 'bg-teal-900 text-teal-50' : 'bg-stone-900/6 text-stone-700 hover:bg-stone-900/10'}`}
					>
						{autoCompactTools ? 'Tools stay compact' : 'Expand tool details'}
					</button>
					<button
						type="button"
						onclick={openLatestDiff}
						disabled={!latestDiffId}
						class="rounded-full bg-stone-900/6 px-3 py-2 transition hover:bg-stone-900/10 disabled:cursor-not-allowed disabled:opacity-45"
					>
						Jump to latest diff
					</button>
					<button
						type="button"
						onclick={jumpToLatest}
						class="rounded-full bg-stone-900/6 px-3 py-2 transition hover:bg-stone-900/10"
					>
						Jump to latest turn
					</button>
				</div>
			</details>
		</div>

		<div class="mt-5 flex flex-wrap gap-2 text-xs font-[var(--font-ui)] text-stone-600">
			{#each badges as badge}
				<span class="rounded-full border border-stone-900/10 bg-white/65 px-3 py-1.5">{badge}</span>
			{/each}
			<span class="rounded-full border border-stone-900/10 bg-white/65 px-3 py-1.5"
				>{activeTurn ? 'Live turn active' : 'Thread settled'}</span
			>
		</div>
	</div>

	<div bind:this={transcriptRoot} class="space-y-4 px-4 py-4 md:px-7 md:py-6">
		{#if sessionBanner}
			<section class={`rounded-[24px] border px-4 py-4 shadow-[0_12px_30px_rgba(0,0,0,0.05)] ${transcriptSessionTone(sessionBanner.state)}`}>
				<div class="flex flex-wrap items-start justify-between gap-4">
					<div>
						<p class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] uppercase opacity-72">
							Session state
						</p>
						<h4 class="mt-2 text-[1.65rem] leading-tight font-[var(--font-display)] tracking-[-0.03em]">
							{sessionBanner.title}
						</h4>
						<p class="mt-3 max-w-3xl text-sm leading-6 font-[var(--font-ui)] opacity-88">
							{sessionBanner.detail}
						</p>
					</div>
					{#if sessionBanner.primaryAction && sessionBanner.primaryLabel && onSessionAction}
						<button
							type="button"
							onclick={() => onSessionAction?.(sessionBanner.primaryAction!)}
							class="rounded-full bg-stone-950 px-4 py-2 text-sm font-[var(--font-ui)] text-stone-50 transition hover:bg-stone-800"
						>
							{sessionBanner.primaryLabel}
						</button>
					{/if}
				</div>
			</section>
		{/if}

		{#if activeTurn || latestDiffId || firstPendingCellId}
			<section class="grid gap-3 md:grid-cols-3">
				<section class="rounded-[22px] border border-stone-900/8 bg-white/72 px-4 py-4 shadow-[0_10px_26px_rgba(0,0,0,0.05)]">
					<p class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] text-stone-500 uppercase">
						Live signal
					</p>
					<h4 class="mt-3 text-[1.45rem] leading-tight font-[var(--font-display)] tracking-[-0.03em] text-stone-950">
						{activeTurn ? 'The thread is streaming now.' : 'The transcript is settled.'}
					</h4>
					<p class="mt-3 text-sm leading-6 font-[var(--font-ui)] text-stone-700">
						{activeTurn
							? 'Stay in follow mode or jump back to the latest live cell whenever you scroll away.'
							: 'Review the latest changes, then resume the thread when you are ready to steer again.'}
					</p>
					<p class="mt-3 text-[11px] font-[var(--font-ui)] tracking-[0.16em] text-stone-500 uppercase">
						{liveFollowLabel()}
					</p>
					{#if liveBacklogBadges.length}
						<div class="mt-3 flex flex-wrap gap-2 text-[11px] font-[var(--font-ui)] tracking-[0.16em] text-stone-600 uppercase">
							{#each liveBacklogBadges as badge}
								<span class="rounded-full border border-stone-900/10 bg-stone-950/4 px-2.5 py-1.5">
									{badge}
								</span>
							{/each}
						</div>
					{/if}
					<button
						type="button"
						onclick={jumpToLatest}
						class="mt-4 rounded-full bg-stone-950 px-3 py-2 text-xs font-[var(--font-ui)] tracking-[0.18em] text-stone-50 uppercase transition hover:bg-stone-800"
					>
						{latestJumpLabel()}
					</button>
				</section>

				<section class="rounded-[22px] border border-emerald-900/10 bg-emerald-50/70 px-4 py-4 shadow-[0_10px_26px_rgba(0,0,0,0.05)]">
					<p class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] text-emerald-900/58 uppercase">
						Diff review
					</p>
					<h4 class="mt-3 text-[1.45rem] leading-tight font-[var(--font-display)] tracking-[-0.03em] text-stone-950">
						{latestDiffId
							? `${totalDiffFiles} changed file${totalDiffFiles === 1 ? '' : 's'}`
							: 'No diffs captured yet.'}
					</h4>
					<p class="mt-3 text-sm leading-6 font-[var(--font-ui)] text-stone-700">
						{latestDiffId
							? `+${totalDiffAdditions} additions, -${totalDiffDeletions} deletions across ${totalDiffHunks} hunk${totalDiffHunks === 1 ? '' : 's'}.`
							: 'When a turn edits files, the latest diff is promoted here for faster review.'}
					</p>
					<button
						type="button"
						onclick={openLatestDiff}
						disabled={!latestDiffId}
						class="mt-4 rounded-full bg-emerald-950 px-3 py-2 text-xs font-[var(--font-ui)] tracking-[0.18em] text-emerald-50 uppercase transition hover:bg-emerald-900 disabled:cursor-not-allowed disabled:opacity-45"
					>
						Open latest diff
					</button>
				</section>

				<section class="rounded-[22px] border border-amber-900/10 bg-amber-50/72 px-4 py-4 shadow-[0_10px_26px_rgba(0,0,0,0.05)]">
					<p class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] text-amber-900/58 uppercase">
						Response queue
					</p>
					<h4 class="mt-3 text-[1.45rem] leading-tight font-[var(--font-display)] tracking-[-0.03em] text-stone-950">
						{firstPendingCellId
							? `${pendingApprovalCells.length + pendingQuestionCells.length} item${pendingApprovalCells.length + pendingQuestionCells.length === 1 ? '' : 's'} need you`
							: 'No approvals or questions are waiting.'}
					</h4>
					<p class="mt-3 text-sm leading-6 font-[var(--font-ui)] text-stone-700">
						{firstPendingCellId
							? `${pendingApprovalCells.length} approval${pendingApprovalCells.length === 1 ? '' : 's'} and ${pendingQuestionCells.length} question${pendingQuestionCells.length === 1 ? '' : 's'} remain in the transcript.`
							: 'You can keep reading without losing your place because nothing is blocked right now.'}
					</p>
					<button
						type="button"
						onclick={jumpToPending}
						disabled={!firstPendingCellId}
						class="mt-4 rounded-full bg-amber-900 px-3 py-2 text-xs font-[var(--font-ui)] tracking-[0.18em] text-amber-50 uppercase transition hover:bg-amber-800 disabled:cursor-not-allowed disabled:opacity-45"
					>
						Jump to blocker
					</button>
				</section>
			</section>
		{/if}

		{#if reviewArtifacts.length}
			<section class="rounded-[24px] border border-stone-900/8 bg-white/68 px-4 py-4 shadow-[0_12px_30px_rgba(0,0,0,0.05)] md:px-5">
				<div class="flex flex-col gap-3 md:flex-row md:items-end md:justify-between">
					<div>
						<p class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] text-stone-500 uppercase">
							Review board
						</p>
						<h4 class="mt-2 text-[1.65rem] leading-tight font-[var(--font-display)] tracking-[-0.03em] text-stone-950">
							Artifacts worth reviewing stay grouped here.
						</h4>
						<p class="mt-3 max-w-3xl text-sm leading-6 font-[var(--font-ui)] text-stone-700">
							Jump straight to diffs, code blocks, tool output, and failures. Local review notes stay attached to this thread in this browser so handoff comments do not disappear into the transcript.
						</p>
					</div>
					<span class="rounded-full border border-stone-900/10 bg-stone-950 px-3 py-2 text-[11px] font-[var(--font-ui)] tracking-[0.18em] text-stone-50 uppercase">
						{reviewArtifacts.length} artifact{reviewArtifacts.length === 1 ? '' : 's'}
					</span>
				</div>
				<div class="mt-4 grid gap-3 xl:grid-cols-2">
					{#each reviewArtifacts as artifact}
						<section class={`rounded-[22px] border p-4 ${reviewArtifactTone(artifact)}`}>
							<div class="flex flex-wrap items-start justify-between gap-3">
								<div>
									<p class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.2em] text-stone-500 uppercase">
										{reviewArtifactKindLabel(artifact)}
									</p>
									<h5 class="mt-2 text-[1.35rem] leading-tight font-[var(--font-display)] tracking-[-0.03em] text-stone-950">
										{artifact.title}
									</h5>
								</div>
								<p class="text-[11px] font-[var(--font-ui)] tracking-[0.16em] text-stone-500 uppercase">
									{artifact.meta}
								</p>
							</div>
							<p class="mt-3 text-sm leading-6 font-[var(--font-ui)] text-stone-700">
								{artifact.summary}
							</p>
							<div class="mt-4 flex flex-wrap gap-2 text-xs font-[var(--font-ui)] tracking-[0.18em] uppercase">
								<button
									type="button"
									onclick={() => jumpToReviewArtifact(artifact)}
									class="rounded-full bg-stone-950 px-3 py-2 text-stone-50 transition hover:bg-stone-800"
								>
									Jump to artifact
								</button>
								{#if artifact.copyText}
									<button
										type="button"
										onclick={() => copyText(`${artifact.id}-artifact`, artifact.copyText!)}
										class="rounded-full bg-white px-3 py-2 text-stone-700 ring-1 ring-stone-900/10 transition hover:bg-stone-100"
									>
										{copyLabel(`${artifact.id}-artifact`, artifact.copyLabel ?? 'Copy')}
									</button>
								{/if}
							</div>
							<label class="mt-4 grid gap-2 text-sm font-[var(--font-ui)] text-stone-700">
								<span class="text-[0.68rem] tracking-[0.18em] text-stone-500 uppercase">Review note</span>
								<textarea
									rows="3"
									value={reviewNoteValue(artifact.id)}
									oninput={(event) => updateReviewNote(artifact.id, (event.currentTarget as HTMLTextAreaElement).value)}
									class="rounded-[20px] border border-stone-900/10 bg-white/75 px-4 py-3 text-sm text-stone-900 placeholder:text-stone-500 focus:border-emerald-300/60 focus:ring-0"
									placeholder="Capture follow-up, handoff context, or why this artifact matters..."
								></textarea>
							</label>
						</section>
					{/each}
				</div>
			</section>
		{/if}

		{#if activeTurn && !followsLiveRegion && (unseenCellCount > 0 || hasBufferedLiveChange)}
			<div class="sticky bottom-4 z-10 flex justify-end">
				<div class="flex flex-wrap items-center justify-end gap-2 rounded-full bg-stone-950/96 px-3 py-2 text-stone-50 shadow-[0_12px_24px_rgba(0,0,0,0.18)] backdrop-blur">
					<span class="px-2 text-[11px] font-[var(--font-ui)] tracking-[0.18em] text-stone-300 uppercase">
						{liveFollowLabel()}
					</span>
					{#each liveBacklogBadges as badge}
						<span class="rounded-full border border-white/12 bg-white/8 px-2.5 py-1 text-[11px] font-[var(--font-ui)] tracking-[0.16em] text-stone-100 uppercase">
							{badge}
						</span>
					{/each}
					{#if liveBacklog.firstBlockerCellId}
						<button
							type="button"
							onclick={() => jumpToCell(liveBacklog.firstBlockerCellId)}
							class="rounded-full bg-amber-300 px-3 py-2 text-xs font-[var(--font-ui)] tracking-[0.18em] text-stone-950 uppercase transition hover:bg-amber-200"
						>
							Jump to blocker
						</button>
					{/if}
					<button
						type="button"
						onclick={jumpToLatest}
						class="rounded-full bg-white px-4 py-2 text-sm font-[var(--font-ui)] text-stone-950 transition hover:bg-stone-100"
					>
						{latestJumpLabel()}
					</button>
				</div>
			</div>
		{/if}

		{#if !cells.length}
			<div
				class="rounded-[26px] border border-dashed border-stone-900/12 bg-white/60 px-5 py-8 text-sm leading-7 font-[var(--font-ui)] text-stone-600"
			>
				{emptyMessage}
			</div>
		{:else}
			{#each cells as cell}
				{#if cell.kind === 'user_message'}
					<section
						data-transcript-cell-id={cell.id}
						class="ml-auto max-w-3xl rounded-[26px] bg-stone-950 px-5 py-4 text-stone-50 shadow-[0_14px_30px_rgba(0,0,0,0.14)]"
					>
						<p
							class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] text-stone-400 uppercase"
						>
							You · {cell.timestamp}
						</p>
						<p class="mt-3 text-[15px] leading-7 font-[var(--font-ui)] whitespace-pre-wrap">
							{cell.text}
						</p>
					</section>
				{:else if cell.kind === 'assistant_message'}
					<section
						data-transcript-cell-id={cell.id}
						class="max-w-4xl rounded-[30px] border border-stone-900/8 bg-white px-5 py-5 shadow-[0_18px_40px_rgba(37,34,28,0.08)]"
					>
						<p
							class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] text-stone-500 uppercase"
						>
							EveryCode · {cell.timestamp}
						</p>
						<h4 class={assistantTitleClass(cell)}>
							{cell.title}
						</h4>
						<div class="mt-4 space-y-4 text-[15px] leading-7 font-[var(--font-ui)] text-stone-700">
							{#each cell.blocks as block, blockIndex}
								{#if block.type === 'paragraph'}
									<p>{block.text}</p>
								{:else if block.type === 'bullets'}
									<ul class="space-y-2 pl-5">
										{#each block.items as item}
											<li class="list-disc">{item}</li>
										{/each}
									</ul>
								{:else}
									<div class="rounded-[20px] bg-stone-950 p-3 text-stone-100">
										<div class="mb-2 flex items-center justify-between gap-3">
											<span class="rounded-full bg-white/8 px-3 py-1 text-[11px] font-[var(--font-ui)] tracking-[0.18em] text-stone-200 uppercase">
												{highlightLanguageLabel(block.language)}
											</span>
											<button
												type="button"
												onclick={() => copyText(`${cell.id}-code-${blockIndex}`, block.text)}
												class="rounded-full bg-white/10 px-3 py-1 text-[11px] font-[var(--font-ui)] tracking-[0.18em] uppercase transition hover:bg-white/16"
											>
												{copyLabel(`${cell.id}-code-${blockIndex}`)}
											</button>
										</div>
										<pre class="overflow-x-auto px-1 py-1 text-[13px] leading-6 text-stone-100"><code>{#each codeBlockLines(cell.id, blockIndex) as lineTokens}<span class="block whitespace-pre">{#each lineTokens as token}<span class={highlightToneClass(token.tone)}>{token.text}</span>{/each}</span>{/each}</code></pre>
									</div>
								{/if}
							{/each}
						</div>
					</section>
				{:else if cell.kind === 'thinking'}
					<section
						data-transcript-cell-id={cell.id}
						class={`max-w-3xl rounded-[26px] border px-4 py-4 transition ${cell.state === 'active' ? 'border-teal-700/20 bg-teal-950 text-teal-50 shadow-[0_18px_30px_rgba(7,50,48,0.18)]' : 'border-stone-900/8 bg-[#f4efe7] text-stone-800'}`}
					>
						<div class="flex flex-wrap items-center justify-between gap-3">
							<div class="flex items-center gap-2">
								<span
									class={`h-2.5 w-2.5 rounded-full ${cell.state === 'active' ? 'animate-pulse bg-teal-300' : 'bg-amber-500'}`}
								></span>
								<p
									class={`text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] uppercase ${cell.state === 'active' ? 'text-teal-200' : 'text-stone-500'}`}
								>
									{cell.state === 'active' ? 'Thinking live' : 'Reasoning'}
								</p>
							</div>
							<div
								class={`flex items-center gap-2 text-xs font-[var(--font-ui)] ${cell.state === 'active' ? 'text-teal-100/75' : 'text-stone-500'}`}
							>
								<span>{cell.timestamp}</span>
								<span>·</span>
								<span>{cell.duration}</span>
								{#if cell.pinned}
									<span
										class="rounded-full bg-stone-950/10 px-2 py-0.5 text-[11px] tracking-[0.18em] uppercase"
										>Pinned</span
									>
								{/if}
								{#if cell.state !== 'active' && isThinkingFresh(cell)}
									<span
										class="rounded-full bg-amber-500/14 px-2 py-0.5 text-[11px] tracking-[0.18em] text-amber-700 uppercase"
										>Just finished</span
									>
								{/if}
							</div>
						</div>

						{#if shouldExpandThinking(cell)}
							<p
								class="mt-3 text-[1.45rem] leading-tight font-[var(--font-display)] tracking-[-0.03em]"
							>
								{cell.summary}
							</p>
							<div
								class={`mt-3 space-y-3 text-sm leading-6 font-[var(--font-ui)] ${cell.state === 'active' ? 'text-teal-50/82' : 'text-stone-700'}`}
							>
								{#each cell.content as paragraph}
									<p>{paragraph}</p>
								{/each}
							</div>
							{#if cell.state !== 'active'}
								<div class="mt-4 flex flex-wrap gap-2 text-xs font-[var(--font-ui)]">
									<button
										type="button"
										onclick={() => toggleThinkingExpanded(cell)}
										class="rounded-full bg-stone-950 px-3 py-1.5 tracking-[0.18em] text-stone-50 uppercase"
									>
										Collapse reasoning
									</button>
								</div>
							{/if}
						{:else}
							<div
								class="mt-3 flex items-center justify-between gap-4 rounded-[18px] bg-white/60 px-4 py-3 text-sm font-[var(--font-ui)] text-stone-700"
							>
								<p>{cell.summary}</p>
								<button
									type="button"
									onclick={() => toggleThinkingExpanded(cell)}
									class="rounded-full bg-stone-950 px-3 py-1.5 text-[11px] tracking-[0.18em] text-stone-50 uppercase"
									>Expand reasoning</button
								>
							</div>
						{/if}
					</section>
				{:else if cell.kind === 'tool_call'}
					<section
						data-transcript-cell-id={cell.id}
						class={`max-w-4xl rounded-[28px] border px-4 py-4 ${cell.state === 'running' ? 'border-sky-900/20 bg-sky-950 text-sky-50 shadow-[0_16px_30px_rgba(13,71,88,0.18)]' : cell.state === 'failed' ? 'border-rose-900/20 bg-rose-950 text-rose-50' : 'border-stone-900/8 bg-[#f8f3ec] text-stone-900'}`}
					>
						<div class="flex flex-wrap items-start justify-between gap-3">
							<div>
								<p
									class={`text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] uppercase ${cell.state === 'running' ? 'text-sky-200' : 'text-stone-500'}`}
								>
									{cell.label}
								</p>
								<p
									class="mt-2 text-[1.45rem] leading-tight font-[var(--font-display)] tracking-[-0.03em]"
								>
									{cell.command}
								</p>
							</div>
							<div
								class={`flex items-center gap-2 text-xs font-[var(--font-ui)] ${cell.state === 'running' ? 'text-sky-100/75' : 'text-stone-500'}`}
							>
								<span>{cell.timestamp}</span>
								<span>·</span>
								<span>{cell.duration}</span>
								<span
									class={`rounded-full px-2.5 py-1 ${cell.state === 'running' ? 'bg-sky-400/14 text-sky-100 ring-1 ring-sky-400/25' : 'bg-stone-900/6 text-stone-700'}`}
									>{cell.exitSummary}</span
								>
								{#if cell.state !== 'running' && isToolFresh(cell)}
									<span
										class="rounded-full bg-amber-500/14 px-2 py-1 text-[11px] tracking-[0.18em] text-amber-700 uppercase"
										>Just finished</span
									>
								{/if}
							</div>
						</div>

						{#if shouldExpandTool(cell)}
							<div
								class="mt-4 overflow-hidden rounded-[22px] bg-black/80 shadow-inner shadow-black/30"
							>
								<div class="flex justify-end px-4 pt-3">
									<button
										type="button"
										onclick={() => copyText(`${cell.id}-tool-output`, cell.output.join('\n'))}
										class="rounded-full bg-white/10 px-3 py-1 text-[11px] font-[var(--font-ui)] tracking-[0.18em] uppercase text-stone-100 transition hover:bg-white/16"
									>
										{copyLabel(`${cell.id}-tool-output`)}
									</button>
								</div>
								<pre
									class={`overflow-y-auto px-4 py-4 text-[13px] leading-6 font-[var(--font-code)] ${cell.state === 'running' ? 'max-h-[260px] text-sky-50' : 'max-h-[240px] text-stone-100'}`}><code>{#each toolOutputLines(cell) as lineTokens}<span class="block whitespace-pre">{#each lineTokens as token}<span class={highlightToneClass(token.tone)}>{token.text}</span>{/each}</span>{/each}</code></pre>
							</div>
							{#if cell.state !== 'running'}
								<div
									class="mt-4 flex flex-wrap gap-2 text-xs font-[var(--font-ui)] tracking-[0.18em] uppercase"
								>
									<button
										type="button"
										onclick={() => toggleToolExpanded(cell)}
										class="rounded-full bg-stone-950 px-3 py-1.5 text-stone-50"
										>Collapse output</button
									>
								</div>
							{/if}
						{:else}
							<div
								class="mt-4 flex flex-wrap items-center justify-between gap-3 rounded-[20px] bg-white/70 px-4 py-3 text-sm font-[var(--font-ui)] text-stone-700"
							>
								<p class="max-w-2xl leading-6">{cell.preview}</p>
								<button
									type="button"
									onclick={() => toggleToolExpanded(cell)}
									class="rounded-full bg-stone-950 px-3 py-1.5 text-[11px] tracking-[0.18em] text-stone-50 uppercase"
									>Expand output</button
								>
							</div>
						{/if}
					</section>
				{:else if cell.kind === 'diff'}
					{@const summary = diffSummary(cell)}
					<section
						data-transcript-cell-id={cell.id}
						class={`max-w-4xl rounded-[28px] border border-emerald-900/12 bg-[#eef4ef] px-4 py-4 text-stone-900 shadow-[0_16px_30px_rgba(43,84,67,0.08)] ${isFullDiff(cell) ? 'ring-2 ring-emerald-300/45' : ''}`}
					>
						<div class="flex flex-wrap items-start justify-between gap-3">
							<div>
								<p
									class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] text-emerald-800/70 uppercase"
								>
									Diff
								</p>
								<h4
									class="mt-2 text-[1.55rem] leading-tight font-[var(--font-display)] tracking-[-0.03em] text-stone-950"
								>
									{cell.summary}
								</h4>
								<p class="mt-2 text-sm font-[var(--font-ui)] text-stone-700">
									{cell.stats} · {cell.previewFile}
								</p>
								{#if summary.files.length}
									<div class="mt-3 flex flex-wrap gap-2 text-[11px] font-[var(--font-ui)] tracking-[0.16em] text-emerald-950/72 uppercase">
										{#each summary.files.slice(0, 4) as file}
											<span class="rounded-full border border-emerald-950/10 bg-white/58 px-2.5 py-1">
												{file.path} · +{file.additions} -{file.deletions}
											</span>
										{/each}
										{#if summary.files.length > 4}
											<span class="rounded-full border border-emerald-950/10 bg-white/58 px-2.5 py-1">
												+{summary.files.length - 4} more file{summary.files.length - 4 === 1 ? '' : 's'}
											</span>
										{/if}
									</div>
								{/if}
							</div>
							<div
								class="flex flex-wrap gap-2 text-[11px] font-[var(--font-ui)] tracking-[0.18em] uppercase"
							>
								<button
									type="button"
									onclick={() => cycleDiffView(cell)}
									class="rounded-full bg-stone-950 px-3 py-1.5 text-stone-50"
								>
									{isCompactDiff(cell)
										? 'Expand diff'
										: isFullDiff(cell)
											? 'Return to compact'
											: 'Open full diff'}
								</button>
							</div>
						</div>

						<div
							class="mt-4 overflow-hidden rounded-[22px] border border-emerald-950/8 bg-[#13211b]"
						>
							<div class="flex flex-wrap items-center justify-between gap-3 border-b border-white/8 px-4 py-3">
								<div class="flex flex-wrap gap-2 text-[11px] font-[var(--font-ui)] tracking-[0.16em] text-emerald-50/82 uppercase">
									<span>{summary.files.length || 1} file{summary.files.length === 1 ? '' : 's'}</span>
									<span>+{summary.totalAdditions}</span>
									<span>-{summary.totalDeletions}</span>
									<span>{summary.totalHunks} hunk{summary.totalHunks === 1 ? '' : 's'}</span>
								</div>
								<button
									type="button"
									onclick={() => copyText(`${cell.id}-diff`, cell.patch)}
									class="rounded-full bg-white/10 px-3 py-1 text-[11px] font-[var(--font-ui)] tracking-[0.18em] uppercase text-emerald-50 transition hover:bg-white/16"
								>
									{copyLabel(`${cell.id}-diff`)}
								</button>
							</div>
							<div
								class={`overflow-auto ${isCompactDiff(cell) ? 'max-h-[220px]' : isFullDiff(cell) ? 'max-h-[560px]' : 'max-h-[340px]'}`}
							>
								<div class="min-w-max px-2 py-3 text-[13px] leading-6 font-[var(--font-code)]">
									{#each diffLines(cell) as diffLine, lineIndex}
										<div class={`grid grid-cols-[auto_minmax(0,1fr)] rounded-md border-l-2 ${diffLineTone(diffLine.kind)}`}>
											<span class="px-3 py-0.5 text-[10px] text-current/55 select-none">
												{lineIndex + 1}
											</span>
											<code class="block whitespace-pre pr-4 py-0.5">{#each diffLine.tokens as token}<span class={highlightToneClass(token.tone)}>{token.text}</span>{/each}</code>
										</div>
									{/each}
								</div>
							</div>
						</div>
					</section>
				{:else if cell.kind === 'approval'}
					{@const approvalCell = cell as ApprovalCell}
					<section
						data-transcript-cell-id={approvalCell.id}
						class="max-w-3xl rounded-[28px] border border-amber-900/18 bg-[#22140c] px-4 py-4 text-amber-50 shadow-[0_18px_30px_rgba(34,20,12,0.25)]"
					>
						<p
							class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] text-amber-200/76 uppercase"
						>
							{approvalCell.status === 'pending' ? 'Approval needed' : 'Approval answered'} · {approvalCell.timestamp}
						</p>
						<h4
							class="mt-3 text-[1.7rem] leading-tight font-[var(--font-display)] tracking-[-0.03em]"
						>
							{approvalCell.label}
						</h4>
						<p class="mt-3 text-sm leading-6 font-[var(--font-ui)] text-amber-50/86">
							{approvalCell.detail}
						</p>
						{#if approvalCell.status === 'pending' && canApprove && onApprovalAction}
							<div class="mt-4 flex flex-wrap gap-3 text-sm font-[var(--font-ui)]">
								<button
									type="button"
									onclick={() => onApprovalAction?.(approvalCell.id, approvalCell.primary)}
									class="rounded-2xl bg-amber-300 px-4 py-2 text-amber-950 transition hover:bg-amber-200"
								>
									{approvalCell.primary}
								</button>
								<button
									type="button"
									onclick={() => onApprovalAction?.(approvalCell.id, approvalCell.secondary)}
									class="rounded-2xl border border-amber-200/20 bg-transparent px-4 py-2 text-amber-50 transition hover:bg-amber-200/8"
								>
									{approvalCell.secondary}
								</button>
							</div>
							<p class="mt-3 text-[11px] font-[var(--font-ui)] tracking-[0.16em] text-amber-100/70 uppercase">
								Keyboard: Y approve, N decline
							</p>
						{:else}
							<p class="mt-4 text-sm font-[var(--font-ui)] text-amber-50/86">
								{#if approvalCell.status === 'answered'}
									<span class="font-semibold text-amber-100"
										>{responseOriginLabel(approvalCell.respondedBy, approvalCell.device ?? 'another device')}</span
									>: {approvalCell.resolution}
								{:else}
									Awaiting a structured response from another device.
								{/if}
							</p>
						{/if}
					</section>
				{:else if cell.kind === 'question'}
					{@const questionCell = cell as QuestionCell}
					<section
						data-transcript-cell-id={questionCell.id}
						class="max-w-3xl rounded-[28px] border border-stone-900/8 bg-white px-4 py-4 shadow-[0_16px_30px_rgba(41,34,24,0.08)]"
					>
						<p
							class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] text-stone-500 uppercase"
						>
							Question · {questionCell.timestamp}
						</p>
						<p class="mt-2 text-[11px] font-[var(--font-ui)] tracking-[0.18em] text-stone-500 uppercase">
							{questionCell.header || 'Question group'}
						</p>
						<h4
							class="mt-3 text-[1.65rem] leading-tight font-[var(--font-display)] tracking-[-0.03em] text-stone-950"
						>
							{questionCell.prompt}
						</h4>
						{#if questionCell.status === 'pending'}
							{#if questionCell.options.length}
								<div class="mt-4 flex flex-wrap gap-2">
									{#each questionCell.options as option}
										<button
											type="button"
											onclick={() => onQuestionOptionSelect?.(questionCell.id, option.label)}
											class={`rounded-full px-3 py-2 text-xs font-[var(--font-ui)] transition ${questionDraft(questionCell).selectedOption === option.label ? 'bg-stone-950 text-stone-50' : 'bg-stone-900/6 text-stone-700 hover:bg-stone-900/10'}`}
										>
											{option.label}
										</button>
									{/each}
									{#if questionCell.allowOther}
										<button
											type="button"
											onclick={() => onQuestionOptionSelect?.(questionCell.id, '__other')}
											class={`rounded-full px-3 py-2 text-xs font-[var(--font-ui)] transition ${questionDraft(questionCell).selectedOption === '__other' ? 'bg-amber-200 text-amber-950' : 'bg-stone-900/6 text-stone-700 hover:bg-stone-900/10'}`}
										>
											Other
										</button>
									{/if}
								</div>
							{/if}
							{#if questionCell.allowOther || questionCell.allowSecret || !questionCell.options.length}
								<input
									type={questionCell.allowSecret ? 'password' : 'text'}
									value={questionDraft(questionCell).customValue}
									oninput={(event) =>
										onQuestionCustomValue?.(
											questionCell.id,
											(event.currentTarget as HTMLInputElement).value
										)}
									class="mt-4 w-full rounded-2xl border border-stone-900/10 bg-stone-900/[0.04] px-4 py-3 text-sm text-stone-900 placeholder:text-stone-500 focus:border-amber-300/60 focus:ring-0"
									placeholder={questionCell.allowSecret
										? 'Enter secret answer'
										: questionCell.allowOther
											? 'Enter another answer'
											: 'Enter your answer'}
								/>
							{/if}
							<div class="mt-4 flex flex-wrap gap-3">
								{#if questionCell.questionCountInGroup === 1 && onQuestionSubmit}
									<button
										type="button"
										onclick={() => onQuestionSubmit?.(questionCell.id)}
										disabled={!questionAnswerReady(questionCell)}
										class="rounded-full bg-stone-950 px-4 py-2 text-sm font-[var(--font-ui)] text-stone-50 transition hover:bg-stone-800 disabled:cursor-not-allowed disabled:opacity-45"
									>
										Send answer
									</button>
								{/if}
								{#if questionCell.questionCountInGroup > 1 && onQuestionFocus}
									<button
										type="button"
										onclick={() => onQuestionFocus?.(questionCell.id)}
										class="rounded-full bg-stone-950 px-4 py-2 text-sm font-[var(--font-ui)] text-stone-50 transition hover:bg-stone-800"
									>
										Open grouped responder
									</button>
								{/if}
							</div>
						{:else}
							<p class="mt-4 text-sm leading-6 font-[var(--font-ui)] text-stone-700">
								Options: {questionCell.options.map((option) => option.label).join(' · ')}
							</p>
						{/if}
						{#if questionCell.answer}
							<p class="mt-4 text-sm font-[var(--font-ui)] text-stone-600">
								<span class="font-semibold text-stone-900"
									>{responseOriginLabel(questionCell.respondedBy)}</span
								>: {questionCell.answer}
							</p>
						{/if}
					</section>
				{:else if cell.kind === 'error'}
					<section
						data-transcript-cell-id={cell.id}
						class="max-w-3xl rounded-[28px] border border-rose-900/14 bg-[#fff1ef] px-4 py-4 text-stone-900 shadow-[0_16px_30px_rgba(102,41,41,0.07)]"
					>
						<p
							class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] text-rose-700/70 uppercase"
						>
							{cell.status} · {cell.timestamp}
						</p>
						<h4
							class="mt-3 text-[1.55rem] leading-tight font-[var(--font-display)] tracking-[-0.03em] text-stone-950"
						>
							{cell.message}
						</h4>
						<p class="mt-3 text-sm leading-6 font-[var(--font-ui)] text-stone-700">{cell.detail}</p>
					</section>
				{/if}
			{/each}
		{/if}
	</div>
</section>

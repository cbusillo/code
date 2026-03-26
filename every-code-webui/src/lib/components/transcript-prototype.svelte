<script lang="ts">
	import { browser } from '$app/environment';
	import { onMount } from 'svelte';
	import {
		selectedSession as initialSelectedSession,
		sessionRail,
		transcriptCells
	} from '$lib/transcript-fixture';
	import type {
		ApprovalCell,
		DiffCell,
		QuestionCell,
		ThinkingCell,
		ToolCallCell,
		TranscriptCell
	} from '$lib/transcript-model';

	type ContinuityMode = 'view-only' | 'respond-remotely' | 'take-over';
	type DiffView = 'compact' | 'expanded' | 'full';
	type PersistedLabState = {
		showAllReasoning: boolean;
		autoCompactTools: boolean;
		continuityMode: ContinuityMode;
		settleActiveTurn: boolean;
		expandedThinking: Record<string, boolean>;
		pinnedThinking: Record<string, boolean>;
		expandedTools: Record<string, boolean>;
		pinnedTools: Record<string, boolean>;
		diffViews: Record<string, DiffView>;
		approvalResponses: Record<string, string>;
		questionResponses: Record<string, string>;
	};

	const STORAGE_KEY = 'every-code-webui-transcript-lab:v1';
	const continuityModes: ContinuityMode[] = ['view-only', 'respond-remotely', 'take-over'];

	let showAllReasoning = $state(false);
	let autoCompactTools = $state(true);
	let continuityMode = $state<ContinuityMode>('view-only');
	let settleActiveTurn = $state(false);
	let composerText = $state('');
	let takeoverQueued = $state(false);
	let hydrated = false;

	let expandedThinking = $state<Record<string, boolean>>({});
	let pinnedThinking = $state<Record<string, boolean>>({});
	let expandedTools = $state<Record<string, boolean>>({});
	let pinnedTools = $state<Record<string, boolean>>({});
	let diffViews = $state<Record<string, DiffView>>({});
	let approvalResponses = $state<Record<string, string>>({});
	let questionResponses = $state<Record<string, string>>({});

	const selectedSession = initialSelectedSession;

	const visibleCells = $derived.by(() => transcriptCells.map((cell) => materializeCell(cell)));
	const activeTurn = $derived.by(() =>
		visibleCells.some(
			(cell) =>
				(cell.kind === 'thinking' && cell.state === 'active') ||
				(cell.kind === 'tool_call' && cell.state === 'running')
		)
	);
	const latestLiveCellId = $derived.by(() => {
		const latest = [...visibleCells].reverse().find((cell) => isLiveOrActionableCell(cell));

		return latest?.id ?? visibleCells.at(-1)?.id ?? '';
	});
	const latestDiffId = $derived.by(() => {
		const latestDiff = [...visibleCells]
			.reverse()
			.find((cell): cell is DiffCell => cell.kind === 'diff');

		return latestDiff?.id ?? '';
	});

	onMount(() => {
		if (!browser) {
			return;
		}

		const rawState = window.localStorage.getItem(STORAGE_KEY);
		if (rawState) {
			try {
				const parsedState = JSON.parse(rawState) as Partial<PersistedLabState>;

				showAllReasoning = parsedState.showAllReasoning ?? showAllReasoning;
				autoCompactTools = parsedState.autoCompactTools ?? autoCompactTools;
				continuityMode = parsedState.continuityMode ?? continuityMode;
				settleActiveTurn = parsedState.settleActiveTurn ?? settleActiveTurn;
				expandedThinking = parsedState.expandedThinking ?? expandedThinking;
				pinnedThinking = parsedState.pinnedThinking ?? pinnedThinking;
				expandedTools = parsedState.expandedTools ?? expandedTools;
				pinnedTools = parsedState.pinnedTools ?? pinnedTools;
				diffViews = parsedState.diffViews ?? diffViews;
				approvalResponses = parsedState.approvalResponses ?? approvalResponses;
				questionResponses = parsedState.questionResponses ?? questionResponses;
			} catch {
				window.localStorage.removeItem(STORAGE_KEY);
			}
		}

		hydrated = true;
	});

	$effect(() => {
		if (!browser || !hydrated) {
			return;
		}

		const persistedState: PersistedLabState = {
			showAllReasoning,
			autoCompactTools,
			continuityMode,
			settleActiveTurn,
			expandedThinking,
			pinnedThinking,
			expandedTools,
			pinnedTools,
			diffViews,
			approvalResponses,
			questionResponses
		};

		window.localStorage.setItem(STORAGE_KEY, JSON.stringify(persistedState));
	});

	$effect(() => {
		if (!activeTurn) {
			takeoverQueued = false;
		}

		if (continuityMode !== 'take-over') {
			takeoverQueued = false;
		}
	});

	function materializeCell(cell: TranscriptCell): TranscriptCell {
		switch (cell.kind) {
			case 'thinking': {
				const isPinned = pinnedThinking[cell.id] ?? cell.pinned;
				const state = settleActiveTurn && cell.state === 'active' ? 'completed' : cell.state;
				const duration = settleActiveTurn && cell.state === 'active' ? '41s' : cell.duration;

				return { ...cell, pinned: isPinned, state, duration };
			}
			case 'tool_call': {
				const isPinned = pinnedTools[cell.id] ?? cell.pinned;

				if (settleActiveTurn && cell.state === 'running') {
					return {
						...cell,
						pinned: isPinned,
						state: 'completed',
						duration: '5.4s',
						exitSummary: 'exit 0',
						preview: 'Prototype server is reachable and waiting for the next browser interaction.',
						output: [
							...cell.output,
							'  GET / 200 in 18 ms',
							'  browser fetch confirmed transcript HTML content',
							'  dev session settled cleanly'
						]
					};
				}

				return { ...cell, pinned: isPinned };
			}
			case 'approval': {
				const resolution = approvalResponses[cell.id];

				if (!resolution) {
					return cell;
				}

				return {
					...cell,
					status: 'answered',
					resolution,
					respondedBy: 'This browser'
				};
			}
			case 'question': {
				const answer = questionResponses[cell.id];

				if (!answer) {
					return cell;
				}

				return {
					...cell,
					status: 'answered',
					answer,
					respondedBy: 'This browser'
				};
			}
			default:
				return cell;
		}
	}

	function stateTone(state: string) {
		switch (state) {
			case 'live-here':
				return 'bg-emerald-500/15 text-emerald-200 ring-1 ring-emerald-400/30';
			case 'live-elsewhere':
				return 'bg-amber-500/15 text-amber-100 ring-1 ring-amber-400/30';
			case 'waiting':
				return 'bg-sky-500/15 text-sky-100 ring-1 ring-sky-400/30';
			default:
				return 'bg-white/8 text-stone-200 ring-1 ring-white/12';
		}
	}

	function continuityTone(mode: ContinuityMode) {
		switch (mode) {
			case 'respond-remotely':
				return 'border-sky-300/25 bg-sky-200/10 text-sky-50';
			case 'take-over':
				return 'border-amber-300/30 bg-amber-200/12 text-amber-50';
			default:
				return 'border-white/12 bg-white/8 text-stone-100';
		}
	}

	function continuityModeLabel(mode: ContinuityMode) {
		switch (mode) {
			case 'respond-remotely':
				return 'Respond remotely';
			case 'take-over':
				return 'Take over';
			default:
				return 'View only';
		}
	}

	function continuityModeHint(mode: ContinuityMode) {
		switch (mode) {
			case 'respond-remotely':
				return 'Answer approvals and multiple-choice questions from this device.';
			case 'take-over':
				return activeTurn
					? 'Stage the next prompt now and take control when the live turn settles.'
					: 'This browser can become the active driver for the next prompt.';
			default:
				return 'Read the thread without taking shell ownership away from the Mac.';
		}
	}

	function continuityHeading() {
		switch (continuityMode) {
			case 'respond-remotely':
				return 'Answer approvals from this device';
			case 'take-over':
				return activeTurn
					? 'Take over is queued behind the live turn'
					: 'This device is ready to continue';
			default:
				return 'Read only on this device';
		}
	}

	function continuityBadge() {
		if (continuityMode === 'take-over' && !activeTurn) {
			return 'Ready here';
		}

		if (continuityMode === 'take-over' && activeTurn) {
			return 'Queued behind Studio Mac';
		}

		if (continuityMode === 'respond-remotely') {
			return 'Structured replies only';
		}

		return 'Live on Studio Mac';
	}

	function continuityBody() {
		switch (continuityMode) {
			case 'respond-remotely':
				return 'You can answer approvals and multiple-choice questions without stealing shell ownership from the active device.';
			case 'take-over':
				return activeTurn
					? 'Write the next prompt now if you want, but it will wait until the running turn settles before this browser becomes the driver.'
					: 'The current turn is settled, so this browser can become the active driver for the next free-form prompt.';
			default:
				return 'You can inspect the full thread here, but the live keyboard and shell stay attached to the Studio Mac until you choose a more active mode.';
		}
	}

	function shouldExpandThinking(cell: ThinkingCell) {
		if (cell.state === 'active' || cell.pinned || showAllReasoning) {
			return true;
		}

		return expandedThinking[cell.id] ?? false;
	}

	function shouldExpandTool(cell: ToolCallCell) {
		if (cell.state === 'running' || cell.pinned || !autoCompactTools) {
			return true;
		}

		return expandedTools[cell.id] ?? false;
	}

	function diffView(cell: DiffCell) {
		return diffViews[cell.id] ?? (cell.autoCompact ? 'compact' : 'expanded');
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

	function jumpToCell(cellId: string) {
		if (!browser || !cellId) {
			return;
		}

		const node = document.querySelector<HTMLElement>(`[data-cell-id="${cellId}"]`);
		node?.scrollIntoView({ behavior: 'smooth', block: 'center' });
	}

	function jumpToLatest() {
		jumpToCell(latestLiveCellId);
	}

	function openLatestDiff() {
		if (!latestDiffId) {
			return;
		}

		diffViews = { ...diffViews, [latestDiffId]: 'full' };
		jumpToCell(latestDiffId);
	}

	function setMode(mode: ContinuityMode) {
		continuityMode = mode;
		takeoverQueued = mode === 'take-over' && activeTurn;
	}

	function toggleThinkingExpanded(cellId: string) {
		expandedThinking = {
			...expandedThinking,
			[cellId]: !(expandedThinking[cellId] ?? false)
		};
	}

	function toggleThinkingPinned(cellId: string) {
		pinnedThinking = {
			...pinnedThinking,
			[cellId]: !(pinnedThinking[cellId] ?? false)
		};
	}

	function toggleToolExpanded(cellId: string) {
		expandedTools = {
			...expandedTools,
			[cellId]: !(expandedTools[cellId] ?? false)
		};
	}

	function toggleToolPinned(cellId: string) {
		pinnedTools = {
			...pinnedTools,
			[cellId]: !(pinnedTools[cellId] ?? false)
		};
	}

	function cycleDiffView(cellId: string) {
		const currentView = diffViews[cellId] ?? 'compact';
		const nextView: DiffView =
			currentView === 'compact' ? 'expanded' : currentView === 'expanded' ? 'full' : 'compact';

		diffViews = { ...diffViews, [cellId]: nextView };
	}

	function answerApproval(cell: ApprovalCell, resolution: string) {
		if (continuityMode === 'view-only') {
			return;
		}

		approvalResponses = { ...approvalResponses, [cell.id]: resolution };
	}

	function answerQuestion(cell: QuestionCell, answer: string) {
		if (continuityMode === 'view-only') {
			return;
		}

		questionResponses = { ...questionResponses, [cell.id]: answer };
	}

	function composerPrimaryLabel() {
		if (continuityMode !== 'take-over') {
			return 'Take over to compose';
		}

		if (activeTurn) {
			return takeoverQueued ? 'Takeover queued' : 'Queue takeover';
		}

		return 'Continue here';
	}

	function composerHint() {
		if (continuityMode === 'view-only') {
			return 'Switch out of view-only mode before composing.';
		}

		if (continuityMode === 'respond-remotely') {
			return 'This mode is for approvals and questions only; free-form prompts still belong to the active device.';
		}

		if (activeTurn) {
			return 'Your next prompt can be staged now and sent once the current turn settles.';
		}

		return 'The live turn is settled, so this browser can continue the thread immediately.';
	}

	function composerDisabled() {
		return continuityMode !== 'take-over';
	}

	function handleComposerPrimary() {
		if (continuityMode !== 'take-over') {
			setMode('take-over');
			return;
		}

		if (activeTurn) {
			takeoverQueued = true;
			return;
		}

		composerText = '';
	}

	function assistantTitleClass(cell: { title: string; blocks: Array<unknown> }) {
		const compactHeading = cell.title.length < 42 && cell.blocks.length <= 1;

		return compactHeading
			? 'mt-3 text-[1.45rem] leading-tight font-[var(--font-display)] tracking-[-0.03em] text-stone-950'
			: 'mt-3 text-[1.85rem] leading-none font-[var(--font-display)] tracking-[-0.04em] text-stone-950';
	}
</script>

<svelte:head>
	<title>EveryCode WebUI Prototype</title>
	<meta
		name="description"
		content="Golden transcript prototype for EveryCode continuity-first WebUI design."
	/>
	<link rel="preconnect" href="https://fonts.googleapis.com" />
	<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin="anonymous" />
	<link
		href="https://fonts.googleapis.com/css2?family=Fraunces:opsz,wght@9..144,500;9..144,600;9..144,700&family=IBM+Plex+Sans:wght@400;500;600&family=IBM+Plex+Mono:wght@400;500&display=swap"
		rel="stylesheet"
	/>
</svelte:head>

<div
	class="min-h-screen overflow-x-clip bg-[radial-gradient(circle_at_top,_rgba(43,84,87,0.42),_transparent_38%),linear-gradient(180deg,_#071012_0%,_#0d1718_45%,_#120f0d_100%)] text-stone-100"
>
	<div class="mx-auto flex min-h-screen max-w-[1600px] flex-col px-4 py-4 lg:px-6 lg:py-6">
		<header
			class="mb-4 grid gap-4 rounded-[28px] border border-white/10 bg-[linear-gradient(135deg,_rgba(255,255,255,0.08),_rgba(255,255,255,0.03))] px-5 py-5 shadow-[0_20px_80px_rgba(0,0,0,0.35)] backdrop-blur md:grid-cols-[1.28fr_0.92fr] lg:mb-6 lg:px-7 lg:py-6"
		>
			<div class="space-y-4">
				<p
					class="text-[0.7rem] font-[var(--font-ui)] tracking-[0.28em] text-amber-200/72 uppercase"
				>
					EveryCode WebUI · Golden transcript lab
				</p>
				<div class="space-y-3">
					<h1
						class="max-w-3xl text-[3.15rem] leading-[0.95] font-[var(--font-display)] tracking-[-0.04em] text-stone-50 sm:text-[3.6rem] md:text-5xl lg:text-[4rem]"
					>
						A continuity-first browser home for EveryCode.
					</h1>
					<p
						class="max-w-2xl text-sm leading-7 font-[var(--font-ui)] text-stone-300 md:text-[15px]"
					>
						This version behaves like a lab instead of a poster. Reasoning, tool output, diffs,
						approvals, and takeover modes can all be exercised in-place so the transcript rules
						become tangible.
					</p>
				</div>
				<div class="flex flex-wrap gap-3 text-xs font-[var(--font-ui)] text-stone-300/80">
					<span class="rounded-full border border-white/10 bg-white/5 px-3 py-1.5"
						>Local TUI -> couch browser continuity</span
					>
					<span class="rounded-full border border-white/10 bg-white/5 px-3 py-1.5"
						>Pinned reasoning persists locally</span
					>
					<span class="rounded-full border border-white/10 bg-white/5 px-3 py-1.5"
						>Compact-by-default tool cells</span
					>
				</div>
			</div>

			<section
				class="grid gap-3 rounded-[24px] border border-white/8 bg-black/18 p-4 shadow-inner shadow-black/20"
			>
				<div class="flex items-center justify-between gap-4">
					<div>
						<p
							class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.24em] text-stone-400 uppercase"
						>
							Current continuity state
						</p>
						<p class="mt-2 text-2xl font-[var(--font-display)] text-stone-50">
							{continuityHeading()}
						</p>
					</div>
					<span
						class="rounded-full bg-amber-500/15 px-3 py-1.5 text-xs font-[var(--font-ui)] text-amber-100 ring-1 ring-amber-400/30"
					>
						{continuityBadge()}
					</span>
				</div>
				<p class="text-sm leading-6 font-[var(--font-ui)] text-stone-300">
					{continuityBody()}
				</p>
				<div class="grid gap-2 text-sm font-[var(--font-ui)] text-stone-300 sm:grid-cols-3">
					<button
						type="button"
						onclick={() => setMode('view-only')}
						class={`rounded-[22px] border px-3 py-3 text-left transition ${continuityMode === 'view-only' ? continuityTone('view-only') : 'border-white/12 bg-white/8 hover:bg-white/12'}`}
					>
						<span class="block text-sm font-medium text-current">View only</span>
						<span class="mt-1 block text-xs leading-5 text-current/72">
							Read the thread without interrupting the Mac.
						</span>
					</button>
					<button
						type="button"
						onclick={() => setMode('respond-remotely')}
						class={`rounded-[22px] border px-3 py-3 text-left transition ${continuityMode === 'respond-remotely' ? continuityTone('respond-remotely') : 'border-white/12 bg-white/8 hover:bg-white/12'}`}
					>
						<span class="block text-sm font-medium text-current">Respond remotely</span>
						<span class="mt-1 block text-xs leading-5 text-current/72">
							Handle approvals and questions without taking over.
						</span>
					</button>
					<button
						type="button"
						onclick={() => setMode('take-over')}
						class={`rounded-[22px] border px-3 py-3 text-left transition ${continuityMode === 'take-over' ? continuityTone('take-over') : 'border-white/12 bg-white/8 hover:bg-white/12'}`}
					>
						<span class="block text-sm font-medium text-current">Take over</span>
						<span class="mt-1 block text-xs leading-5 text-current/72">
							Queue a free-form prompt and continue from this browser.
						</span>
					</button>
				</div>
			</section>
		</header>

		<div class="grid min-h-0 flex-1 gap-4 lg:grid-cols-[320px_minmax(0,1fr)]">
			<aside
				class="overflow-hidden rounded-[28px] border border-white/10 bg-[linear-gradient(180deg,_rgba(255,255,255,0.07),_rgba(255,255,255,0.03))] p-4 shadow-[0_16px_60px_rgba(0,0,0,0.25)] backdrop-blur lg:sticky lg:top-6 lg:self-start"
			>
				<div class="mb-4 flex items-center justify-between">
					<div>
						<p
							class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] text-stone-400 uppercase"
						>
							Session home
						</p>
						<h2 class="mt-2 text-3xl font-[var(--font-display)] text-stone-50">Your threads</h2>
					</div>
					<button
						class="rounded-full border border-white/10 bg-white/5 px-3 py-2 text-xs font-[var(--font-ui)] text-stone-200 transition hover:bg-white/10"
						>New session</button
					>
				</div>

				<div class="flex snap-x gap-3 overflow-x-auto pb-1 lg:block lg:space-y-3 lg:overflow-visible lg:pb-0">
					{#each sessionRail as session}
						<button
							type="button"
							class={`w-[calc(100vw-3.5rem)] min-w-[280px] shrink-0 snap-start rounded-[24px] border px-4 py-4 text-left transition sm:w-[340px] lg:w-full lg:min-w-0 ${session.id === selectedSession.id ? 'border-amber-300/30 bg-amber-100/8 shadow-[0_12px_30px_rgba(0,0,0,0.16)]' : 'border-white/8 bg-black/14 hover:bg-white/6'}`}
						>
							<div class="mb-3 flex items-center justify-between gap-3">
								<span
									class={`rounded-full px-2.5 py-1 text-[11px] font-[var(--font-ui)] tracking-[0.18em] uppercase ${stateTone(session.state)}`}
								>
									{session.state.replace('-', ' ')}
								</span>
								<span class="text-[11px] font-[var(--font-ui)] text-stone-400"
									>{session.updated}</span
								>
							</div>
							<h3 class="text-[1.35rem] leading-tight font-[var(--font-display)] text-stone-50">
								{session.title}
							</h3>
							<p
								class="mt-3 text-xs font-[var(--font-ui)] tracking-[0.18em] text-stone-500 uppercase"
							>
								{session.workspace}
							</p>
							<p class="mt-3 text-sm leading-6 font-[var(--font-ui)] text-stone-300">
								{session.preview}
							</p>
							<p class="mt-3 text-xs font-[var(--font-ui)] text-stone-500">
								Driven from {session.device}
							</p>
						</button>
					{/each}
				</div>
			</aside>

			<main
				class="min-w-0 overflow-hidden rounded-[32px] border border-white/10 bg-[linear-gradient(180deg,_rgba(255,252,247,0.98),_rgba(246,238,227,0.95))] text-stone-900 shadow-[0_24px_90px_rgba(0,0,0,0.34)]"
			>
				<div class="border-b border-stone-900/8 px-5 py-5 md:px-7">
					<div class="flex flex-col gap-4 xl:flex-row xl:items-end xl:justify-between">
						<div>
							<p
								class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] text-stone-500 uppercase"
							>
								Golden transcript fixture
							</p>
							<h2
								class="mt-2 text-4xl leading-none font-[var(--font-display)] tracking-[-0.04em] text-stone-950"
							>
								{selectedSession.title}
							</h2>
							<p class="mt-3 max-w-3xl text-sm leading-7 font-[var(--font-ui)] text-stone-600">
								The lab mixes live, settled, and cross-device states on purpose so we can evaluate
								the transcript system before widening the shell.
							</p>
						</div>
						<div class="-mx-1 flex gap-2 overflow-x-auto px-1 pb-1 text-xs font-[var(--font-ui)] text-stone-700 sm:mx-0 sm:flex-wrap sm:overflow-visible sm:px-0 sm:pb-0">
							<button
								type="button"
								onclick={() => (showAllReasoning = !showAllReasoning)}
								class={`rounded-full px-3 py-2 transition ${showAllReasoning ? 'bg-stone-950 text-stone-50' : 'bg-stone-900/6 text-stone-700 hover:bg-stone-900/10'}`}
							>
								{showAllReasoning ? 'Reasoning: all' : 'Reasoning: auto'}
							</button>
							<button
								type="button"
								onclick={() => (autoCompactTools = !autoCompactTools)}
								class={`rounded-full px-3 py-2 transition ${autoCompactTools ? 'bg-teal-900 text-teal-50' : 'bg-stone-900/6 text-stone-700 hover:bg-stone-900/10'}`}
							>
								{autoCompactTools ? 'Tools: compact' : 'Tools: expanded'}
							</button>
							<button
								type="button"
								onclick={openLatestDiff}
								class="rounded-full bg-stone-900/6 px-3 py-2 transition hover:bg-stone-900/10"
							>
								Latest diff
							</button>
							<button
								type="button"
								onclick={jumpToLatest}
								class="rounded-full bg-stone-900/6 px-3 py-2 transition hover:bg-stone-900/10"
							>
								Latest turn
							</button>
							<button
								type="button"
								onclick={() => (settleActiveTurn = !settleActiveTurn)}
								class={`rounded-full px-3 py-2 transition ${settleActiveTurn ? 'bg-amber-300 text-amber-950' : 'bg-stone-900/6 text-stone-700 hover:bg-stone-900/10'}`}
							>
								{settleActiveTurn ? 'Replay live turn' : 'Settle turn'}
							</button>
						</div>
					</div>

					<div class="mt-5 grid gap-3 lg:grid-cols-[1.1fr_0.9fr]">
						<div
							class="rounded-[24px] border border-stone-900/8 bg-white/65 px-4 py-4 shadow-[inset_0_1px_0_rgba(255,255,255,0.9)]"
						>
							<div class="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
								<div>
									<p
										class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] text-stone-500 uppercase"
									>
										Session continuity
									</p>
									<div
										class="mt-3 flex flex-wrap items-center gap-2 text-sm font-[var(--font-ui)] text-stone-700"
									>
										{#each continuityModes as mode}
											<span
												class={`rounded-full border px-3 py-1.5 ${continuityMode === mode ? mode === 'view-only' ? 'border-stone-900/12 bg-stone-950 text-stone-50' : mode === 'respond-remotely' ? 'border-sky-300/25 bg-sky-100 text-sky-900' : 'border-amber-300/30 bg-amber-100 text-amber-950' : 'bg-stone-100'}`}
											>
												{continuityModeLabel(mode)}
											</span>
										{/each}
									</div>
								</div>
								<div class="max-w-sm space-y-2">
									<p class="text-sm leading-6 font-[var(--font-ui)] text-stone-600">
										{continuityModeHint(continuityMode)}
									</p>
									<p class="text-xs leading-5 font-[var(--font-ui)] text-stone-500">
										Prefs persist in local storage so this fixture behaves like a real reading
										surface.
									</p>
								</div>
							</div>
						</div>
						<div
							class="rounded-[24px] border border-stone-900/8 bg-white/65 px-4 py-4 shadow-[inset_0_1px_0_rgba(255,255,255,0.9)]"
						>
							<p
								class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] text-stone-500 uppercase"
							>
								Current workspace
							</p>
							<p class="mt-3 break-all text-[1.45rem] font-[var(--font-display)] text-stone-950 sm:text-[1.65rem]">
								{selectedSession.workspace}
							</p>
							<p class="mt-2 text-sm font-[var(--font-ui)] text-stone-600">
								Active driver: {selectedSession.device}
							</p>
							<p class="mt-2 text-sm font-[var(--font-ui)] text-stone-600">
								Turn state: {activeTurn ? 'live' : 'settled'}
							</p>
						</div>
					</div>
				</div>

				<div class="space-y-4 px-4 py-4 md:px-7 md:py-6">
					{#each visibleCells as cell}
						{#if cell.kind === 'user_message'}
							<section
								data-cell-id={cell.id}
								class="ml-auto max-w-full overflow-hidden rounded-[26px] bg-stone-950 px-5 py-4 text-stone-50 shadow-[0_14px_30px_rgba(0,0,0,0.14)] sm:max-w-3xl"
							>
								<p
									class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] text-stone-400 uppercase"
								>
									You · {cell.timestamp}
								</p>
								<p class="mt-3 text-[15px] leading-7 font-[var(--font-ui)]">{cell.text}</p>
							</section>
						{:else if cell.kind === 'assistant_message'}
							<section
								data-cell-id={cell.id}
								class="max-w-full overflow-hidden rounded-[30px] border border-stone-900/8 bg-white px-5 py-5 shadow-[0_18px_40px_rgba(37,34,28,0.08)] sm:max-w-4xl"
							>
								<p
									class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] text-stone-500 uppercase"
								>
									EveryCode · {cell.timestamp}
								</p>
								<h3 class={assistantTitleClass(cell)}>
									{cell.title}
								</h3>
								<div
									class="mt-4 space-y-4 text-[15px] leading-7 font-[var(--font-ui)] text-stone-700"
								>
									{#each cell.blocks as block}
										{#if block.type === 'paragraph'}
											<p>{block.text}</p>
										{:else if block.type === 'bullets'}
											<ul class="space-y-2 pl-5">
												{#each block.items as item}
													<li class="list-disc">{item}</li>
												{/each}
											</ul>
										{:else}
											<pre
												class="max-w-full overflow-x-auto rounded-[20px] bg-stone-950 px-4 py-4 text-[13px] leading-6 text-stone-100"><code class="block min-w-full"
													>{block.text}</code
												></pre>
										{/if}
									{/each}
								</div>
							</section>
						{:else if cell.kind === 'thinking'}
							<section
								data-cell-id={cell.id}
								class={`max-w-full overflow-hidden rounded-[26px] border px-4 py-4 transition sm:max-w-3xl ${cell.state === 'active' ? 'border-teal-700/20 bg-teal-950 text-teal-50 shadow-[0_18px_30px_rgba(7,50,48,0.18)]' : 'border-stone-900/8 bg-[#f4efe7] text-stone-800'}`}
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
									<div class="mt-4 flex flex-wrap gap-2 text-xs font-[var(--font-ui)]">
										{#if cell.state !== 'active'}
											<button
												type="button"
												onclick={() => toggleThinkingExpanded(cell.id)}
												class="rounded-full bg-stone-950 px-3 py-1.5 tracking-[0.18em] text-stone-50 uppercase"
											>
												Collapse reasoning
											</button>
										{/if}
										<button
											type="button"
											onclick={() => toggleThinkingPinned(cell.id)}
											class="rounded-full border border-stone-900/12 px-3 py-1.5 tracking-[0.18em] text-current uppercase"
										>
											{cell.pinned ? 'Unpin reasoning' : 'Pin reasoning'}
										</button>
									</div>
								{:else}
									<div
										class="mt-3 flex items-center justify-between gap-4 rounded-[18px] bg-white/60 px-4 py-3 text-sm font-[var(--font-ui)] text-stone-700"
									>
										<p>{cell.summary}</p>
										<div class="flex flex-wrap gap-2 text-[11px] tracking-[0.18em] uppercase">
											<button
												type="button"
												onclick={() => toggleThinkingExpanded(cell.id)}
												class="rounded-full bg-stone-950 px-3 py-1.5 text-stone-50"
												>Expand reasoning</button
											>
											<button
												type="button"
												onclick={() => toggleThinkingPinned(cell.id)}
												class="rounded-full border border-stone-900/12 px-3 py-1.5"
												>Pin reasoning</button
											>
										</div>
									</div>
								{/if}
							</section>
						{:else if cell.kind === 'tool_call'}
							<section
								data-cell-id={cell.id}
								class={`max-w-full overflow-hidden rounded-[28px] border px-4 py-4 sm:max-w-4xl ${cell.state === 'running' ? 'border-sky-900/20 bg-sky-950 text-sky-50 shadow-[0_16px_30px_rgba(13,71,88,0.18)]' : cell.state === 'failed' ? 'border-rose-900/20 bg-rose-950 text-rose-50' : 'border-stone-900/8 bg-[#f8f3ec] text-stone-900'}`}
							>
								<div class="flex flex-wrap items-start justify-between gap-3">
									<div>
										<p
											class={`text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] uppercase ${cell.state === 'running' ? 'text-sky-200' : 'text-stone-500'}`}
										>
											{cell.label}
										</p>
										<p
											class="mt-2 break-all text-[1.3rem] leading-tight font-[var(--font-display)] tracking-[-0.03em] sm:text-[1.45rem]"
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
										{#if cell.pinned}
											<span
												class="rounded-full bg-stone-950/10 px-2 py-0.5 text-[11px] tracking-[0.18em] uppercase"
												>Pinned</span
											>
										{/if}
									</div>
								</div>

								{#if shouldExpandTool(cell)}
									<div
										class="mt-4 overflow-hidden rounded-[22px] bg-black/80 shadow-inner shadow-black/30"
									>
										<pre
											class={`max-w-full overflow-x-auto overflow-y-auto px-4 py-4 text-[13px] leading-6 font-[var(--font-code)] ${cell.state === 'running' ? 'max-h-[260px] text-sky-50' : 'max-h-[240px] text-stone-100'}`}><code class="block min-w-full"
												>{cell.output.join('\n')}</code
											></pre>
									</div>
									<div
										class="mt-4 flex flex-wrap gap-2 text-xs font-[var(--font-ui)] tracking-[0.18em] uppercase"
									>
										{#if cell.state !== 'running'}
											<button
												type="button"
												onclick={() => toggleToolExpanded(cell.id)}
												class="rounded-full bg-stone-950 px-3 py-1.5 text-stone-50"
												>Collapse output</button
											>
										{/if}
										<button
											type="button"
											onclick={() => toggleToolPinned(cell.id)}
											class="rounded-full border border-stone-900/12 px-3 py-1.5 text-current"
										>
											{cell.pinned ? 'Unpin tool' : 'Pin tool'}
										</button>
									</div>
								{:else}
									<div
										class="mt-4 flex flex-wrap items-center justify-between gap-3 rounded-[20px] bg-white/70 px-4 py-3 text-sm font-[var(--font-ui)] text-stone-700"
									>
										<p class="max-w-2xl leading-6">{cell.preview}</p>
										<div class="flex flex-wrap gap-2 text-[11px] tracking-[0.18em] uppercase">
											<button
												type="button"
												onclick={() => toggleToolExpanded(cell.id)}
												class="rounded-full bg-stone-950 px-3 py-1.5 text-stone-50"
												>Expand output</button
											>
											<button
												type="button"
												onclick={() => toggleToolPinned(cell.id)}
												class="rounded-full border border-stone-900/12 px-3 py-1.5">Pin tool</button
											>
										</div>
									</div>
								{/if}
							</section>
						{:else if cell.kind === 'diff'}
							<section
								data-cell-id={cell.id}
								class={`max-w-full overflow-hidden rounded-[28px] border border-emerald-900/12 bg-[#eef4ef] px-4 py-4 text-stone-900 shadow-[0_16px_30px_rgba(43,84,67,0.08)] sm:max-w-4xl ${isFullDiff(cell) ? 'ring-2 ring-emerald-300/45' : ''}`}
							>
								<div class="flex flex-wrap items-start justify-between gap-3">
									<div>
										<p
											class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] text-emerald-800/70 uppercase"
										>
											Diff
										</p>
										<h3
											class="mt-2 text-[1.55rem] leading-tight font-[var(--font-display)] tracking-[-0.03em] text-stone-950"
										>
											{cell.summary}
										</h3>
										<p class="mt-2 text-sm font-[var(--font-ui)] text-stone-700">
											{cell.stats} · {cell.previewFile}
										</p>
									</div>
									<div
										class="flex flex-wrap gap-2 text-[11px] font-[var(--font-ui)] tracking-[0.18em] uppercase"
									>
										<button
											type="button"
											onclick={() => cycleDiffView(cell.id)}
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
									<pre
										class={`max-w-full overflow-x-auto px-4 py-4 text-[13px] leading-6 font-[var(--font-code)] text-emerald-50 ${isCompactDiff(cell) ? 'max-h-[220px]' : isFullDiff(cell) ? 'max-h-[560px]' : 'max-h-[340px]'}`}><code class="block min-w-full"
											>{cell.patch}</code
										></pre>
								</div>
							</section>
						{:else if cell.kind === 'approval'}
							<section
								data-cell-id={cell.id}
								class="max-w-full overflow-hidden rounded-[28px] border border-amber-900/18 bg-[#22140c] px-4 py-4 text-amber-50 shadow-[0_18px_30px_rgba(34,20,12,0.25)] sm:max-w-3xl"
							>
								<p
									class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] text-amber-200/76 uppercase"
								>
									{cell.status === 'pending' ? 'Approval needed' : 'Approval answered'} · {cell.timestamp}
								</p>
								<h3
									class="mt-3 text-[1.7rem] leading-tight font-[var(--font-display)] tracking-[-0.03em]"
								>
									{cell.label}
								</h3>
								<p class="mt-3 text-sm leading-6 font-[var(--font-ui)] text-amber-50/86">
									{cell.detail}
								</p>
								{#if cell.status === 'pending'}
									<div class="mt-4 flex flex-wrap gap-3 text-sm font-[var(--font-ui)]">
										<button
											type="button"
											onclick={() => answerApproval(cell, cell.primary)}
											disabled={continuityMode === 'view-only'}
											class="rounded-2xl bg-amber-300 px-4 py-2 text-amber-950 transition enabled:hover:bg-amber-200 disabled:cursor-not-allowed disabled:opacity-45"
										>
											{cell.primary}
										</button>
										<button
											type="button"
											onclick={() => answerApproval(cell, cell.secondary)}
											disabled={continuityMode === 'view-only'}
											class="rounded-2xl border border-amber-200/20 bg-transparent px-4 py-2 text-amber-50 transition enabled:hover:bg-amber-200/8 disabled:cursor-not-allowed disabled:opacity-45"
										>
											{cell.secondary}
										</button>
									</div>
									<p
										class="mt-3 text-xs font-[var(--font-ui)] tracking-[0.18em] text-amber-100/65 uppercase"
									>
										{continuityMode === 'view-only'
											? 'Switch to respond remotely or take over to answer.'
											: 'This action can be answered remotely without taking over the shell.'}
									</p>
								{:else}
									<p class="mt-4 text-sm font-[var(--font-ui)] text-amber-50/86">
										Answered by <span class="font-semibold text-amber-100"
											>{cell.respondedBy ?? cell.device ?? 'This browser'}</span
										>: {cell.resolution}
									</p>
								{/if}
							</section>
						{:else if cell.kind === 'question'}
							<section
								data-cell-id={cell.id}
								class="max-w-full overflow-hidden rounded-[28px] border border-stone-900/8 bg-white px-4 py-4 shadow-[0_16px_30px_rgba(41,34,24,0.08)] sm:max-w-3xl"
							>
								<p
									class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] text-stone-500 uppercase"
								>
									Question · {cell.timestamp}
								</p>
								<h3
									class="mt-3 text-[1.65rem] leading-tight font-[var(--font-display)] tracking-[-0.03em] text-stone-950"
								>
									{cell.prompt}
								</h3>
								<div class="mt-4 flex flex-wrap gap-2">
									{#each cell.options as option}
										<button
											type="button"
											onclick={() => answerQuestion(cell, option.label)}
											disabled={continuityMode === 'view-only' || cell.status === 'answered'}
											class={`rounded-full px-3 py-2 text-xs font-[var(--font-ui)] ${cell.answer === option.label ? 'bg-stone-950 text-stone-50' : 'bg-stone-900/6 text-stone-700'} disabled:cursor-not-allowed disabled:opacity-70`}
										>
											{option.label}
										</button>
									{/each}
								</div>
								{#if cell.answer}
									<p class="mt-4 text-sm font-[var(--font-ui)] text-stone-600">
										Answered by <span class="font-semibold text-stone-900"
											>{cell.respondedBy ?? 'This browser'}</span
										>: {cell.answer}
									</p>
								{/if}
							</section>
						{:else if cell.kind === 'error'}
							<section
								data-cell-id={cell.id}
								class="max-w-full overflow-hidden rounded-[28px] border border-rose-900/14 bg-[#fff1ef] px-4 py-4 text-stone-900 shadow-[0_16px_30px_rgba(102,41,41,0.07)] sm:max-w-3xl"
							>
								<p
									class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] text-rose-700/70 uppercase"
								>
									{cell.status} · {cell.timestamp}
								</p>
								<h3
									class="mt-3 text-[1.55rem] leading-tight font-[var(--font-display)] tracking-[-0.03em] text-stone-950"
								>
									{cell.message}
								</h3>
								<p class="mt-3 text-sm leading-6 font-[var(--font-ui)] text-stone-700">
									{cell.detail}
								</p>
							</section>
						{/if}
					{/each}
				</div>

				<div class="border-t border-stone-900/8 px-4 py-4 md:px-7 md:py-5">
					<div
						class="rounded-[28px] border border-stone-900/8 bg-stone-950 px-4 py-4 text-stone-50 shadow-[0_14px_30px_rgba(0,0,0,0.18)]"
					>
						<div class="flex flex-wrap items-start justify-between gap-3">
							<div>
								<p
									class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] text-stone-400 uppercase"
								>
									Composer
								</p>
								<p class="mt-2 max-w-2xl text-sm leading-6 font-[var(--font-ui)] text-stone-300">
									{composerHint()}
								</p>
							</div>
							{#if takeoverQueued}
								<span
									class="rounded-full bg-amber-300/16 px-3 py-1.5 text-xs font-[var(--font-ui)] tracking-[0.18em] text-amber-100 uppercase ring-1 ring-amber-300/25"
									>Takeover queued</span
								>
							{/if}
						</div>
						<div class="mt-3 grid gap-3 lg:grid-cols-[1fr_auto]">
							<textarea
								bind:value={composerText}
								disabled={composerDisabled()}
								class="min-h-[120px] rounded-[22px] border border-white/10 bg-white/6 px-4 py-4 text-[15px] leading-7 font-[var(--font-ui)] text-stone-50 placeholder:text-stone-500 focus:border-amber-300/30 focus:ring-0 focus:outline-none disabled:cursor-not-allowed disabled:opacity-45"
								placeholder="Continue here to take over from the Mac, or stay in view-only / respond-remotely mode while the current turn runs."
							></textarea>
							<div class="flex flex-col gap-2">
								<button
									type="button"
									onclick={handleComposerPrimary}
									class="rounded-[22px] bg-amber-300 px-5 py-3 text-sm font-[var(--font-ui)] font-semibold text-amber-950 transition hover:bg-amber-200"
								>
									{composerPrimaryLabel()}
								</button>
								<button
									type="button"
									onclick={() => setMode('view-only')}
									class="rounded-[22px] border border-white/12 bg-white/6 px-5 py-3 text-sm font-[var(--font-ui)] text-stone-50 transition hover:bg-white/10"
								>
									Stay read only
								</button>
							</div>
						</div>
					</div>
				</div>
			</main>
		</div>
	</div>
</div>

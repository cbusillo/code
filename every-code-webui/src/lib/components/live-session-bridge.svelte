<script lang="ts">
	import { onDestroy } from 'svelte';

	import TranscriptThreadView from '$lib/components/transcript-thread-view.svelte';
	import { protocolThreadToTranscriptCells } from '$lib/protocol-transcript';
	import { SharedSessionDomain, type SharedSessionEvent } from '$lib/shared-session-domain';
	import type { ProtocolThread } from '$lib/shared-session-types';

	const domain = new SharedSessionDomain();
	const bridgeState = domain.store;

	let wsUrl = $state(domain.getSnapshot().wsUrl);
	let startCwd = $state(domain.getSnapshot().startCwd);
	let startPrompt = $state(domain.getSnapshot().startPrompt);
	let composerText = $state(domain.getSnapshot().composerText);

	const unsubscribe = bridgeState.subscribe((state) => {
		wsUrl = state.wsUrl;
		startCwd = state.startCwd;
		startPrompt = state.startPrompt;
		composerText = state.composerText;
	});

	onDestroy(() => {
		unsubscribe();
		domain.disconnect('Bridge panel closed.');
	});

	const selectedThread = $derived.by(() => {
		if (
			$bridgeState.hydratedThread &&
			$bridgeState.hydratedThread.id === $bridgeState.selectedThreadId
		) {
			return $bridgeState.hydratedThread;
		}

		return (
			$bridgeState.threads.find((thread) => thread.id === $bridgeState.selectedThreadId) ?? null
		);
	});

	const phaseTone = $derived.by(() => {
		switch ($bridgeState.connectionPhase) {
			case 'connected':
				return 'border-emerald-300/30 bg-emerald-300/14 text-emerald-50';
			case 'connecting':
				return 'border-sky-300/30 bg-sky-300/14 text-sky-50';
			case 'error':
				return 'border-rose-300/30 bg-rose-300/14 text-rose-50';
			default:
				return 'border-white/12 bg-white/8 text-stone-100';
		}
	});

	function syncDrafts() {
		domain.setWsUrl(wsUrl);
		domain.setStartCwd(startCwd);
		domain.setStartPrompt(startPrompt);
		domain.setComposerText(composerText);
	}

	async function connect() {
		syncDrafts();
		await domain.connect();
	}

	async function refresh() {
		await domain.refreshThreads();
	}

	async function hydrateSelectedThread() {
		await domain.hydrateSelectedThread();
	}

	async function resumeSelectedThread() {
		await domain.resumeSelectedThread();
	}

	async function startThread() {
		syncDrafts();
		await domain.startThread();
	}

	async function sendPrompt() {
		syncDrafts();
		await domain.sendPrompt();
	}

	function selectThread(threadId: string) {
		domain.selectThread(threadId);
	}

	function formatUpdatedAt(timestamp: number) {
		return new Intl.DateTimeFormat('en-US', {
			month: 'short',
			day: 'numeric',
			hour: 'numeric',
			minute: '2-digit'
		}).format(timestamp * 1000);
	}

	function formatEventTime(event: SharedSessionEvent) {
		return new Intl.DateTimeFormat('en-US', {
			hour: 'numeric',
			minute: '2-digit',
			second: '2-digit'
		}).format(event.at);
	}

	const transcriptCells = $derived.by(() =>
		selectedThread ? protocolThreadToTranscriptCells(selectedThread) : []
	);
	const transcriptBadges = $derived.by(() => {
		if (!selectedThread) {
			return [];
		}

		return [
			selectedThread.cwd,
			`${selectedThread.turns.length} turn${selectedThread.turns.length === 1 ? '' : 's'}`,
			selectedThread.gitInfo?.branch ? `branch ${selectedThread.gitInfo.branch}` : 'no git info',
			`${selectedThread.modelProvider} · ${selectedThread.cliVersion}`
		];
	});

	function threadTone(thread: ProtocolThread) {
		if (thread.id === $bridgeState.activeThreadId) {
			return 'border-amber-300/35 bg-amber-100/10 shadow-[0_20px_50px_rgba(0,0,0,0.18)]';
		}

		if (thread.id === $bridgeState.selectedThreadId) {
			return 'border-sky-300/30 bg-sky-100/10 shadow-[0_18px_44px_rgba(0,0,0,0.14)]';
		}

		return 'border-white/8 bg-black/16 hover:bg-white/6';
	}

	function sourceBadge(thread: ProtocolThread) {
		if (typeof thread.source === 'string') {
			return thread.source;
		}

		return thread.source.subAgent;
	}
</script>

<section
	class="rounded-[30px] border border-white/10 bg-[linear-gradient(145deg,_rgba(255,255,255,0.08),_rgba(255,255,255,0.03))] p-4 shadow-[0_24px_90px_rgba(0,0,0,0.3)] backdrop-blur lg:p-6"
>
	<div class="flex flex-col gap-5">
		<div class="flex flex-col gap-4 xl:flex-row xl:items-end xl:justify-between">
			<div class="space-y-3">
				<p
					class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.24em] text-teal-200/80 uppercase"
				>
					Protocol bridge
				</p>
				<div class="space-y-2">
					<h2
						class="text-3xl font-[var(--font-display)] tracking-[-0.04em] text-stone-50 md:text-[2.75rem]"
					>
						Live threads, hydrated from the real app-server.
					</h2>
					<p class="max-w-3xl text-sm leading-7 font-[var(--font-ui)] text-stone-300">
						This panel keeps the transcript lab honest. It connects to the current `thread/*` and
						`turn/start` contract, hydrates stored turns, and tails live updates over
						`addConversationListener`.
					</p>
				</div>
			</div>

			<div
				class={`inline-flex items-center gap-3 self-start rounded-full border px-4 py-2 text-sm font-[var(--font-ui)] ${phaseTone}`}
			>
				<span class="h-2.5 w-2.5 rounded-full bg-current"></span>
				<span class="capitalize">{$bridgeState.connectionPhase}</span>
				{#if $bridgeState.pendingAction}
					<span class="text-xs tracking-[0.18em] text-current/70 uppercase"
						>{$bridgeState.pendingAction}</span
					>
				{/if}
			</div>
		</div>

		<div class="grid gap-4 xl:grid-cols-[minmax(0,1.25fr)_minmax(0,1.1fr)_minmax(0,0.95fr)]">
			<section
				class="rounded-[26px] border border-white/10 bg-black/18 p-4 shadow-inner shadow-black/25"
			>
				<div class="space-y-4">
					<label class="grid gap-2 text-sm font-[var(--font-ui)] text-stone-200">
						<span class="text-[0.68rem] tracking-[0.2em] text-stone-400 uppercase"
							>WebSocket URL</span
						>
						<input
							type="text"
							bind:value={wsUrl}
							oninput={() => domain.setWsUrl(wsUrl)}
							placeholder="ws://127.0.0.1:8877"
							class="rounded-2xl border border-white/12 bg-white/8 px-4 py-3 text-stone-100 placeholder:text-stone-500 focus:border-teal-300/40 focus:ring-0"
						/>
					</label>

					<div class="flex flex-wrap gap-2 text-sm font-[var(--font-ui)]">
						<button
							type="button"
							onclick={connect}
							class="rounded-full bg-teal-200 px-4 py-2 text-stone-950 transition hover:bg-teal-100"
						>
							Connect
						</button>
						<button
							type="button"
							onclick={() => domain.disconnect('Disconnected.')}
							class="rounded-full border border-white/12 bg-white/8 px-4 py-2 text-stone-100 transition hover:bg-white/12"
						>
							Disconnect
						</button>
						<button
							type="button"
							onclick={refresh}
							class="rounded-full border border-white/12 bg-white/8 px-4 py-2 text-stone-100 transition hover:bg-white/12"
						>
							Refresh
						</button>
					</div>

					<p
						class="rounded-[20px] border border-white/8 bg-white/6 px-4 py-3 text-sm leading-6 font-[var(--font-ui)] text-stone-300"
					>
						{$bridgeState.statusMessage}
					</p>

					<div class="grid gap-3 sm:grid-cols-2">
						<label class="grid gap-2 text-sm font-[var(--font-ui)] text-stone-200">
							<span class="text-[0.68rem] tracking-[0.2em] text-stone-400 uppercase">Start cwd</span
							>
							<input
								type="text"
								bind:value={startCwd}
								oninput={() => domain.setStartCwd(startCwd)}
								placeholder="/Users/cbusillo/Developer/code"
								class="rounded-2xl border border-white/12 bg-white/8 px-4 py-3 text-stone-100 placeholder:text-stone-500 focus:border-teal-300/40 focus:ring-0"
							/>
						</label>
						<label class="grid gap-2 text-sm font-[var(--font-ui)] text-stone-200 sm:col-span-2">
							<span class="text-[0.68rem] tracking-[0.2em] text-stone-400 uppercase"
								>Optional first prompt</span
							>
							<textarea
								rows="3"
								bind:value={startPrompt}
								oninput={() => domain.setStartPrompt(startPrompt)}
								placeholder="Start a fresh continuity session from the browser..."
								class="rounded-[24px] border border-white/12 bg-white/8 px-4 py-3 text-stone-100 placeholder:text-stone-500 focus:border-teal-300/40 focus:ring-0"
							></textarea>
						</label>
					</div>

					<button
						type="button"
						onclick={startThread}
						class="w-full rounded-[22px] bg-amber-200 px-4 py-3 text-sm font-[var(--font-ui)] font-medium text-stone-950 transition hover:bg-amber-100"
					>
						Start new thread
					</button>
				</div>
			</section>

			<section
				class="rounded-[26px] border border-white/10 bg-black/18 p-4 shadow-inner shadow-black/25"
			>
				<div class="mb-4 flex items-center justify-between gap-3">
					<div>
						<p
							class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.2em] text-stone-400 uppercase"
						>
							Session inventory
						</p>
						<h3 class="mt-2 text-3xl font-[var(--font-display)] text-stone-50">
							{$bridgeState.threads.length} live choices
						</h3>
					</div>
					{#if selectedThread}
						<span
							class="rounded-full border border-white/12 bg-white/8 px-3 py-1.5 text-xs font-[var(--font-ui)] text-stone-200"
						>
							Selected
						</span>
					{/if}
				</div>

				<div class="max-h-[29rem] space-y-3 overflow-y-auto pr-1">
					{#if !$bridgeState.threads.length}
						<div
							class="rounded-[22px] border border-dashed border-white/10 bg-white/4 px-4 py-5 text-sm leading-6 font-[var(--font-ui)] text-stone-400"
						>
							Connect and refresh to pull persisted sessions from the app-server.
						</div>
					{:else}
						{#each $bridgeState.threads as thread}
							<button
								type="button"
								onclick={() => selectThread(thread.id)}
								class={`w-full rounded-[22px] border px-4 py-4 text-left transition ${threadTone(thread)}`}
							>
								<div class="mb-3 flex items-center justify-between gap-3">
									<span
										class="rounded-full border border-white/12 bg-white/8 px-2.5 py-1 text-[11px] font-[var(--font-ui)] tracking-[0.18em] text-stone-200 uppercase"
									>
										{sourceBadge(thread)}
									</span>
									<span class="text-[11px] font-[var(--font-ui)] text-stone-400"
										>{formatUpdatedAt(thread.updatedAt)}</span
									>
								</div>
								<h4 class="text-[1.3rem] leading-tight font-[var(--font-display)] text-stone-50">
									{thread.preview || thread.id}
								</h4>
								<p
									class="mt-3 text-xs font-[var(--font-ui)] tracking-[0.16em] text-stone-500 uppercase"
								>
									{thread.cwd}
								</p>
								<p class="mt-3 text-sm leading-6 font-[var(--font-ui)] text-stone-300">
									{thread.gitInfo?.branch
										? `${thread.gitInfo.branch} · `
										: ''}{thread.modelProvider} · {thread.cliVersion}
								</p>
							</button>
						{/each}
					{/if}
				</div>
			</section>

			<section
				class="rounded-[26px] border border-white/10 bg-black/18 p-4 shadow-inner shadow-black/25"
			>
				<div class="space-y-4">
					<div class="flex flex-wrap gap-2 text-sm font-[var(--font-ui)]">
						<button
							type="button"
							onclick={hydrateSelectedThread}
							class="rounded-full bg-sky-200 px-4 py-2 text-stone-950 transition hover:bg-sky-100"
						>
							Hydrate selected thread
						</button>
						<button
							type="button"
							onclick={resumeSelectedThread}
							class="rounded-full border border-white/12 bg-white/8 px-4 py-2 text-stone-100 transition hover:bg-white/12"
						>
							Resume for live updates
						</button>
					</div>

					{#if selectedThread}
						<div class="rounded-[22px] border border-white/10 bg-white/6 p-4">
							<p
								class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.18em] text-stone-400 uppercase"
							>
								Hydrated thread
							</p>
							<h3 class="mt-2 text-2xl font-[var(--font-display)] text-stone-50">
								{selectedThread.preview || selectedThread.id}
							</h3>
							<p class="mt-3 text-sm leading-6 font-[var(--font-ui)] text-stone-300">
								{selectedThread.cwd}
							</p>
							<p
								class="mt-2 text-xs font-[var(--font-ui)] tracking-[0.16em] text-stone-500 uppercase"
							>
								{selectedThread.turns.length} turn{selectedThread.turns.length === 1 ? '' : 's'} · {selectedThread.modelProvider}
							</p>
						</div>

						<div class="grid gap-3">
							<label class="grid gap-2 text-sm font-[var(--font-ui)] text-stone-200">
								<span class="text-[0.68rem] tracking-[0.2em] text-stone-400 uppercase"
									>Next prompt</span
								>
								<textarea
									rows="4"
									bind:value={composerText}
									oninput={() => domain.setComposerText(composerText)}
									placeholder="Send the next turn through the real app-server..."
									class="rounded-[24px] border border-white/12 bg-white/8 px-4 py-3 text-stone-100 placeholder:text-stone-500 focus:border-teal-300/40 focus:ring-0"
								></textarea>
							</label>
							<button
								type="button"
								onclick={sendPrompt}
								class="rounded-[22px] bg-stone-100 px-4 py-3 text-sm font-[var(--font-ui)] font-medium text-stone-950 transition hover:bg-white"
							>
								Send prompt
							</button>
						</div>

						<TranscriptThreadView
							title={selectedThread.preview || selectedThread.id}
							description="Hydrated turns now flow through the same transcript cell model as the golden lab, so live browser continuity uses the real protocol instead of raw item cards."
							badges={transcriptBadges}
							cells={transcriptCells}
							emptyMessage="This thread is hydrated, but there are no turn items yet. Start or resume a turn and the transcript will populate as protocol items arrive."
						/>
					{:else}
						<div
							class="rounded-[22px] border border-dashed border-white/10 bg-white/4 px-4 py-5 text-sm leading-6 font-[var(--font-ui)] text-stone-400"
						>
							Pick a thread, hydrate it, and this column becomes the read-only transcript bridge for
							the real app-server.
						</div>
					{/if}

					<div class="max-h-[15rem] space-y-2 overflow-y-auto pr-1">
						<p
							class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.18em] text-stone-400 uppercase"
						>
							Live event rail
						</p>
						{#if !$bridgeState.events.length}
							<div
								class="rounded-[20px] border border-dashed border-white/10 bg-white/4 px-4 py-4 text-sm font-[var(--font-ui)] text-stone-400"
							>
								Resume a thread and start a turn to watch event deltas land here.
							</div>
						{:else}
							{#each $bridgeState.events as event}
								<div
									class="rounded-[20px] border border-white/8 bg-white/5 px-4 py-3 text-sm font-[var(--font-ui)] text-stone-300"
								>
									<div
										class="mb-1 flex items-center justify-between gap-3 text-[11px] tracking-[0.16em] text-stone-500 uppercase"
									>
										<span>{event.method}</span>
										<span>{formatEventTime(event)}</span>
									</div>
									<p>{event.summary}</p>
								</div>
							{/each}
						{/if}
					</div>
				</div>
			</section>
		</div>
	</div>
</section>

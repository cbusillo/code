import { afterEach, describe, expect, it, vi } from 'vitest';

import { SharedSessionDomain, type SharedSessionTransport } from '$lib/shared-session-domain';
import type {
	ConversationListenerResult,
	JsonRpcNotification,
	JsonRpcRequestId,
	JsonRpcServerRequest,
	ProtocolThread,
	ProtocolTurn,
	ProtocolTurnStartParams,
	ThreadReadResult,
	ThreadResumeResult,
	ThreadStartResult,
	ThreadForkResult,
	TurnSteerResult,
	TurnStartResult
} from '$lib/shared-session-types';

describe('SharedSessionDomain', () => {
	afterEach(() => {
		vi.useRealTimers();
		vi.unstubAllGlobals();
	});

	it('connects, lists threads, and hydrates the selected transcript', async () => {
		const thread = makeThread('thread_alpha', []);
		const hydratedThread = makeThread('thread_alpha', [
			makeTurn('turn_one', 'completed', 'Hydrate the transcript')
		]);
		const client = new FakeSharedSessionClient({
			threads: [thread],
			threadReads: new Map([[thread.id, hydratedThread]])
		});
		const domain = new SharedSessionDomain(client);

		domain.setWsUrl('ws://127.0.0.1:8877');
		await domain.connect();
		await domain.hydrateSelectedThread();

		const snapshot = domain.getSnapshot();
		expect(snapshot.connectionPhase).toBe('connected');
		expect(snapshot.selectedThreadId).toBe(thread.id);
		expect(snapshot.threads).toHaveLength(1);
		expect(snapshot.hydratedThread?.turns).toHaveLength(1);
		expect(snapshot.hydratedThread?.turns[0]?.id).toBe('turn_one');
		expect(snapshot.statusMessage).toContain('Hydrated');
	});

	it('blocks insecure remote ws targets before opening a socket', async () => {
		const thread = makeThread('thread_alpha', []);
		const client = new FakeSharedSessionClient({ threads: [thread] });
		const domain = new SharedSessionDomain(client);

		domain.setWsUrl('ws://code.example.com:8877');
		await domain.connect();

		expect(client.connectCallCount).toBe(0);
		expect(domain.getSnapshot()).toMatchObject({
			connectionPhase: 'error'
		});
		expect(domain.getSnapshot().statusMessage).toContain('wss://');
	});

	it('requires takeover before sending into a thread loaded elsewhere', async () => {
		const thread = makeThread('thread_loaded_elsewhere', []);
		const client = new FakeSharedSessionClient({
			threads: [thread],
			threadReads: new Map([[thread.id, thread]]),
			loadedThreadIds: [thread.id]
		});
		const domain = new SharedSessionDomain(client);

		domain.setWsUrl('ws://127.0.0.1:8877');
		await domain.connect();
		domain.selectThread(thread.id);
		await domain.hydrateSelectedThread();
		domain.setComposerText('Send this only after takeover');
		await domain.sendPrompt();

		expect(client.startedPrompts).toEqual([]);
		expect(domain.getSnapshot().statusMessage).toContain('Take over here');
	});

	it('resumes a thread, sends prompts, and merges live notifications', async () => {
		const thread = makeThread('thread_beta', []);
		const resumedThread = makeThread('thread_beta', [
			makeTurn('turn_live', 'inProgress', 'Keep following from the browser')
		]);
		const completedTurn = makeTurn('turn_live', 'completed', 'Keep following from the browser');
		const client = new FakeSharedSessionClient({
			threads: [thread],
			threadReads: new Map([[thread.id, thread]]),
			threadResumes: new Map([[thread.id, resumedThread]]),
			turnStarts: [completedTurn]
		});
		const domain = new SharedSessionDomain(client);

		domain.setWsUrl('ws://127.0.0.1:8877');
		await domain.connect();
		domain.selectThread(thread.id);
		await domain.hydrateSelectedThread();
		domain.setComposerText('Keep following from the browser');
		await domain.sendPrompt();

		client.emit({
			jsonrpc: '2.0',
			method: 'turn/completed',
			params: {
				threadId: thread.id,
				turn: completedTurn
			}
		});

		const snapshot = domain.getSnapshot();
		expect(snapshot.activeThreadId).toBeNull();
		expect(client.subscriptionThreadIds).toEqual([thread.id]);
		expect(client.startedPrompts).toEqual([
			{ threadId: thread.id, text: 'Keep following from the browser', overrides: undefined }
		]);
		expect(snapshot.hydratedThread?.turns).toHaveLength(1);
		expect(snapshot.hydratedThread?.turns[0]?.status).toBe('completed');
		expect(snapshot.events[0]?.method).toBe('turn/completed');
	});

	it('seeds an optimistic user message when turn/start returns no items yet', async () => {
		const thread = makeThread('thread_seeded', []);
		const client = new FakeSharedSessionClient({
			threads: [thread],
			threadReads: new Map([[thread.id, thread]]),
			threadResumes: new Map([[thread.id, thread]]),
			turnStarts: [
				{
					id: 'turn_seeded',
					status: 'inProgress',
					items: [],
					error: null
				}
			]
		});
		const domain = new SharedSessionDomain(client);

		domain.setWsUrl('ws://127.0.0.1:8877');
		await domain.connect();
		domain.selectThread(thread.id);
		await domain.hydrateSelectedThread();
		domain.setComposerText('Keep continuity even before stream inserts land');
		await domain.sendPrompt();

		expect(domain.getSnapshot().hydratedThread?.turns[0]?.items[0]).toMatchObject({
			type: 'userMessage',
			content: [{ type: 'text', text: 'Keep continuity even before stream inserts land' }]
		});
	});

	it('sends persisted turn steering overrides with the next prompt', async () => {
		const thread = makeThread('thread_overrides', []);
		const client = new FakeSharedSessionClient({
			threads: [thread],
			threadReads: new Map([[thread.id, thread]]),
			threadResumes: new Map([[thread.id, thread]]),
			turnStarts: [makeTurn('turn_override', 'inProgress', 'Route this through a different repo root')]
		});
		const domain = new SharedSessionDomain(client);

		domain.setWsUrl('ws://127.0.0.1:8877');
		await domain.connect();
		domain.selectThread(thread.id);
		await domain.hydrateSelectedThread();
		domain.setTurnOverrideModel('gpt-5.1-codex');
		domain.setTurnOverrideCwd('/Users/cbusillo/Developer/code/code-rs');
		domain.setTurnOverridePersonality('friendly');
		domain.setComposerText('Route this through a different repo root');
		await domain.sendPrompt();

		expect(client.startedPrompts).toEqual([
			{
				threadId: thread.id,
				text: 'Route this through a different repo root',
				overrides: {
					model: 'gpt-5.1-codex',
					cwd: '/Users/cbusillo/Developer/code/code-rs',
					personality: 'friendly'
				}
			}
		]);
		expect(domain.getSnapshot()).toMatchObject({
			turnOverrideModel: 'gpt-5.1-codex',
			turnOverrideCwd: '/Users/cbusillo/Developer/code/code-rs',
			turnOverridePersonality: 'friendly',
			composerText: ''
		});
	});

	it('steers an active turn without sending a fresh prompt', async () => {
		const runningThread = makeThread('thread_steer', [
			makeTurn('turn_live', 'inProgress', 'Still running here')
		]);
		const client = new FakeSharedSessionClient({
			threads: [runningThread],
			threadReads: new Map([[runningThread.id, runningThread]])
		});
		const domain = new SharedSessionDomain(client);

		domain.setWsUrl('ws://127.0.0.1:8877');
		await domain.connect();
		domain.selectThread(runningThread.id);
		await domain.hydrateSelectedThread();
		await domain.steerSelectedTurn('Prefer the review summary before any more tool calls.');

		expect(client.steeredTurns).toEqual([
			{
				threadId: runningThread.id,
				turnId: 'turn_live',
				text: 'Prefer the review summary before any more tool calls.'
			}
		]);
		expect(client.startedPrompts).toEqual([]);
	});

	it('forks the selected thread into a new hydrated branch', async () => {
		const sourceThread = makeThread('thread_source', [
			makeTurn('turn_done', 'completed', 'Checkpoint before branching')
		]);
		const forkedThread = makeThread('thread_branch', sourceThread.turns);
		const client = new FakeSharedSessionClient({
			threads: [sourceThread],
			threadReads: new Map([[sourceThread.id, sourceThread]]),
			forkedThreads: new Map([[sourceThread.id, forkedThread]])
		});
		const domain = new SharedSessionDomain(client);

		domain.setWsUrl('ws://127.0.0.1:8877');
		await domain.connect();
		domain.selectThread(sourceThread.id);
		await domain.hydrateSelectedThread();
		await domain.forkSelectedThread();

		expect(domain.getSnapshot()).toMatchObject({
			selectedThreadId: forkedThread.id,
			activeThreadId: forkedThread.id
		});
		expect(client.subscriptionThreadIds.at(-1)).toBe(forkedThread.id);
	});

	it('does not send a prompt into a thread still running elsewhere', async () => {
		const runningThread = makeThread('thread_running_elsewhere', [
			makeTurn('turn_live', 'inProgress', 'Still running on another surface')
		]);
		const client = new FakeSharedSessionClient({
			threads: [runningThread],
			threadReads: new Map([[runningThread.id, runningThread]])
		});
		const domain = new SharedSessionDomain(client);

		domain.setWsUrl('ws://127.0.0.1:8877');
		await domain.connect();
		domain.selectThread(runningThread.id);
		await domain.hydrateSelectedThread();
		domain.setComposerText('Take over while another surface is still running');
		await domain.sendPrompt();

		expect(client.startedPrompts).toEqual([]);
		expect(domain.getSnapshot()).toMatchObject({
			connectionPhase: 'connected',
			statusMessage:
				'This thread is still running elsewhere. Wait for it to settle or take over here before sending a new prompt.'
		});
	});

	it('queues a takeover request until a remote active turn settles', async () => {
		const runningThread = makeThread('thread_takeover_queue', [
			makeTurn('turn_live', 'inProgress', 'Still running elsewhere')
		]);
		const settledThread = makeThread('thread_takeover_queue', [
			makeTurn('turn_live', 'completed', 'Still running elsewhere')
		]);
		const client = new FakeSharedSessionClient({
			threads: [runningThread],
			threadReads: new Map([[runningThread.id, runningThread]]),
			threadResumes: new Map([[runningThread.id, settledThread]])
		});
		const domain = new SharedSessionDomain(client);

		domain.setWsUrl('ws://127.0.0.1:8877');
		await domain.connect();
		domain.selectThread(runningThread.id);
		await domain.hydrateSelectedThread();
		await domain.takeOverSelectedThread();

		expect(domain.getSnapshot()).toMatchObject({
			connectionPhase: 'connected',
			takeoverRequestedThreadId: runningThread.id,
			statusMessage:
				'Takeover queued for thread_takeover_queue. Live updates will attach here when the current turn settles.'
		});
		expect(client.resumeThreadIds).toEqual([]);

		client.emit({
			jsonrpc: '2.0',
			method: 'turn/completed',
			params: {
				threadId: runningThread.id,
				turn: settledTurn('turn_live', 'Still running elsewhere')
			}
		});
		await Promise.resolve();
		await Promise.resolve();
		await new Promise((resolve) => setTimeout(resolve, 0));

		expect(client.resumeThreadIds).toEqual([runningThread.id]);
		expect(domain.getSnapshot()).toMatchObject({
			activeThreadId: runningThread.id,
			takeoverRequestedThreadId: null,
			statusMessage: `Live updates attached to ${runningThread.id}.`
		});
	});

	it('captures structured requests and responds to approvals and grouped questions', async () => {
		const thread = makeThread('thread_actions', []);
		const client = new FakeSharedSessionClient({
			threads: [thread],
			threadReads: new Map([[thread.id, thread]])
		});
		const domain = new SharedSessionDomain(client);

		domain.setWsUrl('ws://127.0.0.1:8877');
		await domain.connect();
		domain.selectThread(thread.id);
		await domain.hydrateSelectedThread();

		client.emitServerRequest({
			jsonrpc: '2.0',
			id: 9,
			method: 'item/commandExecution/requestApproval',
			params: {
				threadId: thread.id,
				turnId: 'turn_actions',
				itemId: 'item_exec',
				reason: 'Network access requested',
				command: 'curl https://example.com',
				cwd: '/Users/cbusillo/Developer/code',
				proposedExecpolicyAmendment: ['allow curl https://example.com']
			}
		});
		client.emitServerRequest({
			jsonrpc: '2.0',
			id: 10,
			method: 'item/fileChange/requestApproval',
			params: {
				threadId: thread.id,
				turnId: 'turn_actions',
				itemId: 'item_patch',
				reason: 'Allow patch writeback'
			}
		});
		client.emitServerRequest({
			jsonrpc: '2.0',
			id: 11,
			method: 'item/tool/requestUserInput',
			params: {
				threadId: thread.id,
				turnId: 'turn_actions',
				itemId: 'item_question',
				questions: [
					{
						id: 'shipping_mode',
						header: 'Shipping',
						question: 'Pick the shipping mode',
						isOther: false,
						isSecret: false,
						options: [
							{ label: 'Fast', description: 'Fastest option' },
							{ label: 'Cheap', description: 'Lowest cost' }
						]
					},
					{
						id: 'notes',
						header: 'Notes',
						question: 'Add deployment note',
						isOther: true,
						isSecret: false,
						options: null
					}
				]
			}
		});

		const snapshot = domain.getSnapshot();
		expect(snapshot.structuredRequests).toHaveLength(4);
		expect(
			snapshot.structuredRequests.find(
				(request) => request.kind === 'command-approval' && request.requestId === 9
			)
		).toMatchObject({
			options: expect.arrayContaining([
				expect.objectContaining({ id: 'acceptWithExecpolicyAmendment' })
			])
		});

		await domain.answerApproval(9, {
			acceptWithExecpolicyAmendment: {
				execpolicy_amendment: ['allow curl https://example.com']
			}
		});
		await domain.answerApproval(10, 'acceptForSession');
		await domain.answerQuestions(11, {
			shipping_mode: ['Fast'],
			notes: ['Ship after QA']
		});

		expect(client.responses).toEqual([
			{
				id: 9,
				result: {
					decision: {
						acceptWithExecpolicyAmendment: {
							execpolicy_amendment: ['allow curl https://example.com']
						}
					}
				}
			},
			{ id: 10, result: { decision: 'acceptForSession' } },
			{
				id: 11,
				result: {
					answers: {
						shipping_mode: { answers: ['Fast'] },
						notes: { answers: ['Ship after QA'] }
					}
				}
			}
		]);
		expect(domain.getSnapshot().structuredRequests).toEqual(
			expect.arrayContaining([
				expect.objectContaining({
					kind: 'command-approval',
					status: 'answered',
					resolution: 'Accepted with policy amendment'
				}),
				expect.objectContaining({
					kind: 'file-approval',
					status: 'answered',
					resolution: 'Accepted for session'
				}),
				expect.objectContaining({ kind: 'question', status: 'answered', answer: 'Fast' }),
				expect.objectContaining({ kind: 'question', status: 'answered', answer: 'Ship after QA' })
			])
		);
	});

	it('auto-hydrates and resumes a restored selected thread on connect', async () => {
		const storage = makeStorage();
		storage.setItem('every-code-webui.composer-text', 'Resume this browser draft');
		storage.setItem('every-code-webui.selected-thread-id', 'thread_saved');
		vi.stubGlobal('window', { localStorage: storage });

		const savedThread = makeThread('thread_saved', [
			makeTurn('turn_saved', 'completed', 'Previously restored thread')
		]);
		const fallbackThread = makeThread('thread_fallback', []);
		const client = new FakeSharedSessionClient({
			threads: [savedThread, fallbackThread],
			threadReads: new Map([[savedThread.id, savedThread]]),
			threadResumes: new Map([[savedThread.id, savedThread]])
		});
		const domain = new SharedSessionDomain(client);

		expect(domain.getSnapshot()).toMatchObject({
			composerText: 'Resume this browser draft',
			selectedThreadId: 'thread_saved'
		});

		domain.setComposerText('Resume this browser draft with edits');
		await domain.connect();

		expect(domain.getSnapshot()).toMatchObject({
			selectedThreadId: savedThread.id,
			activeThreadId: savedThread.id,
			hydratedThread: expect.objectContaining({ id: savedThread.id })
		});
		expect(client.readThreadIds).toEqual([savedThread.id]);
		expect(client.subscriptionThreadIds).toEqual([savedThread.id]);
		expect(storage.getItem('every-code-webui.composer-text')).toBe(
			'Resume this browser draft with edits'
		);
		expect(storage.getItem('every-code-webui.selected-thread-id')).toBe(savedThread.id);
	});

	it('replaces a missing persisted thread selection with the first live thread', async () => {
		const storage = makeStorage();
		storage.setItem('every-code-webui.selected-thread-id', 'thread_missing');
		vi.stubGlobal('window', { localStorage: storage });

		const firstThread = makeThread('thread_first', []);
		const secondThread = makeThread('thread_second', []);
		const client = new FakeSharedSessionClient({
			threads: [firstThread, secondThread]
		});
		const domain = new SharedSessionDomain(client);

		await domain.connect();

		expect(domain.getSnapshot().selectedThreadId).toBe(firstThread.id);
		expect(domain.getSnapshot().activeThreadId).toBeNull();
		expect(domain.getSnapshot().hydratedThread).toBeNull();
		expect(client.readThreadIds).toEqual([]);
		expect(client.subscriptionThreadIds).toEqual([]);
		expect(storage.getItem('every-code-webui.selected-thread-id')).toBe(firstThread.id);
	});

	it('restores and clears the persisted start prompt after starting a thread', async () => {
		const storage = makeStorage();
		storage.setItem('every-code-webui.start-prompt', 'Kick off from the browser after reload');
		vi.stubGlobal('window', { localStorage: storage });

		const client = new FakeSharedSessionClient({
			threads: [],
			turnStarts: [makeTurn('turn_started', 'inProgress', 'Kick off from the browser after reload')]
		});
		const domain = new SharedSessionDomain(client);

		expect(domain.getSnapshot().startPrompt).toBe('Kick off from the browser after reload');

		domain.setWsUrl('ws://127.0.0.1:8877');
		await domain.connect();
		await domain.startThread();

		expect(client.startedPrompts).toEqual([
			{
				threadId: 'thread_started',
				text: 'Kick off from the browser after reload',
				overrides: undefined
			}
		]);
		expect(domain.getSnapshot().startPrompt).toBe('');
		expect(storage.getItem('every-code-webui.start-prompt')).toBeNull();
	});

	it('auto-reconnects and restores the selected thread after an unexpected close', async () => {
		vi.useFakeTimers();

		const thread = makeThread('thread_reconnect', [
			makeTurn('turn_reconnect', 'completed', 'Reconnect this thread after a socket drop')
		]);
		const client = new FakeSharedSessionClient({
			threads: [thread],
			threadReads: new Map([[thread.id, thread]]),
			threadResumes: new Map([[thread.id, thread]]),
			failConnectCalls: [2]
		});
		const domain = new SharedSessionDomain(client);

		domain.setWsUrl('ws://127.0.0.1:8877');
		await domain.connect();
		domain.selectThread(thread.id);
		await domain.hydrateSelectedThread();
		await domain.resumeSelectedThread();

		client.simulateUnexpectedClose();
		expect(domain.getSnapshot()).toMatchObject({
			connectionPhase: 'connecting',
			statusMessage: 'Connection lost. Reconnecting in 1.5s (attempt 1 of 4)...',
			activeThreadId: null
		});

		await vi.advanceTimersByTimeAsync(1500);
		expect(client.connectCallCount).toBe(2);
		expect(domain.getSnapshot()).toMatchObject({
			connectionPhase: 'connecting',
			statusMessage: 'Connection lost. Reconnecting in 3s (attempt 2 of 4)...'
		});

		await vi.advanceTimersByTimeAsync(3000);
		expect(client.connectCallCount).toBe(3);
		expect(domain.getSnapshot()).toMatchObject({
			connectionPhase: 'connected',
			selectedThreadId: thread.id,
			activeThreadId: thread.id,
			hydratedThread: expect.objectContaining({ id: thread.id })
		});
	});

	it('quietly re-hydrates the selected thread from raw live event notifications', async () => {
		vi.useFakeTimers();

		const initialThread = makeThread('thread_live_refresh', [
			makeTurn('turn_live', 'inProgress', 'Keep the browser transcript fresh')
		]);
		const refreshedThread = makeThread('thread_live_refresh', [
			{
				...makeTurn('turn_live', 'inProgress', 'Keep the browser transcript fresh'),
				items: [
					{
						type: 'userMessage',
						id: 'turn_live_user',
						content: [{ type: 'text', text: 'Keep the browser transcript fresh', text_elements: [] }]
					},
					{
						type: 'agentMessage',
						id: 'turn_live_agent',
						text: 'Streaming output reached the browser after a raw live event.'
					}
				]
			}
		]);
		const client = new FakeSharedSessionClient({
			threads: [initialThread],
			threadReads: new Map([[initialThread.id, initialThread]])
		});
		const domain = new SharedSessionDomain(client);

		domain.setWsUrl('ws://127.0.0.1:8877');
		await domain.connect();
		domain.selectThread(initialThread.id);
		await domain.hydrateSelectedThread();
		client.threadReads.set(initialThread.id, refreshedThread);

		client.emit({
			jsonrpc: '2.0',
			method: 'codex/event/item_updated',
			params: {
				conversationId: initialThread.id,
				msg: 'item_updated'
			}
		});

		await vi.advanceTimersByTimeAsync(250);
		await Promise.resolve();

		expect(client.readThreadIds).toEqual([initialThread.id, initialThread.id]);
		expect(domain.getSnapshot().hydratedThread?.turns[0]?.items).toHaveLength(2);
		expect(domain.getSnapshot().pendingAction).toBeNull();
	});

	it('quietly refreshes threads and the selected transcript when returning to the foreground', async () => {
		const thread = makeThread('thread_foreground', [
			makeTurn('turn_foreground', 'completed', 'Refresh after mobile backgrounding')
		]);
		const refreshedThread = makeThread('thread_foreground', [
			makeTurn('turn_foreground', 'completed', 'Refresh after mobile backgrounding'),
			makeTurn('turn_follow_up', 'completed', 'Foreground sync pulled the latest state')
		]);
		const client = new FakeSharedSessionClient({
			threads: [thread],
			threadReads: new Map([[thread.id, thread]])
		});
		const domain = new SharedSessionDomain(client);

		domain.setWsUrl('ws://127.0.0.1:8877');
		await domain.connect();
		domain.selectThread(thread.id);
		await domain.hydrateSelectedThread();

		client.threads = [refreshedThread];
		client.threadReads.set(thread.id, refreshedThread);
		await domain.refreshForForeground();

		expect(client.readThreadIds).toEqual([thread.id, thread.id]);
		expect(domain.getSnapshot().hydratedThread?.turns).toHaveLength(2);
		expect(domain.getSnapshot().pendingAction).toBeNull();
	});
});

type FakeClientConfig = {
	threads: ProtocolThread[];
	threadReads?: Map<string, ProtocolThread>;
	threadResumes?: Map<string, ProtocolThread>;
	turnStarts?: ProtocolTurn[];
	forkedThreads?: Map<string, ProtocolThread>;
	loadedThreadIds?: string[];
	failConnectCalls?: number[];
};

class FakeSharedSessionClient implements SharedSessionTransport {
	threads: ProtocolThread[];
	threadReads: Map<string, ProtocolThread>;
	threadResumes: Map<string, ProtocolThread>;
	turnStarts: ProtocolTurn[];
	forkedThreads: Map<string, ProtocolThread>;
	loadedThreadIds: string[];
	interrupts: Array<{ threadId: string; turnId: string }> = [];
	steeredTurns: Array<{ threadId: string; turnId: string; text: string }> = [];
	readThreadIds: string[] = [];
	resumeThreadIds: string[] = [];
	subscriptionThreadIds: string[] = [];
	startedPrompts: Array<{
		threadId: string;
		text: string;
		overrides?: Pick<ProtocolTurnStartParams, 'cwd' | 'model' | 'personality'>;
	}> = [];
	responses: Array<{ id: JsonRpcRequestId; result: unknown }> = [];
	connectCallCount = 0;
	connectedUrls: string[] = [];
	#listener: ((notification: JsonRpcNotification) => void) | null = null;
	#serverRequestListener: ((request: JsonRpcServerRequest) => void) | null = null;
	#onClose: ((event: CloseEvent) => void) | null = null;
	#failConnectCalls: Set<number>;

	constructor(config: FakeClientConfig) {
		this.threads = config.threads;
		this.threadReads = config.threadReads ?? new Map();
		this.threadResumes = config.threadResumes ?? new Map();
		this.turnStarts = config.turnStarts ?? [];
		this.forkedThreads = config.forkedThreads ?? new Map();
		this.loadedThreadIds = [...(config.loadedThreadIds ?? [])];
		this.#failConnectCalls = new Set(config.failConnectCalls ?? []);
	}

	async connect(...args: [string, ((event: CloseEvent) => void)?]) {
		this.connectCallCount += 1;
		this.connectedUrls.push(args[0]);
		this.#onClose = args[1] ?? null;
		if (this.#failConnectCalls.has(this.connectCallCount)) {
			throw new Error(`connect attempt ${this.connectCallCount} failed`);
		}

		return Promise.resolve();
	}

	close() {
		this.#onClose?.({ code: 1000 } as CloseEvent);
		this.#onClose = null;
	}

	onNotification(listener: (notification: JsonRpcNotification) => void) {
		this.#listener = listener;
		return () => {
			this.#listener = null;
		};
	}

	onServerRequest(listener: (request: JsonRpcServerRequest) => void) {
		this.#serverRequestListener = listener;
		return () => {
			this.#serverRequestListener = null;
		};
	}

	async listThreads() {
		return { data: this.threads, nextCursor: null };
	}

	async listLoadedThreads() {
		return { data: this.loadedThreadIds, nextCursor: null };
	}

	async readThread(threadId: string): Promise<ThreadReadResult> {
		this.readThreadIds.push(threadId);
		return { thread: this.threadReads.get(threadId) ?? this.threads[0]! };
	}

	async resumeThread(threadId: string): Promise<ThreadResumeResult> {
		this.resumeThreadIds.push(threadId);
		if (!this.loadedThreadIds.includes(threadId)) {
			this.loadedThreadIds.push(threadId);
		}
		const thread = this.threadResumes.get(threadId) ?? this.threads[0]!;
		return {
			thread,
			model: 'gpt-5.4',
			modelProvider: thread.modelProvider,
			cwd: thread.cwd,
			approvalPolicy: 'on-request',
			sandbox: {
				type: 'workspaceWrite',
				networkAccess: false,
				writableRoots: [],
				excludeTmpdirEnvVar: false,
				excludeSlashTmp: false
			},
			reasoningEffort: 'high'
		};
	}

	async forkThread(
		threadId: string,
		overrides?: Pick<ProtocolTurnStartParams, 'cwd' | 'model'>
	): Promise<ThreadForkResult> {
		const thread =
			this.forkedThreads.get(threadId) ??
			makeThread(`fork_${threadId}`, this.threadReads.get(threadId)?.turns ?? [], overrides?.cwd ?? this.threads[0]!.cwd);
		if (!this.loadedThreadIds.includes(thread.id)) {
			this.loadedThreadIds.push(thread.id);
		}
		return {
			thread,
			model: overrides?.model ?? 'gpt-5.4',
			modelProvider: thread.modelProvider,
			cwd: overrides?.cwd ?? thread.cwd,
			approvalPolicy: 'on-request',
			sandbox: {
				type: 'workspaceWrite',
				networkAccess: false,
				writableRoots: [],
				excludeTmpdirEnvVar: false,
				excludeSlashTmp: false
			},
			reasoningEffort: 'high'
		};
	}

	async startThread(cwd?: string): Promise<ThreadStartResult> {
		const threadId = 'thread_started';
		if (!this.loadedThreadIds.includes(threadId)) {
			this.loadedThreadIds.push(threadId);
		}
		return {
			thread: makeThread(
				threadId,
				[],
				cwd ?? '/Users/cbusillo/Developer/code/every-code-webui'
			),
			model: 'gpt-5.4',
			modelProvider: 'openai',
			cwd: cwd ?? '/Users/cbusillo/Developer/code/every-code-webui',
			approvalPolicy: 'on-request',
			sandbox: {
				type: 'workspaceWrite',
				networkAccess: false,
				writableRoots: [],
				excludeTmpdirEnvVar: false,
				excludeSlashTmp: false
			},
			reasoningEffort: 'high'
		};
	}

	async addConversationListener(threadId: string): Promise<ConversationListenerResult> {
		this.subscriptionThreadIds.push(threadId);
		return { subscriptionId: `sub_${threadId}` };
	}

	async startTurn(
		threadId: string,
		text: string,
		overrides?: Pick<ProtocolTurnStartParams, 'cwd' | 'model' | 'personality'>
	): Promise<TurnStartResult> {
		this.startedPrompts.push({ threadId, text, overrides });
		return { turn: this.turnStarts.shift() ?? makeTurn('turn_started', 'inProgress', text) };
	}

	async interruptTurn(threadId: string, turnId: string) {
		this.interrupts.push({ threadId, turnId });
		return {};
	}

	async steerTurn(threadId: string, turnId: string, text: string): Promise<TurnSteerResult> {
		this.steeredTurns.push({ threadId, turnId, text });
		return { turnId: turnId };
	}

	async respond(id: JsonRpcRequestId, result: unknown) {
		this.responses.push({ id, result });
	}

	emit(notification: JsonRpcNotification) {
		this.#listener?.(notification);
	}

	emitServerRequest(request: JsonRpcServerRequest) {
		this.#serverRequestListener?.(request);
	}

	simulateUnexpectedClose() {
		this.#onClose?.({ code: 1006 } as CloseEvent);
		this.#onClose = null;
	}
}

function settledTurn(id: string, promptText: string): ProtocolTurn {
	return makeTurn(id, 'completed', promptText);
}

function makeThread(
	id: string,
	turns: ProtocolTurn[],
	cwd = '/Users/cbusillo/Developer/code/every-code-webui'
): ProtocolThread {
	return {
		id,
		preview: `Preview for ${id}`,
		modelProvider: 'openai',
		createdAt: 1,
		updatedAt: 2,
		path: `${cwd}/.code/${id}.jsonl`,
		cwd,
		cliVersion: '0.0.0',
		source: 'cli',
		gitInfo: {
			branch: 'main',
			sha: 'abc123',
			originUrl: 'https://github.com/just-every/code'
		},
		turns
	};
}

function makeTurn(id: string, status: ProtocolTurn['status'], text: string): ProtocolTurn {
	return {
		id,
		status,
		error: null,
		items: [
			{
				type: 'userMessage',
				id: `${id}_user`,
				content: [{ type: 'text', text, text_elements: [] }]
			}
		]
	};
}

function makeStorage(): Storage {
	const values = new Map<string, string>();

	return {
		get length() {
			return values.size;
		},
		clear() {
			values.clear();
		},
		getItem(key) {
			return values.get(key) ?? null;
		},
		key(index) {
			return [...values.keys()][index] ?? null;
		},
		removeItem(key) {
			values.delete(key);
		},
		setItem(key, value) {
			values.set(key, value);
		}
	};
}

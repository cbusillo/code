import { get, readable, writable, type Readable } from 'svelte/store';

import { SharedSessionClient } from '$lib/shared-session-client';
import { assessWebSocketConnection } from '$lib/shared-session-connection-policy';
import {
	LIVE_EVENT_REFRESH_DELAY_MS,
	shouldHydrateThreadFromNotification
} from '$lib/shared-session-live-sync';
import type {
	ConversationListenerResult,
	JsonRpcNotification,
	JsonRpcRequestId,
	JsonRpcServerRequest,
	ProtocolCommandExecutionApprovalDecision,
	ProtocolCommandExecutionRequestApprovalParams,
	ProtocolCommandExecutionRequestApprovalResponse,
	ProtocolFileChangeApprovalDecision,
	ProtocolFileChangeRequestApprovalParams,
	ProtocolFileChangeRequestApprovalResponse,
	ProtocolPersonality,
	ProtocolThread,
	ProtocolTurnStartParams,
	ProtocolToolRequestUserInputAnswer,
	ProtocolToolRequestUserInputParams,
	ProtocolToolRequestUserInputResponse,
	ProtocolTurn,
	ThreadForkResult,
	ThreadLoadedListResult,
	ThreadReadResult,
	ThreadResumeResult,
	ThreadStartResult,
	TurnSteerResult,
	TurnStartResult
} from '$lib/shared-session-types';

const DEFAULT_WS_URL = 'ws://127.0.0.1:8877';
const STORAGE_KEYS = {
	wsUrl: 'every-code-webui.ws-url',
	cwd: 'every-code-webui.cwd',
	startPrompt: 'every-code-webui.start-prompt',
	composerText: 'every-code-webui.composer-text',
	turnOverrideModel: 'every-code-webui.turn-override-model',
	turnOverrideCwd: 'every-code-webui.turn-override-cwd',
	turnOverridePersonality: 'every-code-webui.turn-override-personality',
	selectedThreadId: 'every-code-webui.selected-thread-id'
} as const;

const AUTO_RECONNECT_DELAYS_MS = [1_500, 3_000, 5_000] as const;

export type ConnectionPhase = 'offline' | 'connecting' | 'connected' | 'error';

export type SharedSessionEvent = {
	id: string;
	at: number;
	method: string;
	summary: string;
	threadId: string | null;
};

export type SharedSessionTurnOverrides = Pick<
	ProtocolTurnStartParams,
	'cwd' | 'model' | 'personality'
>;

export type SharedSessionApprovalRequest = {
	kind: 'command-approval' | 'file-approval';
	id: string;
	requestId: JsonRpcRequestId;
	threadId: string;
	turnId: string;
	itemId: string;
	status: 'pending' | 'answered';
	label: string;
	detail: string;
	primary: string;
	secondary: string;
	options: Array<{
		id: string;
		label: string;
		description: string;
		decision: ProtocolCommandExecutionApprovalDecision | ProtocolFileChangeApprovalDecision;
	}>;
	resolution?: string;
	respondedBy?: string;
};

export type SharedSessionQuestionRequest = {
	kind: 'question';
	id: string;
	requestId: JsonRpcRequestId;
	threadId: string;
	turnId: string;
	itemId: string;
	questionId: string;
	status: 'pending' | 'answered';
	header: string;
	prompt: string;
	options: Array<{ label: string; description: string }>;
	allowOther: boolean;
	allowSecret: boolean;
	answer?: string;
	respondedBy?: string;
};

export type SharedSessionStructuredRequest =
	| SharedSessionApprovalRequest
	| SharedSessionQuestionRequest;

export type SharedSessionState = {
	connectionPhase: ConnectionPhase;
	statusMessage: string;
	wsUrl: string;
	startCwd: string;
	startPrompt: string;
	composerText: string;
	turnOverrideModel: string;
	turnOverrideCwd: string;
	turnOverridePersonality: '' | ProtocolPersonality;
	threads: ProtocolThread[];
	loadedThreadIds: string[];
	attachedThreadIds: string[];
	selectedThreadId: string | null;
	hydratedThread: ProtocolThread | null;
	activeThreadId: string | null;
	takeoverRequestedThreadId: string | null;
	pendingAction: string | null;
	events: SharedSessionEvent[];
	structuredRequests: SharedSessionStructuredRequest[];
};

export interface SharedSessionTransport {
	connect(url: string, onClose?: (event: CloseEvent) => void): Promise<void>;
	close(): void;
	onNotification(listener: (notification: JsonRpcNotification) => void): () => void;
	onServerRequest(listener: (request: JsonRpcServerRequest) => void): () => void;
	listThreads(cwd?: string): Promise<{ data: ProtocolThread[]; nextCursor: string | null }>;
	listLoadedThreads(): Promise<ThreadLoadedListResult>;
	readThread(threadId: string): Promise<ThreadReadResult>;
	resumeThread(threadId: string): Promise<ThreadResumeResult>;
	forkThread(
		threadId: string,
		overrides?: Pick<ProtocolTurnStartParams, 'cwd' | 'model'>
	): Promise<ThreadForkResult>;
	startThread(cwd?: string): Promise<ThreadStartResult>;
	addConversationListener(threadId: string): Promise<ConversationListenerResult>;
	startTurn(
		threadId: string,
		text: string,
		overrides?: SharedSessionTurnOverrides
	): Promise<TurnStartResult>;
	interruptTurn(threadId: string, turnId: string): Promise<unknown>;
	steerTurn(threadId: string, turnId: string, text: string): Promise<TurnSteerResult>;
	respond(id: JsonRpcRequestId, result: unknown): Promise<void>;
}

const initialState: SharedSessionState = {
	connectionPhase: 'offline',
	statusMessage: 'Waiting for a local app-server.',
	wsUrl: DEFAULT_WS_URL,
	startCwd: '',
	startPrompt: '',
	composerText: '',
	turnOverrideModel: '',
	turnOverrideCwd: '',
	turnOverridePersonality: '',
	threads: [],
	loadedThreadIds: [],
	attachedThreadIds: [],
	selectedThreadId: null,
	hydratedThread: null,
	activeThreadId: null,
	takeoverRequestedThreadId: null,
	pendingAction: null,
	events: [],
	structuredRequests: []
};

export class SharedSessionDomain {
	#client: SharedSessionTransport;
	#state = writable<SharedSessionState>(loadPersistedState());
	#subscriptions = new Map<string, string>();
	#unsubscribeNotifications: (() => void) | null = null;
	#unsubscribeServerRequests: (() => void) | null = null;
	#nextEventId = 1;
	#ignoreSocketClose = false;
	#autoReconnectEnabled = false;
	#reconnectTimer: ReturnType<typeof setTimeout> | null = null;
	#liveRefreshTimer: ReturnType<typeof setTimeout> | null = null;
	#liveRefreshInFlight = false;
	#pendingLiveRefreshThreadId: string | null = null;

	constructor(client: SharedSessionTransport = new SharedSessionClient()) {
		this.#client = client;
	}

	get store(): Readable<SharedSessionState> {
		return readable(get(this.#state), (set) => this.#state.subscribe(set));
	}

	getSnapshot() {
		return get(this.#state);
	}

	setWsUrl(wsUrl: string) {
		persistValue(STORAGE_KEYS.wsUrl, wsUrl);
		this.#state.update((state) => ({ ...state, wsUrl }));
	}

	setStartCwd(startCwd: string) {
		persistValue(STORAGE_KEYS.cwd, startCwd);
		this.#state.update((state) => ({ ...state, startCwd }));
	}

	setStartPrompt(startPrompt: string) {
		persistOptionalValue(STORAGE_KEYS.startPrompt, startPrompt);
		this.#state.update((state) => ({ ...state, startPrompt }));
	}

	setComposerText(composerText: string) {
		persistOptionalValue(STORAGE_KEYS.composerText, composerText);
		this.#state.update((state) => ({ ...state, composerText }));
	}

	setTurnOverrideModel(turnOverrideModel: string) {
		persistOptionalValue(STORAGE_KEYS.turnOverrideModel, turnOverrideModel);
		this.#state.update((state) => ({ ...state, turnOverrideModel }));
	}

	setTurnOverrideCwd(turnOverrideCwd: string) {
		persistOptionalValue(STORAGE_KEYS.turnOverrideCwd, turnOverrideCwd);
		this.#state.update((state) => ({ ...state, turnOverrideCwd }));
	}

	setTurnOverridePersonality(turnOverridePersonality: '' | ProtocolPersonality) {
		persistOptionalValue(STORAGE_KEYS.turnOverridePersonality, turnOverridePersonality);
		this.#state.update((state) => ({ ...state, turnOverridePersonality }));
	}

	selectThread(threadId: string) {
		persistOptionalValue(STORAGE_KEYS.selectedThreadId, threadId);
		this.#state.update((state) => {
			const hydratedThread = state.hydratedThread?.id === threadId ? state.hydratedThread : null;

			return {
				...state,
				selectedThreadId: threadId,
				hydratedThread,
				takeoverRequestedThreadId:
					state.takeoverRequestedThreadId === null || state.takeoverRequestedThreadId === threadId
						? state.takeoverRequestedThreadId
						: null
			};
		});
	}

	async connect() {
		this.#autoReconnectEnabled = true;
		this.#clearReconnectTimer();
		await this.#connectWithRestore(this.getSnapshot().selectedThreadId, 0);
	}

	disconnect(message = 'Offline') {
		this.#autoReconnectEnabled = false;
		this.#clearReconnectTimer();
		this.#resetLiveConnection(message, true);
	}

	async refreshThreads() {
		await this.#runAction('refreshing sessions', async () => {
			this.#assertConnected();
			const selectedThreadId = await this.#refreshThreadInventory();
			this.#state.update((state) => ({
				...state,
				statusMessage: `Loaded ${state.threads.length} thread${state.threads.length === 1 ? '' : 's'}.`
			}));
			persistOptionalValue(STORAGE_KEYS.selectedThreadId, selectedThreadId);
		});
	}

	async refreshForForeground() {
		if (this.getSnapshot().connectionPhase !== 'connected') {
			return;
		}

		await this.#refreshThreadInventory();
		const selectedThreadId = this.getSnapshot().selectedThreadId;
		if (!selectedThreadId) {
			return;
		}

		await this.#refreshThreadSilently(selectedThreadId);
	}

	async hydrateSelectedThread() {
		const selectedThread = this.#getSelectedThread();
		if (!selectedThread) {
			return;
		}

		await this.#runAction('hydrating transcript', async () => {
			this.#assertConnected();
			const response = await this.#client.readThread(selectedThread.id);
			persistOptionalValue(STORAGE_KEYS.selectedThreadId, response.thread.id);

			this.#state.update((state) => ({
				...state,
				threads: upsertThread(state.threads, response.thread),
				hydratedThread: response.thread,
				selectedThreadId: response.thread.id,
				activeThreadId: null,
				statusMessage: `Hydrated ${response.thread.id}.`
			}));
		});
	}

	async startThread() {
		const { startCwd, startPrompt } = this.getSnapshot();

		await this.#runAction('starting thread', async () => {
			this.#assertConnected();
			const response = await this.#client.startThread(startCwd.trim() || undefined);
			await this.#ensureListener(response.thread.id);

			let hydratedThread = response.thread;
			if (startPrompt.trim()) {
				const turnResponse = await this.#client.startTurn(response.thread.id, startPrompt.trim());
				hydratedThread = mergeTurn(
					response.thread,
					seedTurnWithPrompt(turnResponse.turn, startPrompt.trim())
				);
			}

			persistOptionalValue(STORAGE_KEYS.startPrompt, '');
			persistOptionalValue(STORAGE_KEYS.selectedThreadId, hydratedThread.id);

			this.#state.update((state) => ({
				...state,
				threads: upsertThread(state.threads, hydratedThread),
				selectedThreadId: hydratedThread.id,
				hydratedThread,
				activeThreadId: hydratedThread.id,
				startPrompt: '',
				statusMessage: `Started ${hydratedThread.id}.`
			}));
		});
	}

	async sendPrompt() {
		const { composerText } = this.getSnapshot();
		if (!composerText.trim()) {
			this.#updateStatus('Enter a prompt before sending it.');
			return;
		}

		const selectedThread = this.#getSelectedThread();
		if (
			selectedThread &&
			this.getSnapshot().loadedThreadIds.includes(selectedThread.id) &&
			!this.getSnapshot().attachedThreadIds.includes(selectedThread.id)
		) {
			this.#updateStatus(
				'This thread is already attached on another surface. Take over here before sending a new prompt.'
			);
			return;
		}

		if (
			selectedThread &&
			selectedThread.turns.some((turn) => turn.status === 'inProgress') &&
			this.getSnapshot().activeThreadId !== selectedThread.id
		) {
			this.#updateStatus(
				'This thread is still running elsewhere. Wait for it to settle or take over here before sending a new prompt.'
			);
			return;
		}

		await this.#runAction('sending prompt', async () => {
			this.#assertConnected();
			const thread = await this.#ensureActiveThread();
			const turnResponse = await this.#client.startTurn(
				thread.id,
				composerText.trim(),
				buildTurnOverrides(this.getSnapshot())
			);
			const seededTurn = seedTurnWithPrompt(turnResponse.turn, composerText.trim());
			persistOptionalValue(STORAGE_KEYS.composerText, '');

			this.#state.update((state) => ({
				...state,
				threads: state.hydratedThread
					? upsertThread(state.threads, mergeTurn(state.hydratedThread, seededTurn))
					: state.threads,
				hydratedThread: state.hydratedThread
					? mergeTurn(state.hydratedThread, seededTurn)
					: state.hydratedThread,
				composerText: '',
				statusMessage: `Prompt sent to ${thread.id}.`
			}));
			await this.#refreshThreadInventory(thread.id);
		});
	}

	async steerSelectedTurn(text: string) {
		const steerText = text.trim();
		if (!steerText) {
			this.#updateStatus('Add follow-up steering before sending it to the active turn.');
			return;
		}

		await this.#runAction('steering active turn', async () => {
			this.#assertConnected();
			const thread = this.#getSelectedThread();
			if (!thread) {
				throw new Error('Select a thread before steering the active turn.');
			}

			const activeTurn = [...thread.turns].reverse().find((turn) => turn.status === 'inProgress');
			if (!activeTurn) {
				throw new Error('No active turn is running on this thread.');
			}

			await this.#client.steerTurn(thread.id, activeTurn.id, steerText);
			this.#state.update((state) => ({
				...state,
				statusMessage: `Steering queued for ${thread.id}.`
			}));
		});
	}

	async forkSelectedThread() {
		await this.#runAction('forking thread', async () => {
			this.#assertConnected();
			const thread = this.#getSelectedThread();
			if (!thread) {
				throw new Error('Select a thread before branching from it.');
			}

			const response = await this.#client.forkThread(thread.id, buildForkOverrides(this.getSnapshot()));
			await this.#ensureListener(response.thread.id);
			persistOptionalValue(STORAGE_KEYS.selectedThreadId, response.thread.id);
			this.#state.update((state) => ({
				...state,
				threads: upsertThread(state.threads, response.thread),
				selectedThreadId: response.thread.id,
				hydratedThread: response.thread,
				activeThreadId: response.thread.id,
				statusMessage: `Forked ${thread.id} into ${response.thread.id}.`
			}));
		});
	}

	async resumeSelectedThread() {
		await this.#runAction('resuming session', async () => {
			this.#assertConnected();
			const thread = await this.#ensureActiveThread();
			this.#state.update((state) => ({
				...state,
				takeoverRequestedThreadId: null,
				statusMessage: `Live updates attached to ${thread.id}.`
			}));
		});
	}

	async takeOverSelectedThread() {
		const selectedThread = this.#getSelectedThread();
		if (!selectedThread) {
			throw new Error('Select a thread before taking it over here.');
		}

		const hasRunningTurn = selectedThread.turns.some((turn) => turn.status === 'inProgress');
		if (hasRunningTurn && this.getSnapshot().activeThreadId !== selectedThread.id) {
			this.#state.update((state) => ({
				...state,
				takeoverRequestedThreadId: selectedThread.id,
				statusMessage: `Takeover queued for ${selectedThread.id}. Live updates will attach here when the current turn settles.`
			}));
			return;
		}

		await this.resumeSelectedThread();
	}

	async interruptSelectedTurn() {
		await this.#runAction('interrupting turn', async () => {
			this.#assertConnected();
			const thread = this.#getSelectedThread();
			if (!thread) {
				throw new Error('Select a thread before trying to stop it.');
			}

			const activeTurn = [...thread.turns].reverse().find((turn) => turn.status === 'inProgress');
			if (!activeTurn) {
				throw new Error('No active turn is running on this thread.');
			}

			await this.#client.interruptTurn(thread.id, activeTurn.id);
			this.#state.update((state) => ({
				...state,
				statusMessage: `Interrupt requested for ${thread.id}.`
			}));
		});
	}

	async answerApproval(
		requestId: JsonRpcRequestId,
		decision: ProtocolCommandExecutionApprovalDecision | ProtocolFileChangeApprovalDecision,
		respondedBy = 'This browser'
	) {
		const request = this.getSnapshot().structuredRequests.find(
			(entry) => entry.requestId === requestId && entry.kind !== 'question'
		);
		if (!request) {
			throw new Error('Structured approval request not found.');
		}

		await this.#runAction('answering approval', async () => {
			this.#assertConnected();
			const response =
				request.kind === 'command-approval'
					? ({
							decision: decision as ProtocolCommandExecutionApprovalDecision
						} satisfies ProtocolCommandExecutionRequestApprovalResponse)
					: ({
							decision: decision as ProtocolFileChangeApprovalDecision
						} satisfies ProtocolFileChangeRequestApprovalResponse);

			await this.#client.respond(requestId, response);
			this.#markStructuredRequestAnswered(requestId, {
				resolution: formatApprovalResolution(decision),
				respondedBy
			});
			await this.#refreshThreadInventory(request.threadId);
		});
	}

	async answerQuestion(
		requestId: JsonRpcRequestId,
		questionId: string,
		answer: string,
		respondedBy = 'This browser'
	) {
		await this.answerQuestions(requestId, { [questionId]: [answer] }, respondedBy);
	}

	async answerQuestions(
		requestId: JsonRpcRequestId,
		answers: Record<string, string[]>,
		respondedBy = 'This browser'
	) {
		const request = this.getSnapshot().structuredRequests.find(
			(entry) => entry.kind === 'question' && entry.requestId === requestId
		);
		if (!request) {
			throw new Error('Structured question request not found.');
		}

		await this.#runAction('answering question', async () => {
			this.#assertConnected();
			const response = {
				answers: Object.fromEntries(
					Object.entries(answers)
						.filter(([, value]) => value.length)
						.map(([key, value]) => [
							key,
							{ answers: value } satisfies ProtocolToolRequestUserInputAnswer
						])
				)
			} satisfies ProtocolToolRequestUserInputResponse;

			await this.#client.respond(requestId, response);
			this.#markStructuredQuestionsAnswered(requestId, answers, respondedBy);
			await this.#refreshThreadInventory(request.threadId);
		});
	}

	async #ensureActiveThread() {
		const state = this.getSnapshot();
		const selectedThread = this.#getSelectedThread();
		if (!selectedThread) {
			throw new Error('Select or start a thread first.');
		}

		if (state.activeThreadId === selectedThread.id) {
			await this.#ensureListener(selectedThread.id);
			return selectedThread;
		}

		const response = await this.#client.resumeThread(selectedThread.id);
		await this.#ensureListener(response.thread.id);
		persistOptionalValue(STORAGE_KEYS.selectedThreadId, response.thread.id);

		this.#state.update((currentState) => ({
			...currentState,
			threads: upsertThread(currentState.threads, response.thread),
			selectedThreadId: response.thread.id,
			hydratedThread: response.thread,
			activeThreadId: response.thread.id,
			statusMessage: `Resumed ${response.thread.id} for live updates.`
		}));

		return response.thread;
	}

	async #ensureListener(threadId: string) {
		if (this.#subscriptions.has(threadId)) {
			return;
		}

		const response = await this.#client.addConversationListener(threadId);
		this.#subscriptions.set(threadId, response.subscriptionId);
		this.#state.update((state) => ({
			...state,
			loadedThreadIds: appendUniqueId(state.loadedThreadIds, threadId),
			attachedThreadIds: appendUniqueId(state.attachedThreadIds, threadId)
		}));
	}

	async #restoreSelectedThreadOnConnect(previousSelectedThreadId: string | null) {
		if (!previousSelectedThreadId) {
			return;
		}

		const { selectedThreadId } = this.getSnapshot();
		if (selectedThreadId !== previousSelectedThreadId) {
			return;
		}

		await this.hydrateSelectedThread();
		const snapshot = this.getSnapshot();
		if (snapshot.takeoverRequestedThreadId === previousSelectedThreadId) {
			const selectedThread = this.#getSelectedThread();
			if (selectedThread?.turns.some((turn) => turn.status === 'inProgress')) {
				this.#state.update((state) => ({
					...state,
					statusMessage: `Reconnected to ${previousSelectedThreadId}. Takeover is still queued here until the active turn settles.`
				}));
				return;
			}
		}

		await this.resumeSelectedThread();
	}

	async #connectWithRestore(previousSelectedThreadId: string | null, reconnectAttempt: number) {
		const { wsUrl } = this.getSnapshot();
		const connectionAssessment = assessWebSocketConnection(wsUrl);
		if (!connectionAssessment.normalizedUrl) {
			this.#state.update((state) => ({
				...state,
				connectionPhase: 'error',
				statusMessage: connectionAssessment.headline
			}));
			return;
		}

		if (!connectionAssessment.canConnect) {
			this.#state.update((state) => ({
				...state,
				connectionPhase: 'error',
				statusMessage: connectionAssessment.recommendation
			}));
			return;
		}

		const normalizedUrl = connectionAssessment.normalizedUrl;

		this.#resetLiveConnection(
			reconnectAttempt > 0 ? `Reconnecting to ${normalizedUrl}...` : 'Reconnecting...',
			true,
			this.getSnapshot().takeoverRequestedThreadId !== null
		);
		this.#unsubscribeNotifications = this.#client.onNotification((notification) =>
			this.#handleNotification(notification)
		);
		this.#unsubscribeServerRequests = this.#client.onServerRequest((request) =>
			this.#handleServerRequest(request)
		);

		this.#state.update((state) => ({
			...state,
			connectionPhase: 'connecting',
			statusMessage:
				reconnectAttempt > 0
					? `Reconnecting to ${normalizedUrl} (attempt ${reconnectAttempt + 1} of ${AUTO_RECONNECT_DELAYS_MS.length + 1})...`
					: `Connecting to ${normalizedUrl}...`
		}));

		try {
			await this.#client.connect(normalizedUrl, () => this.#handleSocketClose());
			this.#clearReconnectTimer();
			this.#state.update((state) => ({
				...state,
				connectionPhase: 'connected',
				statusMessage: `Connected to ${normalizedUrl}.`
			}));
			await this.refreshThreads();
			await this.#restoreSelectedThreadOnConnect(previousSelectedThreadId);
		} catch (error) {
			this.#clearTransportSubscriptions();
			if (this.#autoReconnectEnabled && reconnectAttempt < AUTO_RECONNECT_DELAYS_MS.length) {
				this.#scheduleReconnect(previousSelectedThreadId, reconnectAttempt);
				return;
			}

			this.#setError(error, `Failed to connect to ${normalizedUrl}.`);
		}
	}

	#handleSocketClose() {
		if (this.#ignoreSocketClose) {
			this.#ignoreSocketClose = false;
			return;
		}

		this.#clearTransportSubscriptions();
		const selectedThreadId = this.getSnapshot().selectedThreadId;
		if (this.#autoReconnectEnabled) {
			this.#scheduleReconnect(selectedThreadId, 0);
			return;
		}

		this.#state.update((state) => ({
			...state,
			connectionPhase: 'offline',
			statusMessage: 'Socket closed.',
			activeThreadId: null,
			pendingAction: null
		}));
	}

	#handleNotification(notification: JsonRpcNotification) {
		const params = isObject(notification.params) ? notification.params : null;
		const threadId = readString(params, 'threadId') ?? readString(params, 'conversationId');
		const turn = readTurn(params?.turn);

		if (threadId && turn) {
			this.#state.update((state) => {
				const nextHydratedThread =
					state.hydratedThread?.id === threadId ? mergeTurn(state.hydratedThread, turn) : state.hydratedThread;
				const nextThreads = nextHydratedThread ? upsertThread(state.threads, nextHydratedThread) : state.threads;

				if (state.hydratedThread?.id !== threadId) {
					return {
						...state,
						threads: nextThreads,
						activeThreadId:
							notification.method === 'turn/started'
								? threadId
								: notification.method === 'turn/completed' && state.activeThreadId === threadId
									? null
									: state.activeThreadId,
						statusMessage: summarizeTurnNotification(notification.method, threadId, turn)
					};
				}

				return {
					...state,
					threads: nextThreads,
					hydratedThread: nextHydratedThread,
					activeThreadId:
						notification.method === 'turn/started'
							? threadId
							: notification.method === 'turn/completed' && state.activeThreadId === threadId
								? null
								: state.activeThreadId,
					statusMessage: summarizeTurnNotification(notification.method, threadId, turn)
				};
			});

			if (notification.method === 'turn/completed') {
				void this.#resumeQueuedTakeover(threadId);
			}
		}

		if (
			shouldHydrateThreadFromNotification(
				notification.method,
				threadId,
				this.getSnapshot().selectedThreadId,
				this.getSnapshot().activeThreadId
			)
		) {
			this.#scheduleLiveRefresh(threadId!);
		}

		this.#state.update((state) => ({
			...state,
			events: [
				{
					id: `event_${this.#nextEventId++}`,
					at: Date.now(),
					method: notification.method,
					summary: summarizeNotification(notification.method, params),
					threadId: threadId ?? null
				},
				...state.events
			].slice(0, 40)
		}));
	}

	#handleServerRequest(request: JsonRpcServerRequest) {
		const structuredRequests = mapServerRequest(request);
		if (!structuredRequests.length) {
			return;
		}

		this.#state.update((state) => ({
			...state,
			structuredRequests: mergeStructuredRequests(state.structuredRequests, structuredRequests),
			events: [
				{
					id: `event_${this.#nextEventId++}`,
					at: Date.now(),
					method: request.method,
					summary: summarizeServerRequest(structuredRequests),
					threadId: structuredRequests[0]?.threadId ?? null
				},
				...state.events
			].slice(0, 40)
		}));
	}

	#markStructuredRequestAnswered(
		requestId: JsonRpcRequestId,
		resolution: { resolution: string; respondedBy: string }
	) {
		this.#state.update((state) => ({
			...state,
			structuredRequests: state.structuredRequests.map((request) => {
				if (request.requestId !== requestId) {
					return request;
				}

				if ('resolution' in resolution) {
					return {
						...request,
						status: 'answered',
						resolution: resolution.resolution,
						respondedBy: resolution.respondedBy
					};
				}

				return request;
			}),
			statusMessage: 'Structured response sent.'
		}));
	}

	#markStructuredQuestionsAnswered(
		requestId: JsonRpcRequestId,
		answers: Record<string, string[]>,
		respondedBy: string
	) {
		this.#state.update((state) => ({
			...state,
			structuredRequests: state.structuredRequests.map((request) => {
				if (request.kind !== 'question' || request.requestId !== requestId) {
					return request;
				}

				const nextAnswers = answers[request.questionId] ?? [];
				if (!nextAnswers.length) {
					return request;
				}

				return {
					...request,
					status: 'answered',
					answer: nextAnswers.join(', '),
					respondedBy
				};
			}),
			statusMessage: 'Structured response sent.'
		}));
	}

	#getSelectedThread() {
		const state = this.getSnapshot();
		if (state.hydratedThread && state.hydratedThread.id === state.selectedThreadId) {
			return state.hydratedThread;
		}

		return state.threads.find((thread) => thread.id === state.selectedThreadId) ?? null;
	}

	#assertConnected() {
		const { connectionPhase } = this.getSnapshot();
		if (connectionPhase !== 'connected') {
			throw new Error('Connect to the app-server first.');
		}
	}

	#updateStatus(statusMessage: string) {
		this.#state.update((state) => ({
			...state,
			statusMessage
		}));
	}

	#scheduleReconnect(previousSelectedThreadId: string | null, reconnectAttempt: number) {
		const delayMs = AUTO_RECONNECT_DELAYS_MS[reconnectAttempt] ?? AUTO_RECONNECT_DELAYS_MS.at(-1)!;
		this.#clearReconnectTimer();
		this.#state.update((state) => ({
			...state,
			connectionPhase: 'connecting',
			statusMessage: `Connection lost. Reconnecting in ${formatReconnectDelay(delayMs)} (attempt ${reconnectAttempt + 1} of ${AUTO_RECONNECT_DELAYS_MS.length + 1})...`,
			activeThreadId: null,
			pendingAction: null,
			structuredRequests: []
		}));

		this.#reconnectTimer = setTimeout(() => {
			this.#reconnectTimer = null;
			void this.#connectWithRestore(previousSelectedThreadId, reconnectAttempt + 1);
		}, delayMs);
	}

	#clearReconnectTimer() {
		if (!this.#reconnectTimer) {
			return;
		}

		clearTimeout(this.#reconnectTimer);
		this.#reconnectTimer = null;
	}

	#clearLiveRefreshTimer() {
		if (!this.#liveRefreshTimer) {
			return;
		}

		clearTimeout(this.#liveRefreshTimer);
		this.#liveRefreshTimer = null;
	}

	#clearTransportSubscriptions() {
		this.#unsubscribeNotifications?.();
		this.#unsubscribeServerRequests?.();
		this.#unsubscribeNotifications = null;
		this.#unsubscribeServerRequests = null;
		this.#subscriptions.clear();
	}

	#resetLiveConnection(
		message: string,
		ignoreSocketClose: boolean,
		preserveTakeoverRequestedThreadId = false
	) {
		this.#clearLiveRefreshTimer();
		this.#pendingLiveRefreshThreadId = null;
		this.#liveRefreshInFlight = false;
		this.#ignoreSocketClose =
			ignoreSocketClose &&
			(this.#unsubscribeNotifications !== null ||
				this.#unsubscribeServerRequests !== null ||
				this.#subscriptions.size > 0);
		this.#client.close();
		this.#clearTransportSubscriptions();
		this.#state.update((state) => ({
			...state,
			connectionPhase: 'offline',
			statusMessage: message,
			loadedThreadIds: [],
			attachedThreadIds: [],
			activeThreadId: null,
			takeoverRequestedThreadId: preserveTakeoverRequestedThreadId
				? state.takeoverRequestedThreadId
				: null,
			pendingAction: null,
			structuredRequests: []
		}));
	}

	async #runAction(action: string, work: () => Promise<void>) {
		this.#state.update((state) => ({
			...state,
			pendingAction: action,
			statusMessage: `${capitalize(action)}...`
		}));

		try {
			await work();
			this.#state.update((state) => ({
				...state,
				connectionPhase: 'connected',
				pendingAction: null
			}));
		} catch (error) {
			this.#setError(error);
		}
	}

	async #refreshThreadInventory(preferredSelectedThreadId: string | null = this.getSnapshot().selectedThreadId) {
		const [response, loadedResponse] = await Promise.all([
			this.#client.listThreads(),
			this.#client.listLoadedThreads()
		]);
		let selectedThreadId: string | null = null;

		this.#state.update((state) => {
			const loadedThreadIds = loadedResponse.data;
			const attachedThreadIds = state.attachedThreadIds.filter((threadId) =>
				loadedThreadIds.includes(threadId)
			);
			selectedThreadId =
				preferredSelectedThreadId && response.data.some((thread) => thread.id === preferredSelectedThreadId)
					? preferredSelectedThreadId
					: state.selectedThreadId &&
						response.data.some((thread) => thread.id === state.selectedThreadId)
						? state.selectedThreadId
						: (response.data[0]?.id ?? null);

			const hydratedThread =
				state.hydratedThread && state.hydratedThread.id === selectedThreadId
					? state.hydratedThread
					: null;
			const threads = hydratedThread ? upsertThread(response.data, hydratedThread) : response.data;

			return {
				...state,
				threads,
				loadedThreadIds,
				attachedThreadIds,
				selectedThreadId,
				hydratedThread
			};
		});

		return selectedThreadId;
	}

	#setError(error: unknown, fallback = 'Something went wrong while talking to the app-server.') {
		const message = error instanceof Error ? error.message : fallback;

		this.#state.update((state) => ({
			...state,
			connectionPhase: 'error',
			statusMessage: message,
			pendingAction: null
		}));
	}

	async #resumeQueuedTakeover(threadId: string) {
		const snapshot = this.getSnapshot();
		if (snapshot.takeoverRequestedThreadId !== threadId) {
			return;
		}

		if (snapshot.selectedThreadId !== threadId) {
			this.#state.update((state) => ({
				...state,
				takeoverRequestedThreadId: null
			}));
			return;
		}

		const selectedThread = this.#getSelectedThread();
		if (selectedThread?.turns.some((turn) => turn.status === 'inProgress')) {
			return;
		}

		await this.resumeSelectedThread();
	}

	#scheduleLiveRefresh(threadId: string) {
		this.#pendingLiveRefreshThreadId = threadId;
		this.#clearLiveRefreshTimer();
		this.#liveRefreshTimer = setTimeout(() => {
			this.#liveRefreshTimer = null;
			void this.#flushLiveRefresh();
		}, LIVE_EVENT_REFRESH_DELAY_MS);
	}

	async #flushLiveRefresh() {
		if (this.#liveRefreshInFlight) {
			return;
		}

		const threadId = this.#pendingLiveRefreshThreadId;
		if (!threadId) {
			return;
		}

		this.#pendingLiveRefreshThreadId = null;
		this.#liveRefreshInFlight = true;

		try {
			await this.#refreshThreadSilently(threadId);
		} finally {
			this.#liveRefreshInFlight = false;
			if (this.#pendingLiveRefreshThreadId) {
				this.#scheduleLiveRefresh(this.#pendingLiveRefreshThreadId);
			}
		}
	}

	async #refreshThreadSilently(threadId: string) {
		if (this.getSnapshot().connectionPhase !== 'connected') {
			return;
		}

		const response = await this.#client.readThread(threadId);
		this.#state.update((state) => {
			if (state.selectedThreadId !== threadId && state.activeThreadId !== threadId) {
				return state;
			}

			const hydratedThread = response.thread;
			return {
				...state,
				threads: upsertThread(state.threads, hydratedThread),
				hydratedThread:
					state.selectedThreadId === threadId ? hydratedThread : state.hydratedThread
			};
		});
	}
}

function formatReconnectDelay(delayMs: number) {
	if (delayMs % 1000 === 0) {
		return `${delayMs / 1000}s`;
	}

	return `${(delayMs / 1000).toFixed(1)}s`;
}

function loadPersistedState(): SharedSessionState {
	if (typeof window === 'undefined') {
		return initialState;
	}

	return {
		...initialState,
		wsUrl: window.localStorage.getItem(STORAGE_KEYS.wsUrl) ?? initialState.wsUrl,
		startCwd: window.localStorage.getItem(STORAGE_KEYS.cwd) ?? initialState.startCwd,
		startPrompt: window.localStorage.getItem(STORAGE_KEYS.startPrompt) ?? initialState.startPrompt,
		composerText:
			window.localStorage.getItem(STORAGE_KEYS.composerText) ?? initialState.composerText,
		turnOverrideModel:
			window.localStorage.getItem(STORAGE_KEYS.turnOverrideModel) ??
			initialState.turnOverrideModel,
		turnOverrideCwd:
			window.localStorage.getItem(STORAGE_KEYS.turnOverrideCwd) ??
			initialState.turnOverrideCwd,
		turnOverridePersonality:
			normalizeStoredPersonality(window.localStorage.getItem(STORAGE_KEYS.turnOverridePersonality)) ??
			initialState.turnOverridePersonality,
		selectedThreadId: window.localStorage.getItem(STORAGE_KEYS.selectedThreadId)
	};
}

function buildTurnOverrides(state: SharedSessionState): SharedSessionTurnOverrides | undefined {
	const model = trimOptionalText(state.turnOverrideModel);
	const cwd = trimOptionalText(state.turnOverrideCwd);
	const personality = state.turnOverridePersonality || undefined;

	if (!model && !cwd && !personality) {
		return undefined;
	}

	return {
		...(model ? { model } : {}),
		...(cwd ? { cwd } : {}),
		...(personality ? { personality } : {})
	};
}

function appendUniqueId(ids: string[], id: string): string[] {
	if (!id || ids.includes(id)) {
		return ids;
	}

	return [...ids, id];
}

function buildForkOverrides(
	state: SharedSessionState
): Pick<ProtocolTurnStartParams, 'cwd' | 'model'> | undefined {
	const model = trimOptionalText(state.turnOverrideModel);
	const cwd = trimOptionalText(state.turnOverrideCwd);

	if (!model && !cwd) {
		return undefined;
	}

	return {
		...(model ? { model } : {}),
		...(cwd ? { cwd } : {})
	};
}

function trimOptionalText(value: string) {
	const trimmed = value.trim();
	return trimmed ? trimmed : undefined;
}

function normalizeStoredPersonality(
	value: string | null
): '' | ProtocolPersonality | null {
	if (!value) {
		return '';
	}

	if (value === 'none' || value === 'friendly' || value === 'pragmatic') {
		return value;
	}

	return null;
}

function persistValue(key: string, value: string) {
	if (typeof window === 'undefined') {
		return;
	}

	window.localStorage.setItem(key, value);
}

function persistOptionalValue(key: string, value: string | null) {
	if (typeof window === 'undefined') {
		return;
	}

	if (!value) {
		window.localStorage.removeItem(key);
		return;
	}

	window.localStorage.setItem(key, value);
}

function upsertThread(threads: ProtocolThread[], nextThread: ProtocolThread) {
	const existingIndex = threads.findIndex((thread) => thread.id === nextThread.id);
	if (existingIndex === -1) {
		return [nextThread, ...threads];
	}

	return [
		nextThread,
		...threads.filter((thread, index) => index !== existingIndex)
	];
}

function mergeTurn(thread: ProtocolThread, nextTurn: ProtocolTurn) {
	const existingIndex = thread.turns.findIndex((turn) => turn.id === nextTurn.id);
	const turns =
		existingIndex === -1
			? [...thread.turns, nextTurn]
			: thread.turns.map((turn, index) =>
					index === existingIndex ? { ...turn, ...nextTurn } : turn
				);

	return {
		...thread,
		turns
	};
}

function seedTurnWithPrompt(turn: ProtocolTurn, promptText: string): ProtocolTurn {
	if (turn.items.length || !promptText.trim()) {
		return turn;
	}

	return {
		...turn,
		items: [
			{
				type: 'userMessage',
				id: `${turn.id}_seeded_user`,
				content: [{ type: 'text', text: promptText, text_elements: [] }]
			}
		]
	};
}

function mapServerRequest(request: JsonRpcServerRequest): SharedSessionStructuredRequest[] {
	if (request.method === 'item/commandExecution/requestApproval') {
		const params = request.params as ProtocolCommandExecutionRequestApprovalParams;
		return [
			{
				kind: 'command-approval',
				id: `approval_${String(request.id)}`,
				requestId: request.id,
				threadId: params.threadId,
				turnId: params.turnId,
				itemId: params.itemId,
				status: 'pending',
				label: 'Command approval',
				detail: [params.reason, params.command, params.cwd ? `cwd: ${params.cwd}` : null]
					.filter(Boolean)
					.join(' · '),
				primary: 'Run command',
				secondary: 'Decline',
				options: buildCommandApprovalOptions(params)
			}
		];
	}

	if (request.method === 'item/fileChange/requestApproval') {
		const params = request.params as ProtocolFileChangeRequestApprovalParams;
		return [
			{
				kind: 'file-approval',
				id: `approval_${String(request.id)}`,
				requestId: request.id,
				threadId: params.threadId,
				turnId: params.turnId,
				itemId: params.itemId,
				status: 'pending',
				label: 'Patch approval',
				detail: [params.reason, params.grantRoot ? `grant root: ${params.grantRoot}` : null]
					.filter(Boolean)
					.join(' · '),
				primary: 'Apply patch',
				secondary: 'Decline',
				options: buildFileApprovalOptions()
			}
		];
	}

	if (request.method === 'item/tool/requestUserInput') {
		const params = request.params as ProtocolToolRequestUserInputParams;
		return params.questions.map((question) => ({
			kind: 'question',
			id: `question_${String(request.id)}_${question.id}`,
			requestId: request.id,
			threadId: params.threadId,
			turnId: params.turnId,
			itemId: params.itemId,
			questionId: question.id,
			status: 'pending',
			header: question.header,
			prompt: question.question,
			options:
				question.options?.map((option) => ({
					label: option.label,
					description: option.description
				})) ?? [],
			allowOther: question.isOther,
			allowSecret: question.isSecret
		}));
	}

	return [];
}

function mergeStructuredRequests(
	existing: SharedSessionStructuredRequest[],
	incoming: SharedSessionStructuredRequest[]
) {
	const incomingIds = new Set(incoming.map((request) => request.id));
	return [...incoming, ...existing.filter((request) => !incomingIds.has(request.id))].slice(0, 40);
}

function summarizeServerRequest(requests: SharedSessionStructuredRequest[]) {
	const first = requests[0];
	if (!first) {
		return 'Structured request received.';
	}

	if (first.kind === 'question') {
		return `Input requested${requests.length > 1 ? ` (${requests.length} questions)` : ''}.`;
	}

	return `${first.label} requested.`;
}

function formatApprovalResolution(
	decision: ProtocolCommandExecutionApprovalDecision | ProtocolFileChangeApprovalDecision
) {
	if (typeof decision === 'string') {
		switch (decision) {
			case 'accept':
				return 'Accepted';
			case 'acceptForSession':
				return 'Accepted for session';
			case 'decline':
				return 'Declined';
			case 'cancel':
				return 'Canceled';
		}
	}

	return 'Accepted with policy amendment';
}

function buildFileApprovalOptions(): SharedSessionApprovalRequest['options'] {
	return [
		{
			id: 'accept',
			label: 'Apply patch',
			description: 'Approve this patch once.',
			decision: 'accept'
		},
		{
			id: 'acceptForSession',
			label: 'Allow for session',
			description: 'Approve similar file changes for the rest of this session.',
			decision: 'acceptForSession'
		},
		{
			id: 'decline',
			label: 'Decline',
			description: 'Reject the requested file change.',
			decision: 'decline'
		},
		{
			id: 'cancel',
			label: 'Cancel',
			description: 'Leave the request unresolved and cancel the action.',
			decision: 'cancel'
		}
	];
}

function buildCommandApprovalOptions(
	params: ProtocolCommandExecutionRequestApprovalParams
): SharedSessionApprovalRequest['options'] {
	const options: SharedSessionApprovalRequest['options'] = [
		{
			id: 'accept',
			label: 'Run command',
			description: 'Approve this command once.',
			decision: 'accept'
		},
		{
			id: 'acceptForSession',
			label: 'Allow for session',
			description: 'Approve similar commands for the rest of this session.',
			decision: 'acceptForSession'
		}
	];

	if (params.proposedExecpolicyAmendment?.length) {
		options.push({
			id: 'acceptWithExecpolicyAmendment',
			label: 'Approve with rule',
			description: `Accept and add ${params.proposedExecpolicyAmendment.length} exec-policy rule${params.proposedExecpolicyAmendment.length === 1 ? '' : 's'}.`,
			decision: {
				acceptWithExecpolicyAmendment: {
					execpolicy_amendment: params.proposedExecpolicyAmendment
				}
			}
		});
	}

	options.push(
		{
			id: 'decline',
			label: 'Decline',
			description: 'Reject the requested command.',
			decision: 'decline'
		},
		{
			id: 'cancel',
			label: 'Cancel',
			description: 'Leave the request unresolved and cancel the action.',
			decision: 'cancel'
		}
	);

	return options;
}

function summarizeNotification(method: string, params: Record<string, unknown> | null) {
	const turnId = readString(params?.turn as Record<string, unknown> | undefined, 'id');
	const turnStatus = readString(params?.turn as Record<string, unknown> | undefined, 'status');

	if (method === 'turn/started') {
		return turnId ? `Turn ${turnId} started.` : 'A turn started.';
	}

	if (method === 'turn/completed') {
		if (turnId && turnStatus) {
			return `Turn ${turnId} ${turnStatus}.`;
		}

		return 'A turn completed.';
	}

	if (method.startsWith('codex/event/')) {
		const kind = method.replace('codex/event/', '');
		return kind;
	}

	return params ? JSON.stringify(params) : 'Notification received.';
}

function summarizeTurnNotification(method: string, threadId: string, turn: ProtocolTurn) {
	if (method === 'turn/started') {
		return `Live updates attached to ${threadId}.`;
	}

	if (turn.status === 'interrupted') {
		return `Stopped turn ${turn.id}.`;
	}

	if (turn.status === 'completed') {
		return `Turn ${turn.id} completed.`;
	}

	if (turn.status === 'failed') {
		return `Turn ${turn.id} failed.`;
	}

	return `Turn ${turn.id} updated.`;
}

function readTurn(value: unknown): ProtocolTurn | null {
	if (
		!isObject(value) ||
		typeof value.id !== 'string' ||
		!Array.isArray(value.items) ||
		typeof value.status !== 'string'
	) {
		return null;
	}

	return value as ProtocolTurn;
}

function readString(source: Record<string, unknown> | null | undefined, key: string) {
	const value = source?.[key];
	return typeof value === 'string' ? value : null;
}

function isObject(value: unknown): value is Record<string, unknown> {
	return typeof value === 'object' && value !== null;
}

function capitalize(value: string) {
	return value.charAt(0).toUpperCase() + value.slice(1);
}

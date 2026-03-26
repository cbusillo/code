import type {
	ConversationListenerResult,
	InitializeResult,
	JsonRpcFailure,
	JsonRpcNotification,
	JsonRpcRequestId,
	JsonRpcServerRequest,
	JsonRpcSuccess,
	ProtocolThread,
	ProtocolTurnStartParams,
	ThreadForkResult,
	ThreadLoadedListResult,
	ThreadListResult,
	ThreadReadResult,
	ThreadResumeResult,
	ThreadStartResult,
	TurnInterruptResult,
	TurnSteerResult,
	TurnStartResult
} from '$lib/shared-session-types';

type PendingRequest = {
	resolve: (value: unknown) => void;
	reject: (reason?: unknown) => void;
};

type NotificationListener = (notification: JsonRpcNotification) => void;
type ServerRequestListener = (request: JsonRpcServerRequest) => void;

export class SharedSessionClient {
	#socket: WebSocket | null = null;
	#nextId = 1;
	#pendingRequests = new Map<number, PendingRequest>();
	#listeners = new Set<NotificationListener>();
	#serverRequestListeners = new Set<ServerRequestListener>();
	#openPromise: Promise<void> | null = null;
	#onClose: ((event: CloseEvent) => void) | null = null;

	async connect(url: string, onClose?: (event: CloseEvent) => void) {
		if (this.#socket && this.#socket.readyState === WebSocket.OPEN) {
			return;
		}

		this.#onClose = onClose ?? null;
		this.#socket = new WebSocket(url);

		this.#openPromise = new Promise<void>((resolve, reject) => {
			if (!this.#socket) {
				reject(new Error('WebSocket was not created'));
				return;
			}

			this.#socket.onopen = () => resolve();
			this.#socket.onerror = () => reject(new Error(`Failed to connect to ${url}`));
			this.#socket.onmessage = (event) => this.#handleMessage(event.data);
			this.#socket.onclose = (event) => {
				this.#rejectAllPending(new Error(`Socket closed (${event.code})`));
				this.#socket = null;
				this.#openPromise = null;
				this.#onClose?.(event);
			};
		});

		await this.#openPromise;
		await this.initialize();
	}

	close() {
		this.#socket?.close();
		this.#socket = null;
		this.#openPromise = null;
	}

	onNotification(listener: NotificationListener) {
		this.#listeners.add(listener);

		return () => {
			this.#listeners.delete(listener);
		};
	}

	onServerRequest(listener: ServerRequestListener) {
		this.#serverRequestListeners.add(listener);

		return () => {
			this.#serverRequestListeners.delete(listener);
		};
	}

	async initialize() {
		return this.request<InitializeResult>('initialize', {
			clientInfo: {
				name: 'every-code-webui',
				title: 'EveryCode WebUI',
				version: '0.0.1'
			},
			capabilities: {
				experimentalApi: true
			}
		});
	}

	async listThreads(cwd?: string) {
		const params = cwd ? { cwd } : {};
		return this.request<ThreadListResult>('thread/list', params);
	}

	async listLoadedThreads() {
		return this.request<ThreadLoadedListResult>('thread/loaded/list', {});
	}

	async readThread(threadId: string) {
		return this.request<ThreadReadResult>('thread/read', {
			threadId,
			includeTurns: true
		});
	}

	async resumeThread(threadId: string) {
		return this.request<ThreadResumeResult>('thread/resume', { threadId });
	}

	async forkThread(
		threadId: string,
		overrides?: Pick<ProtocolTurnStartParams, 'cwd' | 'model'>
	) {
		return this.request<ThreadForkResult>('thread/fork', {
			threadId,
			...(overrides?.cwd ? { cwd: overrides.cwd } : {}),
			...(overrides?.model ? { model: overrides.model } : {})
		});
	}

	async startThread(cwd?: string) {
		return this.request<ThreadStartResult>('thread/start', {
			cwd,
			experimentalRawEvents: false
		});
	}

	async addConversationListener(threadId: string) {
		return this.request<ConversationListenerResult>('addConversationListener', {
			conversationId: threadId,
			experimentalRawEvents: false
		});
	}

	async removeConversationListener(subscriptionId: string) {
		return this.request('removeConversationListener', { subscriptionId });
	}

	async startTurn(
		threadId: string,
		text: string,
		overrides?: Pick<ProtocolTurnStartParams, 'cwd' | 'model' | 'personality'>
	) {
		return this.request<TurnStartResult>('turn/start', {
			threadId,
			input: [
				{
					type: 'text',
					text,
					textElements: []
				}
			],
			...(overrides?.cwd ? { cwd: overrides.cwd } : {}),
			...(overrides?.model ? { model: overrides.model } : {}),
			...(overrides?.personality ? { personality: overrides.personality } : {})
		});
	}

	async interruptTurn(threadId: string, turnId: string) {
		return this.request<TurnInterruptResult>('turn/interrupt', {
			threadId,
			turnId
		});
	}

	async steerTurn(threadId: string, turnId: string, text: string) {
		return this.request<TurnSteerResult>('turn/steer', {
			threadId,
			expectedTurnId: turnId,
			input: [
				{
					type: 'text',
					text,
					textElements: []
				}
			]
		});
	}

	async respond(id: JsonRpcRequestId, result: unknown) {
		await this.#awaitOpenSocket();
		this.#socket?.send(
			JSON.stringify({
				jsonrpc: '2.0' as const,
				id,
				result
			})
		);
	}

	async #awaitOpenSocket() {
		if (!this.#socket || this.#socket.readyState !== WebSocket.OPEN) {
			if (!this.#openPromise) {
				throw new Error('Socket is not connected');
			}

			await this.#openPromise;
		}
	}

	async request<T>(method: string, params?: Record<string, unknown>) {
		await this.#awaitOpenSocket();

		const id = this.#nextId;
		this.#nextId += 1;

		const payload = {
			jsonrpc: '2.0' as const,
			id,
			method,
			...(params ? { params } : {})
		};

		const responsePromise = new Promise<T>((resolve, reject) => {
			this.#pendingRequests.set(id, {
				resolve: (value) => resolve(value as T),
				reject
			});
		});

		this.#socket?.send(JSON.stringify(payload));

		return responsePromise;
	}

	#handleMessage(rawMessage: string) {
		const parsedMessage = JSON.parse(rawMessage) as
			| JsonRpcSuccess<unknown>
			| JsonRpcFailure
			| JsonRpcServerRequest
			| JsonRpcNotification;

		if ('method' in parsedMessage && 'id' in parsedMessage) {
			for (const listener of this.#serverRequestListeners) {
				listener(parsedMessage);
			}

			return;
		}

		if ('method' in parsedMessage) {
			for (const listener of this.#listeners) {
				listener(parsedMessage);
			}

			return;
		}

		const pendingRequest = this.#pendingRequests.get(parsedMessage.id);
		if (!pendingRequest) {
			return;
		}

		this.#pendingRequests.delete(parsedMessage.id);

		if ('error' in parsedMessage) {
			pendingRequest.reject(new Error(parsedMessage.error.message));
			return;
		}

		pendingRequest.resolve(parsedMessage.result);
	}

	#rejectAllPending(error: Error) {
		for (const pendingRequest of this.#pendingRequests.values()) {
			pendingRequest.reject(error);
		}

		this.#pendingRequests.clear();
	}
}

export function normalizeThreads(
	result: ThreadListResult | ThreadReadResult | ThreadResumeResult | ThreadStartResult
) {
	if ('data' in result) {
		return result.data;
	}

	return [result.thread] satisfies ProtocolThread[];
}

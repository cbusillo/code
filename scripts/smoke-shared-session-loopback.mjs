#!/usr/bin/env node

import process from 'node:process';

const targetUrl = process.argv[2] ?? 'ws://127.0.0.1:8877';

let nextRequestId = 1;
const pendingRequests = new Map();

function createRequest(method, params = {}) {
	const id = nextRequestId;
	nextRequestId += 1;
	return {
		id,
		payload: {
			jsonrpc: '2.0',
			id,
			method,
			params
		}
	};
}

function sendRequest(socket, method, params = {}) {
	const { id, payload } = createRequest(method, params);
	return new Promise((resolve, reject) => {
		pendingRequests.set(id, { resolve, reject, method });
		socket.send(JSON.stringify(payload));
	});
}

function rejectPending(error) {
	for (const { reject } of pendingRequests.values()) {
		reject(error);
	}
	pendingRequests.clear();
}

function parseMessage(rawMessage) {
	const text = typeof rawMessage === 'string' ? rawMessage : Buffer.from(rawMessage).toString('utf8');
	return JSON.parse(text);
}

async function main() {
	const socket = new WebSocket(targetUrl);

	await new Promise((resolve, reject) => {
		const timeout = setTimeout(() => {
			reject(new Error(`Timed out connecting to ${targetUrl}`));
		}, 10_000);

		socket.addEventListener('open', () => {
			clearTimeout(timeout);
			resolve();
		});

		socket.addEventListener('error', (event) => {
			clearTimeout(timeout);
			reject(event.error ?? new Error(`Failed to connect to ${targetUrl}`));
		});
	});

	socket.addEventListener('message', (event) => {
		try {
			const message = parseMessage(event.data);
			if (typeof message.id === 'number' && pendingRequests.has(message.id)) {
				const pending = pendingRequests.get(message.id);
				pendingRequests.delete(message.id);
				if (message.error) {
					pending.reject(
						new Error(`${pending.method} failed: ${JSON.stringify(message.error)}`)
					);
					return;
				}
				pending.resolve(message.result);
			}
		} catch (error) {
			rejectPending(error instanceof Error ? error : new Error(String(error)));
		}
	});

	socket.addEventListener('close', () => {
		rejectPending(new Error('Socket closed before all requests completed'));
	});

	try {
		const initializeResult = await sendRequest(socket, 'initialize', {
			clientInfo: {
				name: 'shared-session-loopback-smoke',
				version: '0.1.0'
			},
			capabilities: {
				experimentalApi: true
			}
		});

		const threadListResult = await sendRequest(socket, 'thread/list');
		const threadStartResult = await sendRequest(socket, 'thread/start');
		const loadedThreadsResult = await sendRequest(socket, 'thread/loaded/list');

		const thread = threadStartResult?.thread;
		const loadedThreadIds = Array.isArray(loadedThreadsResult?.data)
			? loadedThreadsResult.data.filter((value) => typeof value === 'string')
			: [];

		if (!Array.isArray(threadListResult?.data)) {
			throw new Error('thread/list did not return a data array');
		}

		if (!thread?.id || typeof thread.id !== 'string') {
			throw new Error('thread/start did not return a thread id');
		}

		if (typeof threadStartResult?.model !== 'string' || !threadStartResult.model) {
			throw new Error('thread/start did not return a model');
		}

		if (!loadedThreadIds.includes(thread.id)) {
			throw new Error('thread/loaded/list did not include the started thread');
		}

		console.log(`Connected to ${targetUrl}`);
		console.log(`Initialize result received: ${initializeResult ? 'yes' : 'no'}`);
		console.log(`Persisted threads listed: ${threadListResult.data.length}`);
		console.log(`Started thread: ${thread.id}`);
		console.log(`Loaded thread ids now include started thread: yes`);
		console.log('Shared-session loopback smoke passed.');
	} finally {
		if (socket.readyState === WebSocket.OPEN || socket.readyState === WebSocket.CONNECTING) {
			socket.close();
		}
	}
}

main().catch((error) => {
	console.error(error instanceof Error ? error.message : String(error));
	process.exitCode = 1;
});

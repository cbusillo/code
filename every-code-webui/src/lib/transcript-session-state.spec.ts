import { describe, expect, it } from 'vitest';

import { deriveTranscriptSessionBanner } from '$lib/transcript-session-state';
import type { SharedSessionApprovalRequest } from '$lib/shared-session-domain';
import type { ProtocolThread } from '$lib/shared-session-types';

function makeThread(overrides: Partial<ProtocolThread> = {}): ProtocolThread {
	return {
		id: 'thread-1',
		preview: 'Thread preview',
		createdAt: 1,
		updatedAt: 1,
		path: null,
		cwd: '/workspace',
		cliVersion: '0.0.0',
		modelProvider: 'openai',
		source: 'cli',
		gitInfo: null,
		turns: [],
		...overrides
	};
}

function pendingApproval(threadId = 'thread-1'): SharedSessionApprovalRequest {
	return {
		kind: 'command-approval',
		id: 'approval-1',
		requestId: 1,
		threadId,
		turnId: 'turn-1',
		itemId: 'item-1',
		status: 'pending',
		label: 'Command approval',
		detail: 'Approve command',
		primary: 'Allow',
		secondary: 'Deny',
		options: []
	};
}

describe('deriveTranscriptSessionBanner', () => {
	it('returns null when no thread is selected', () => {
		expect(
			deriveTranscriptSessionBanner({
				connectionPhase: 'connected',
				statusMessage: 'Connected',
				selectedThread: null,
				activeThreadId: null,
				pendingAction: null,
				structuredRequests: []
			})
		).toBeNull();
	});

	it('marks disconnected views as read only', () => {
		const banner = deriveTranscriptSessionBanner({
			connectionPhase: 'offline',
			statusMessage: 'Socket closed.',
			selectedThread: makeThread(),
			activeThreadId: null,
			pendingAction: null,
			structuredRequests: []
		});

		expect(banner?.state).toBe('read_only');
		expect(banner?.primaryAction).toBe('connect');
		expect(banner?.primaryLabel).toBe('Connect and restore');
	});

	it('marks locally running turns as blocked_running', () => {
		const banner = deriveTranscriptSessionBanner({
			connectionPhase: 'connected',
			statusMessage: 'Connected',
			selectedThread: makeThread({
				turns: [{ id: 'turn-1', status: 'inProgress', items: [], error: null }]
			}),
			activeThreadId: 'thread-1',
			pendingAction: null,
			structuredRequests: []
		});

		expect(banner?.state).toBe('blocked_running');
	});

	it('marks remotely running turns as active_elsewhere', () => {
		const banner = deriveTranscriptSessionBanner({
			connectionPhase: 'connected',
			statusMessage: 'Connected',
			selectedThread: makeThread({
				turns: [{ id: 'turn-1', status: 'inProgress', items: [], error: null }]
			}),
			activeThreadId: null,
			pendingAction: null,
			structuredRequests: []
		});

		expect(banner?.state).toBe('active_elsewhere');
		expect(banner?.primaryAction).toBe('take_over');
		expect(banner?.primaryLabel).toBe('Bring it here');
	});

	it('keeps remote running turns explicit when takeover is queued here', () => {
		const banner = deriveTranscriptSessionBanner({
			connectionPhase: 'connected',
			statusMessage: 'Connected',
			selectedThread: makeThread({
				id: 'thread_queued',
				turns: [{ id: 'turn-queued', status: 'inProgress', items: [], error: null }]
			}),
			activeThreadId: null,
			takeoverRequestedThreadId: 'thread_queued',
			pendingAction: null,
			structuredRequests: []
		});

		expect(banner?.state).toBe('active_elsewhere');
		expect(banner?.title).toBe('This browser is next in line.');
		expect(banner?.primaryLabel).toBe('Queued here');
	});

	it('marks pending structured requests as respond_remotely when not active locally', () => {
		const banner = deriveTranscriptSessionBanner({
			connectionPhase: 'connected',
			statusMessage: 'Connected',
			selectedThread: makeThread(),
			activeThreadId: null,
			pendingAction: null,
			structuredRequests: [pendingApproval()]
		});

		expect(banner?.state).toBe('respond_remotely');
		expect(banner?.primaryAction).toBeUndefined();
	});

	it('marks attached settled threads as active_here', () => {
		const banner = deriveTranscriptSessionBanner({
			connectionPhase: 'connected',
			statusMessage: 'Connected',
			selectedThread: makeThread(),
			activeThreadId: 'thread-1',
			pendingAction: null,
			structuredRequests: []
		});

		expect(banner?.state).toBe('active_here');
	});

	it('marks idle loaded threads on another surface as active_elsewhere', () => {
		const banner = deriveTranscriptSessionBanner({
			connectionPhase: 'connected',
			statusMessage: 'Connected',
			selectedThread: makeThread(),
			activeThreadId: null,
			loadedElsewhere: true,
			pendingAction: null,
			structuredRequests: []
		});

		expect(banner?.state).toBe('active_elsewhere');
		expect(banner?.primaryAction).toBe('take_over');
		expect(banner?.title).toBe('This thread is open somewhere else.');
	});

	it('marks settled unattached threads as can_continue_here', () => {
		const banner = deriveTranscriptSessionBanner({
			connectionPhase: 'connected',
			statusMessage: 'Connected',
			selectedThread: makeThread(),
			activeThreadId: null,
			pendingAction: null,
			structuredRequests: []
		});

		expect(banner?.state).toBe('can_continue_here');
		expect(banner?.primaryLabel).toBe('Attach live updates');
	});
});

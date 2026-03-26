import type {
	TranscriptSessionBanner,
	TranscriptSessionState
} from '$lib/transcript-model';
import type {
	ConnectionPhase,
	SharedSessionStructuredRequest
} from '$lib/shared-session-domain';
import type { ProtocolThread } from '$lib/shared-session-types';

type DeriveTranscriptSessionStateParams = {
	connectionPhase: ConnectionPhase;
	statusMessage: string;
	selectedThread: ProtocolThread | null;
	activeThreadId: string | null;
	loadedElsewhere?: boolean;
	takeoverRequestedThreadId?: string | null;
	pendingAction: string | null;
	structuredRequests: SharedSessionStructuredRequest[];
};

export function deriveTranscriptSessionBanner(
	params: DeriveTranscriptSessionStateParams
): TranscriptSessionBanner | null {
	const {
		connectionPhase,
		statusMessage,
		selectedThread,
		activeThreadId,
		loadedElsewhere = false,
		takeoverRequestedThreadId = null,
		pendingAction,
		structuredRequests
	} =
		params;

	if (!selectedThread) {
		return null;
	}

	const hasPendingStructuredRequests = structuredRequests.some((request) => request.status === 'pending');
	const hasRunningTurn = selectedThread.turns.some((turn) => turn.status === 'inProgress');

	if (connectionPhase !== 'connected') {
		return {
			state: 'read_only',
			title: 'Viewing saved context only.',
			detail:
				takeoverRequestedThreadId === selectedThread.id
					? connectionPhase === 'connecting'
						? `The browser is reconnecting so it can restore ${selectedThread.preview || selectedThread.id} and keep the queued takeover ready.`
						: `${statusMessage} Reconnect before the queued takeover can finish for ${selectedThread.preview || selectedThread.id}.`
					: connectionPhase === 'connecting'
						? `The browser is reconnecting so it can restore ${selectedThread.preview || selectedThread.id}.`
						: `${statusMessage} Reconnect before sending prompts or restoring live control for ${selectedThread.preview || selectedThread.id}.`,
			primaryAction: 'connect',
			primaryLabel: connectionPhase === 'connecting' ? 'Reconnect and restore' : 'Connect and restore'
		};
	}

	if (pendingAction === 'sending prompt' || (hasRunningTurn && activeThreadId === selectedThread.id)) {
		return {
			state: 'blocked_running',
			title: 'Turn running from this browser.',
			detail:
				'Live output may still be arriving. Keep reading here, but wait for the turn to settle before sending another prompt.'
		};
	}

	if (activeThreadId === selectedThread.id) {
		return {
			state: 'active_here',
			title: 'This browser is actively attached.',
			detail: hasPendingStructuredRequests
				? 'Pending approvals and questions can be answered from this transcript without leaving the thread.'
				: 'Live updates are attached here, and this browser can continue the thread directly.'
		};
	}

	if (loadedElsewhere) {
		return {
			state: 'active_elsewhere',
			title: 'Another surface is attached to this thread.',
			detail:
				hasPendingStructuredRequests
					? 'You can still answer structured requests here, but take over first before steering or sending a new prompt from this browser.'
					: 'Live control is still attached elsewhere. Take over here before sending the next prompt from this browser.',
			primaryAction: 'take_over',
			primaryLabel: 'Take over here'
		};
	}

	if (hasRunningTurn) {
		return {
			state: 'active_elsewhere',
			title:
				takeoverRequestedThreadId === selectedThread.id
					? 'Takeover queued for this browser.'
					: 'Another surface is driving this turn.',
			detail:
				takeoverRequestedThreadId === selectedThread.id
					? 'Keep reading and answer structured requests here. Live updates will attach automatically when the active turn settles.'
					: 'You can keep reading and answer structured requests here, but wait for the turn to settle unless you intentionally want this browser to take over.',
			primaryAction: 'take_over',
			primaryLabel:
				takeoverRequestedThreadId === selectedThread.id ? 'Takeover queued' : 'Take over here'
		};
	}

	if (hasPendingStructuredRequests) {
		return {
			state: 'respond_remotely',
			title: 'Respond remotely without taking over.',
			detail:
				'Structured approvals or questions are pending on this thread. You can respond here first, then attach live updates only when you want to steer again.'
		};
	}

	return {
		state: 'can_continue_here',
		title: 'Ready to continue here.',
		detail: 'Resume live updates or send the next prompt from this browser when you are ready.',
		primaryAction: 'resume',
		primaryLabel: 'Attach live updates'
	};
}

export function transcriptSessionTone(state: TranscriptSessionState) {
	switch (state) {
		case 'active_here':
			return 'border-teal-200/60 bg-teal-100/70 text-teal-950';
		case 'blocked_running':
			return 'border-sky-200/70 bg-sky-100/80 text-sky-950';
		case 'respond_remotely':
			return 'border-amber-200/80 bg-amber-100/80 text-amber-950';
		case 'active_elsewhere':
			return 'border-violet-200/80 bg-violet-100/80 text-violet-950';
		case 'can_continue_here':
			return 'border-stone-900/12 bg-white/75 text-stone-900';
		case 'read_only':
		default:
			return 'border-stone-900/10 bg-stone-100/85 text-stone-800';
	}
}

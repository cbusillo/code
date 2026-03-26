import type { ReviewArtifact, ReviewNotesState } from '$lib/transcript-review-artifacts';
import type { ProtocolThread } from '$lib/shared-session-types';

const PHASE3_LIBRARY_STORAGE_KEY = 'every-code-webui.phase3-library';

export type ExternalHandoffSource = 'github' | 'slack' | 'jira' | 'linear' | 'generic';

export type SavedCheckpoint = {
	id: string;
	label: string;
	note: string;
	createdAt: number;
	sourceThreadId: string;
	sourceThreadLabel: string;
	forkedThreadId: string | null;
	cwd: string;
	reviewArtifactCount: number;
	reviewNoteCount: number;
};

export type SavedHandoff = {
	id: string;
	createdAt: number;
	source: ExternalHandoffSource;
	url: string;
	title: string;
	prompt: string;
	threadId: string | null;
};

export type SavedReviewExport = {
	id: string;
	createdAt: number;
	threadId: string;
	threadLabel: string;
	artifactCount: number;
	reviewNoteCount: number;
	byteLength: number;
};

export type Phase3Library = {
	checkpoints: SavedCheckpoint[];
	handoffs: SavedHandoff[];
	reviewExports: SavedReviewExport[];
};

export const EMPTY_PHASE3_LIBRARY: Phase3Library = {
	checkpoints: [],
	handoffs: [],
	reviewExports: []
};

export function loadPhase3Library(storage: Storage): Phase3Library {
	const rawValue = storage.getItem(PHASE3_LIBRARY_STORAGE_KEY);
	if (!rawValue) {
		return EMPTY_PHASE3_LIBRARY;
	}

	try {
		const parsed = JSON.parse(rawValue) as Partial<Phase3Library>;
		return {
			checkpoints: Array.isArray(parsed.checkpoints) ? parsed.checkpoints : [],
			handoffs: Array.isArray(parsed.handoffs) ? parsed.handoffs : [],
			reviewExports: Array.isArray(parsed.reviewExports) ? parsed.reviewExports : []
		};
	} catch {
		return EMPTY_PHASE3_LIBRARY;
	}
}

export function persistPhase3Library(storage: Storage, library: Phase3Library) {
	storage.setItem(PHASE3_LIBRARY_STORAGE_KEY, JSON.stringify(library));
}

export function appendCheckpoint(
	library: Phase3Library,
	checkpoint: SavedCheckpoint
): Phase3Library {
	return {
		...library,
		checkpoints: [checkpoint, ...library.checkpoints].slice(0, 20)
	};
}

export function appendHandoff(library: Phase3Library, handoff: SavedHandoff): Phase3Library {
	return {
		...library,
		handoffs: [handoff, ...library.handoffs].slice(0, 20)
	};
}

export function appendReviewExport(
	library: Phase3Library,
	reviewExport: SavedReviewExport
): Phase3Library {
	return {
		...library,
		reviewExports: [reviewExport, ...library.reviewExports].slice(0, 20)
	};
}

export function detectExternalHandoffSource(urlValue: string): ExternalHandoffSource {
	try {
		const url = new URL(urlValue);
		const hostname = url.hostname.toLowerCase();
		if (hostname.includes('github.com')) {
			return 'github';
		}
		if (hostname.includes('slack.com')) {
			return 'slack';
		}
		if (hostname.includes('atlassian.net') || hostname.includes('jira')) {
			return 'jira';
		}
		if (hostname.includes('linear.app')) {
			return 'linear';
		}
	} catch {
		return 'generic';
	}

	return 'generic';
}

export function buildExternalHandoffPrompt(
	source: ExternalHandoffSource,
	url: string,
	threadLabel: string
) {
	const sourceLabel =
		source === 'generic' ? 'external update' : `${source[0]?.toUpperCase() ?? ''}${source.slice(1)} update`;
	return [
		`Use this ${sourceLabel} as the next checkpoint for ${threadLabel}.`,
		`Reference: ${url}`,
		'Restate the ask, call out blockers or approvals that matter, and continue the same companion thread without redoing already finished work.'
	].join('\n');
}

export function buildReviewExportPayload(
	thread: ProtocolThread,
	artifacts: ReviewArtifact[],
	reviewNotes: ReviewNotesState,
	checkpoints: SavedCheckpoint[]
) {
	return {
		thread: {
			id: thread.id,
			label: thread.preview || thread.id,
			cwd: thread.cwd,
			updatedAt: thread.updatedAt,
			turnCount: thread.turns.length
		},
		artifacts: artifacts.map((artifact) => ({
			id: artifact.id,
			kind: artifact.kind,
			title: artifact.title,
			summary: artifact.summary,
			meta: artifact.meta,
			note: reviewNotes[artifact.id] ?? ''
		})),
		checkpoints: checkpoints
			.filter((checkpoint) => checkpoint.sourceThreadId === thread.id)
			.map((checkpoint) => ({
				id: checkpoint.id,
				label: checkpoint.label,
				note: checkpoint.note,
				forkedThreadId: checkpoint.forkedThreadId,
				createdAt: checkpoint.createdAt
			}))
	};
}


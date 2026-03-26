import { summarizeUnifiedDiff } from '$lib/diff-review';
import type { AssistantMessageCell, TranscriptCell } from '$lib/transcript-model';

const REVIEW_NOTES_STORAGE_KEY = 'every-code-webui.review-notes';

export type ReviewArtifact = {
	id: string;
	cellId: string;
	kind: 'diff' | 'code' | 'tool' | 'error';
	title: string;
	summary: string;
	meta: string;
	copyText?: string;
	copyLabel?: string;
	severity: 'info' | 'review' | 'attention';
};

export type ReviewNotesState = Record<string, string>;

export function buildTranscriptReviewArtifacts(cells: TranscriptCell[]): ReviewArtifact[] {
	const artifacts: ReviewArtifact[] = [];

	for (const cell of cells) {
		if (cell.kind === 'diff') {
			const summary = summarizeUnifiedDiff(cell.patch, cell.previewFile);
			artifacts.push({
				id: cell.id,
				cellId: cell.id,
				kind: 'diff',
				title: cell.summary,
				summary: `${summary.files.length || 1} file${summary.files.length === 1 ? '' : 's'} · +${summary.totalAdditions} -${summary.totalDeletions}`,
				meta: cell.previewFile,
				copyText: cell.patch,
				copyLabel: 'Copy patch',
				severity: 'review'
			});
			continue;
		}

		if (cell.kind === 'assistant_message') {
			artifacts.push(...assistantCodeArtifacts(cell));
			continue;
		}

		if (cell.kind === 'tool_call' && cell.output.length) {
			artifacts.push({
				id: cell.id,
				cellId: cell.id,
				kind: 'tool',
				title: cell.command,
				summary: cell.preview,
				meta: `${cell.label} · ${cell.exitSummary}`,
				copyText: cell.output.join('\n'),
				copyLabel: 'Copy output',
				severity: cell.state === 'failed' ? 'attention' : 'info'
			});
			continue;
		}

		if (cell.kind === 'error') {
			artifacts.push({
				id: cell.id,
				cellId: cell.id,
				kind: 'error',
				title: cell.message,
				summary: cell.detail,
				meta: `${cell.status} · ${cell.timestamp}`,
				copyText: `${cell.message}\n\n${cell.detail}`,
				copyLabel: 'Copy error',
				severity: 'attention'
			});
		}
	}

	return artifacts;
}

export function loadReviewNotes(storage: Storage, threadId: string): ReviewNotesState {
	const raw = storage.getItem(REVIEW_NOTES_STORAGE_KEY);
	if (!raw) {
		return {};
	}

	try {
		const parsed = JSON.parse(raw);
		if (!isObject(parsed)) {
			return {};
		}

		const threadNotes = parsed[threadId];
		return normalizeReviewNotes(threadNotes);
	} catch {
		return {};
	}
}

export function persistReviewNotes(
	storage: Storage,
	threadId: string,
	notes: ReviewNotesState
) {
	const raw = storage.getItem(REVIEW_NOTES_STORAGE_KEY);
	const parsed = raw ? safeParse(raw) : {};
	const next = isObject(parsed) ? parsed : {};
	next[threadId] = normalizeReviewNotes(notes);
	storage.setItem(REVIEW_NOTES_STORAGE_KEY, JSON.stringify(next));
}

export function pruneReviewNotes(
	notes: ReviewNotesState,
	artifacts: ReviewArtifact[]
): ReviewNotesState {
	const artifactIds = new Set(artifacts.map((artifact) => artifact.id));
	return Object.fromEntries(
		Object.entries(notes).filter(([artifactId, note]) => artifactIds.has(artifactId) && note.trim())
	);
}

function assistantCodeArtifacts(cell: AssistantMessageCell): ReviewArtifact[] {
	return cell.blocks.flatMap((block, blockIndex) => {
		if (block.type !== 'code') {
			return [];
		}

		const lineCount = block.text ? block.text.split('\n').length : 0;
		return [
			{
				id: `${cell.id}:code:${blockIndex}`,
				cellId: cell.id,
				kind: 'code' as const,
				title: `${cell.title} · code block ${blockIndex + 1}`,
				summary: `${block.language || 'text'} snippet with ${lineCount} line${lineCount === 1 ? '' : 's'}`,
				meta: cell.timestamp,
				copyText: block.text,
				copyLabel: 'Copy code',
				severity: 'review' as const
			}
		];
	});
}

function normalizeReviewNotes(value: unknown): ReviewNotesState {
	if (!isObject(value)) {
		return {};
	}

	return Object.fromEntries(
		Object.entries(value).filter((entry): entry is [string, string] => typeof entry[1] === 'string')
	);
}

function safeParse(raw: string): unknown {
	try {
		return JSON.parse(raw);
	} catch {
		return {};
	}
}

function isObject(value: unknown): value is Record<string, unknown> {
	return typeof value === 'object' && value !== null;
}

import type { TranscriptCell } from '$lib/transcript-model';

export type TranscriptLiveBacklog = {
	total: number;
	blockerCount: number;
	diffCount: number;
	liveCount: number;
	messageCount: number;
	errorCount: number;
	firstBlockerCellId: string;
};

const LIVE_SIGNATURE_TAIL_SIZE = 3;

export function summarizeTranscriptLiveBacklog(
	cells: TranscriptCell[],
	lastFollowedCellCount: number
): TranscriptLiveBacklog {
	const unseenCells = cells.slice(clampFollowedCellCount(lastFollowedCellCount, cells.length));
	let blockerCount = 0;
	let diffCount = 0;
	let liveCount = 0;
	let messageCount = 0;
	let errorCount = 0;
	let firstBlockerCellId = '';

	for (const cell of unseenCells) {
		if (!firstBlockerCellId && isBlockerCell(cell)) {
			firstBlockerCellId = cell.id;
		}

		switch (cell.kind) {
			case 'approval':
			case 'question':
				if (cell.status === 'pending') {
					blockerCount += 1;
				}
				break;
			case 'error':
				blockerCount += 1;
				errorCount += 1;
				break;
			case 'diff':
				diffCount += 1;
				break;
			case 'thinking':
				if (cell.state === 'active') {
					liveCount += 1;
				}
				break;
			case 'tool_call':
				if (cell.state === 'running') {
					liveCount += 1;
				}
				break;
			case 'assistant_message':
			case 'user_message':
				messageCount += 1;
				break;
		}
	}

	return {
		total: unseenCells.length,
		blockerCount,
		diffCount,
		liveCount,
		messageCount,
		errorCount,
		firstBlockerCellId
	};
}

export function buildTranscriptLiveSignature(
	cells: TranscriptCell[],
	latestLiveCellId: string
): string {
	const latestLiveCell = latestLiveCellId
		? (cells.find((cell) => cell.id === latestLiveCellId) ?? null)
		: null;

	return JSON.stringify({
		latestLiveCellId,
		latestLiveCell,
		tail: cells.slice(-LIVE_SIGNATURE_TAIL_SIZE)
	});
}

function clampFollowedCellCount(lastFollowedCellCount: number, cellCount: number) {
	return Math.min(Math.max(lastFollowedCellCount, 0), cellCount);
}

function isBlockerCell(cell: TranscriptCell) {
	return (
		(cell.kind === 'approval' && cell.status === 'pending') ||
		(cell.kind === 'question' && cell.status === 'pending') ||
		cell.kind === 'error'
	);
}

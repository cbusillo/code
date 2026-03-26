import { describe, expect, it } from 'vitest';

import {
	selectedSession,
	sessionRail,
	transcriptCells
} from '$lib/transcript-fixture';

describe('transcript fixture', () => {
	it('keeps the selected session present in the session rail', () => {
		expect(sessionRail.some((session) => session.id === selectedSession.id)).toBe(true);
	});

	it('covers the key transcript primitives for the golden fixture', () => {
		const kinds = new Set(transcriptCells.map((cell) => cell.kind));

		expect(kinds).toEqual(
			new Set([
				'user_message',
				'assistant_message',
				'thinking',
				'tool_call',
				'diff',
				'approval',
				'question',
				'error'
			])
		);
	});

	it('includes at least one pinned reasoning block and one active tool call', () => {
		expect(
			transcriptCells.some(
				(cell) => cell.kind === 'thinking' && cell.pinned
			)
		).toBe(true);

		expect(
			transcriptCells.some(
				(cell) => cell.kind === 'tool_call' && cell.state === 'running'
			)
		).toBe(true);
	});

	it('captures multiple diffs and a remotely answered approval', () => {
		expect(transcriptCells.filter((cell) => cell.kind === 'diff')).toHaveLength(2);

		expect(
			transcriptCells.some(
				(cell) =>
					cell.kind === 'approval' &&
					cell.status === 'answered' &&
					cell.respondedBy === 'iPhone 16 Pro'
			)
		).toBe(true);
	});
});

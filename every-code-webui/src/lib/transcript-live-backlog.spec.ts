import { describe, expect, it } from 'vitest';

import {
	buildTranscriptLiveSignature,
	summarizeTranscriptLiveBacklog
} from '$lib/transcript-live-backlog';
import type { TranscriptCell } from '$lib/transcript-model';

describe('transcript-live-backlog', () => {
	it('summarizes unseen cells by live-review priority', () => {
		const backlog = summarizeTranscriptLiveBacklog(makeCells(), 2);

		expect(backlog).toMatchObject({
			total: 4,
			blockerCount: 2,
			diffCount: 1,
			liveCount: 0,
			messageCount: 1,
			errorCount: 1,
			firstBlockerCellId: 'approval_1'
		});
	});

	it('tracks live output changes even when no new cells were appended', () => {
		const baseline = makeCells();
		const updated = makeCells();
		(updated[1] as Extract<TranscriptCell, { kind: 'tool_call' }>).output = [
			'building...',
			'compiled 12 files'
		];

		expect(buildTranscriptLiveSignature(updated, 'tool_1')).not.toEqual(
			buildTranscriptLiveSignature(baseline, 'tool_1')
		);
	});

	it('ignores older transcript churn outside the live tail signature', () => {
		const baseline = makeCells();
		const updated = makeCells();
		(updated[0] as Extract<TranscriptCell, { kind: 'assistant_message' }>).title =
			'Older context changed';

		expect(buildTranscriptLiveSignature(updated, 'tool_1')).toEqual(
			buildTranscriptLiveSignature(baseline, 'tool_1')
		);
	});
});

function makeCells(): TranscriptCell[] {
	return [
		{
			id: 'assistant_1',
			kind: 'assistant_message',
			timestamp: '10:00',
			title: 'Summary',
			blocks: [{ type: 'paragraph', text: 'Initial context' }]
		},
		{
			id: 'tool_1',
			kind: 'tool_call',
			timestamp: '10:01',
			state: 'running',
			pinned: false,
			label: 'Command execution',
			command: 'pnpm build',
			duration: 'live',
			exitSummary: 'running',
			preview: 'building...',
			output: ['building...']
		},
		{
			id: 'diff_1',
			kind: 'diff',
			timestamp: '10:02',
			autoCompact: true,
			summary: 'Workspace file changes',
			stats: '1 file · completed',
			previewFile: 'src/live.ts',
			patch: 'diff --git a/src/live.ts b/src/live.ts'
		},
		{
			id: 'approval_1',
			kind: 'approval',
			timestamp: '10:03',
			status: 'pending',
			label: 'Run tests?',
			detail: 'The agent wants to run tests.',
			primary: 'Approve',
			secondary: 'Deny'
		},
		{
			id: 'assistant_2',
			kind: 'assistant_message',
			timestamp: '10:04',
			title: 'Follow-up',
			blocks: [{ type: 'paragraph', text: 'The command completed.' }]
		},
		{
			id: 'error_1',
			kind: 'error',
			timestamp: '10:05',
			status: 'failed',
			message: 'Build failed',
			detail: 'TypeScript reported a type mismatch.'
		}
	];
}

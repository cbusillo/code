import { describe, expect, it } from 'vitest';

import {
	buildTranscriptReviewArtifacts,
	pruneReviewNotes,
	type ReviewArtifact
} from '$lib/transcript-review-artifacts';
import type { TranscriptCell } from '$lib/transcript-model';

describe('transcript-review-artifacts', () => {
	it('collects diffs, code blocks, tool output, and errors into a review board', () => {
		const artifacts = buildTranscriptReviewArtifacts(makeCells());

		expect(artifacts.map((artifact) => artifact.kind)).toEqual(['code', 'diff', 'tool', 'error']);
		expect(artifacts[0]).toMatchObject({
			id: 'assistant_1:code:1',
			copyLabel: 'Copy code'
		});
		expect(artifacts[1]).toMatchObject({
			kind: 'diff',
			copyLabel: 'Copy patch'
		});
		expect(artifacts[2]).toMatchObject({
			kind: 'tool',
			severity: 'attention'
		});
	});

	it('prunes notes for artifacts that no longer exist or are blank', () => {
		const artifacts = [
			{ id: 'artifact_a' },
			{ id: 'artifact_b' }
		] as ReviewArtifact[];

		expect(
			pruneReviewNotes(
				{ artifact_a: 'keep', artifact_b: '   ', artifact_c: 'drop me' },
				artifacts
			)
		).toEqual({ artifact_a: 'keep' });
	});
});

function makeCells(): TranscriptCell[] {
	return [
		{
			id: 'assistant_1',
			kind: 'assistant_message',
			timestamp: '10:00',
			title: 'Suggested patch',
			blocks: [
				{ type: 'paragraph', text: 'Here is the summary.' },
				{ type: 'code', language: 'ts', text: 'const answer = 42;' }
			]
		},
		{
			id: 'diff_1',
			kind: 'diff',
			timestamp: '10:01',
			autoCompact: true,
			summary: 'Update review flow',
			stats: '+2 -1',
			previewFile: 'src/review.ts',
			patch: 'diff --git a/src/review.ts b/src/review.ts\n@@ -1 +1,2 @@\n-export const oldValue = 1;\n+export const oldValue = 1;\n+export const nextValue = 2;'
		},
		{
			id: 'tool_1',
			kind: 'tool_call',
			timestamp: '10:02',
			state: 'failed',
			pinned: false,
			label: 'Shell',
			command: 'pnpm build',
			duration: '12s',
			exitSummary: 'Exited 1',
			preview: 'Build failed with one TypeScript error.',
			output: ['src/review.ts:10:5 error TS2322']
		},
		{
			id: 'error_1',
			kind: 'error',
			timestamp: '10:03',
			status: 'failed',
			message: 'Thread failed',
			detail: 'The browser lost its connection to the app-server.'
		}
	];
}

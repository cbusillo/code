import { describe, expect, it } from 'vitest';

import {
	highlightCodeBlock,
	highlightLanguageLabel,
	highlightTerminalOutput,
	highlightUnifiedDiff,
	type HighlightToken
} from '$lib/code-highlight';

describe('code-highlight', () => {
	it('highlights common code block tokens without external deps', () => {
		const lines = highlightCodeBlock('ts', 'const total = 3;\nconsole.log("done")');

		expect(lines).toHaveLength(2);
		expect(lineHasToken(lines[0], 'const', 'keyword')).toBe(true);
		expect(lineHasToken(lines[0], '3', 'number')).toBe(true);
		expect(lineHasToken(lines[1], 'Console', 'type')).toBe(false);
		expect(lineHasToken(lines[1], '"done"', 'string')).toBe(true);
	});

	it('tints tool output for commands, paths, flags, and failure text', () => {
		const lines = highlightTerminalOutput([
			'pnpm --dir every-code-webui build',
			'/Users/cbusillo/Developer/code/every-code-webui/src/lib/code-highlight.ts',
			'Error: failed to reconnect'
		]);

		expect(lineHasToken(lines[0], 'pnpm', 'command')).toBe(true);
		expect(lineHasToken(lines[0], '--dir', 'flag')).toBe(true);
		expect(lineHasToken(lines[1], '/Users/cbusillo/Developer/code/every-code-webui/src/lib/code-highlight.ts', 'path')).toBe(true);
		expect(lineHasToken(lines[2], 'Error', 'error')).toBe(true);
		expect(lineHasToken(lines[2], 'failed', 'error')).toBe(true);
	});

	it('carries language-aware highlighting into unified diffs', () => {
		const lines = highlightUnifiedDiff(`diff --git a/src/example.ts b/src/example.ts
--- a/src/example.ts
+++ b/src/example.ts
@@ -1 +1 @@
-const before = 1;
+const after = 2;`);

		expect(lineHasToken(lines[0].tokens, 'b/src/example.ts', 'path')).toBe(true);
		expect(lines[4].kind).toBe('deletion');
		expect(lineHasToken(lines[4].tokens, '-', 'deletion')).toBe(true);
		expect(lineHasToken(lines[5].tokens, '+', 'addition')).toBe(true);
		expect(lineHasToken(lines[5].tokens, 'const', 'keyword')).toBe(true);
		expect(lineHasToken(lines[5].tokens, '2', 'number')).toBe(true);
	});

	it('normalizes language badges for transcript code cards', () => {
		expect(highlightLanguageLabel('tsx')).toBe('TS / JS');
		expect(highlightLanguageLabel('')).toBe('Text');
	});
});

function lineHasToken(tokens: HighlightToken[], text: string, tone: HighlightToken['tone']) {
	return tokens.some((token) => token.tone === tone && token.text.includes(text));
}

import { classifyDiffLine, type DiffLineKind } from '$lib/diff-review';

export type HighlightTone =
	| 'plain'
	| 'comment'
	| 'keyword'
	| 'string'
	| 'number'
	| 'type'
	| 'property'
	| 'command'
	| 'flag'
	| 'path'
	| 'url'
	| 'meta'
	| 'warning'
	| 'error'
	| 'success'
	| 'addition'
	| 'deletion';

export type HighlightToken = {
	text: string;
	tone: HighlightTone;
};

export type HighlightedDiffLine = {
	kind: DiffLineKind;
	tokens: HighlightToken[];
};

type HighlightPattern = {
	tone: HighlightTone;
	regex: RegExp;
};

const BACKTICK = '`';
const URL_PATTERN = rawPattern(String.raw`https?://[^\s"'${BACKTICK}]+`);
const PATH_PATTERN = rawPattern(String.raw`(?:~/|/|\.{1,2}/)[^\s"'${BACKTICK}]+`);
const STRING_PATTERN = rawPattern(
	String.raw`'(?:\\.|[^'\\])*'|"(?:\\.|[^"\\])*"|${BACKTICK}(?:\\.|[^${BACKTICK}\\])*${BACKTICK}`
);
const NUMBER_PATTERN = rawPattern(String.raw`\b\d+(?:\.\d+)?(?:ms|s|m|h|x|%)?\b`);
const JS_LIKE_KEYWORDS = [
	'async',
	'await',
	'break',
	'case',
	'catch',
	'class',
	'const',
	'continue',
	'default',
	'else',
	'enum',
	'export',
	'extends',
	'false',
	'finally',
	'for',
	'from',
	'function',
	'if',
	'import',
	'in',
	'interface',
	'let',
	'new',
	'null',
	'of',
	'return',
	'static',
	'switch',
	'throw',
	'true',
	'try',
	'type',
	'undefined',
	'var',
	'while'
];
const RUST_KEYWORDS = [
	'as',
	'async',
	'await',
	'const',
	'crate',
	'else',
	'enum',
	'false',
	'fn',
	'for',
	'if',
	'impl',
	'in',
	'let',
	'loop',
	'match',
	'mod',
	'move',
	'mut',
	'pub',
	'return',
	'self',
	'Self',
	'static',
	'struct',
	'super',
	'trait',
	'true',
	'false',
	'use',
	'where',
	'while'
];
const SHELL_KEYWORDS = ['if', 'then', 'else', 'fi', 'for', 'do', 'done', 'case', 'esac', 'in', 'export'];
const JS_LIKE_PATTERNS = [
	pattern('comment', String.raw`//.*$`),
	pattern('url', URL_PATTERN),
	pattern('string', STRING_PATTERN),
	pattern('keyword', keywordPattern(JS_LIKE_KEYWORDS)),
	pattern('property', String.raw`[A-Za-z_$][\w$]*(?=\s*:)`),
	pattern('type', String.raw`[A-Z][A-Za-z0-9_]*`),
	pattern('number', NUMBER_PATTERN),
	pattern('path', PATH_PATTERN)
];
const RUST_PATTERNS = [
	pattern('comment', String.raw`//.*$`),
	pattern('url', URL_PATTERN),
	pattern('string', STRING_PATTERN),
	pattern('keyword', keywordPattern(RUST_KEYWORDS)),
	pattern('type', String.raw`\b(?:Some|None|Ok|Err|Result|Option|String|Vec|HashMap|HashSet)\b|[A-Z][A-Za-z0-9_]*`),
	pattern('number', NUMBER_PATTERN),
	pattern('path', PATH_PATTERN)
];
const JSON_PATTERNS = [
	pattern('property', String.raw`"(?:\\.|[^"\\])*"(?=\s*:)`),
	pattern('string', STRING_PATTERN),
	pattern('keyword', String.raw`\b(?:true|false|null)\b`),
	pattern('number', NUMBER_PATTERN)
];
const OUTPUT_PATTERNS = [
	pattern('url', URL_PATTERN),
	pattern('path', PATH_PATTERN),
	pattern('string', STRING_PATTERN),
	pattern('error', String.raw`\b(?:error|fatal|failed|exception|panic)\b`, 'i'),
	pattern('warning', String.raw`\b(?:warn|warning)\b`, 'i'),
	pattern('success', String.raw`\b(?:ready|ok|done|success|connected|listening|completed)\b`, 'i'),
	pattern('meta', String.raw`\b(?:info|debug|trace)\b`, 'i'),
	pattern('number', NUMBER_PATTERN)
];
const SHELL_REST_PATTERNS = [
	pattern('comment', '#.*$'),
	pattern('url', URL_PATTERN),
	pattern('path', PATH_PATTERN),
	pattern('string', STRING_PATTERN),
	pattern('flag', String.raw`--?[A-Za-z0-9][\w-]*`),
	pattern('keyword', keywordPattern(SHELL_KEYWORDS)),
	pattern('type', String.raw`\$\{?[A-Za-z_][A-Za-z0-9_]*\}?`),
	pattern('number', NUMBER_PATTERN)
];

export function highlightCodeBlock(language: string, text: string): HighlightToken[][] {
	const mode = normalizeLanguage(language);

	return text.split('\n').map((line) => {
		switch (mode) {
			case 'json':
				return tokenizeLine(line, JSON_PATTERNS);
			case 'shell':
				return tokenizeShellLine(line);
			case 'rust':
				return tokenizeLine(line, RUST_PATTERNS);
			case 'javascript':
				return tokenizeLine(line, JS_LIKE_PATTERNS);
			default:
				return ensureVisibleTokens([{ text: line, tone: 'plain' }]);
		}
	});
}

export function highlightTerminalOutput(output: string[] | string): HighlightToken[][] {
	const lines = Array.isArray(output) ? output : output.split('\n');

	return lines.map((line) => {
		if (!looksLikePathLine(line) && looksLikeShellCommand(line)) {
			return tokenizeShellLine(line);
		}

		return tokenizeLine(line, OUTPUT_PATTERNS);
	});
}

export function highlightUnifiedDiff(patch: string): HighlightedDiffLine[] {
	let currentLanguage = 'text';

	return patch.split('\n').map((line) => {
		const kind = classifyDiffLine(line);

		if (line.startsWith('diff --git ')) {
			const parts = line.split(' ');
			currentLanguage = languageFromPath(parts.at(3) ?? parts.at(2) ?? '');
		}

		if (line.startsWith('+++ ')) {
			currentLanguage = languageFromPath(line.slice(4));
		}

		if (line.startsWith('rename to ')) {
			currentLanguage = languageFromPath(line.slice('rename to '.length));
		}

		if (kind === 'meta') {
			return {
				kind,
				tokens: tokenizeDiffMetaLine(line)
			};
		}

		if (kind === 'hunk') {
			return {
				kind,
				tokens: tokenizeDiffHunkLine(line)
			};
		}

		if (kind === 'addition' || kind === 'deletion') {
			const body = highlightCodeBlock(currentLanguage, line.slice(1))[0] ?? [];
			return {
				kind,
				tokens: ensureVisibleTokens([
					{ text: line[0] ?? ' ', tone: kind === 'addition' ? 'addition' : 'deletion' },
					...body
				])
			};
		}

		const body = highlightCodeBlock(currentLanguage, line.startsWith(' ') ? line.slice(1) : line)[0] ?? [];
		return {
			kind,
			tokens: line.startsWith(' ')
				? ensureVisibleTokens([{ text: ' ', tone: 'plain' }, ...body])
				: ensureVisibleTokens(body)
		};
	});
}

export function highlightLanguageLabel(language: string): string {
	switch (normalizeLanguage(language)) {
		case 'javascript':
			return 'TS / JS';
		case 'json':
			return 'JSON';
		case 'rust':
			return 'Rust';
		case 'shell':
			return 'Shell';
		default:
			return language.trim() || 'Text';
	}
}

function tokenizeShellLine(line: string): HighlightToken[] {
	const trimmedStart = line.match(/^\s*/)?.[0] ?? '';
	const content = line.slice(trimmedStart.length);

	if (!content) {
		return ensureVisibleTokens([{ text: line, tone: 'plain' }]);
	}

	if (content.startsWith('#')) {
		return ensureVisibleTokens([
			{ text: trimmedStart, tone: 'plain' },
			{ text: content, tone: 'comment' }
		]);
	}

	const firstSpace = content.search(/\s/);
	const command = firstSpace === -1 ? content : content.slice(0, firstSpace);
	const rest = firstSpace === -1 ? '' : content.slice(firstSpace);
	const commandTone = looksLikeShellCommand(command) ? 'command' : 'plain';

	return ensureVisibleTokens([
		{ text: trimmedStart, tone: 'plain' },
		{ text: command, tone: commandTone },
		...tokenizeLine(rest, SHELL_REST_PATTERNS)
	]);
}

function tokenizeDiffMetaLine(line: string): HighlightToken[] {
	if (line.startsWith('diff --git ')) {
		const parts = line.split(' ');
		return ensureVisibleTokens([
			{ text: 'diff --git ', tone: 'meta' },
			{ text: parts.at(2) ?? '', tone: 'path' },
			{ text: ' ', tone: 'plain' },
			{ text: parts.at(3) ?? '', tone: 'path' }
		]);
	}

	if (line.startsWith('--- ') || line.startsWith('+++ ')) {
		return ensureVisibleTokens([
			{ text: line.slice(0, 4), tone: 'meta' },
			{ text: line.slice(4), tone: 'path' }
		]);
	}

	if (line.startsWith('rename from ') || line.startsWith('rename to ')) {
		const prefix = line.startsWith('rename from ') ? 'rename from ' : 'rename to ';
		return ensureVisibleTokens([
			{ text: prefix, tone: 'meta' },
			{ text: line.slice(prefix.length), tone: 'path' }
		]);
	}

	return tokenizeLine(line, [
		pattern('meta', String.raw`^(?:index|new file mode|deleted file mode|similarity index|old mode|new mode)`),
		pattern('path', PATH_PATTERN),
		pattern('number', NUMBER_PATTERN)
	]);
}

function tokenizeDiffHunkLine(line: string): HighlightToken[] {
	const match = line.match(/^(@@)(\s-\d+(?:,\d+)?)(\s\+\d+(?:,\d+)?)(\s@@)(.*)$/);
	if (!match) {
		return ensureVisibleTokens([{ text: line, tone: 'meta' }]);
	}

	return ensureVisibleTokens([
		{ text: match[1], tone: 'meta' },
		{ text: match[2], tone: 'deletion' },
		{ text: match[3], tone: 'addition' },
		{ text: match[4], tone: 'meta' },
		{ text: match[5], tone: 'comment' }
	]);
}

function tokenizeLine(line: string, patterns: HighlightPattern[]): HighlightToken[] {
	const tokens: HighlightToken[] = [];
	let index = 0;

	while (index < line.length) {
		let matched = false;

		for (const entry of patterns) {
			entry.regex.lastIndex = index;
			const result = entry.regex.exec(line);
			if (!result || result.index !== index) {
				continue;
			}

			pushToken(tokens, { text: result[0], tone: entry.tone });
			index += result[0].length;
			matched = true;
			break;
		}

		if (!matched) {
			pushToken(tokens, { text: line[index] ?? '', tone: 'plain' });
			index += 1;
		}
	}

	return ensureVisibleTokens(tokens);
}

function ensureVisibleTokens(tokens: HighlightToken[]): HighlightToken[] {
	if (!tokens.length) {
		return [{ text: ' ', tone: 'plain' }];
	}

	if (tokens.every((token) => token.text === '')) {
		return [{ text: ' ', tone: 'plain' }];
	}

	return tokens;
}

function pushToken(tokens: HighlightToken[], token: HighlightToken) {
	if (!token.text) {
		return;
	}

	const previous = tokens.at(-1);
	if (previous && previous.tone === token.tone) {
		previous.text += token.text;
		return;
	}

	tokens.push({ ...token });
}

function normalizeLanguage(language: string): string {
	const normalized = language.trim().toLowerCase();

	if (['ts', 'tsx', 'js', 'jsx', 'typescript', 'javascript'].includes(normalized)) {
		return 'javascript';
	}

	if (['bash', 'shell', 'sh', 'zsh', 'fish'].includes(normalized)) {
		return 'shell';
	}

	if (normalized === 'rs' || normalized === 'rust') {
		return 'rust';
	}

	if (normalized === 'json' || normalized === 'jsonc') {
		return 'json';
	}

	return normalized || 'text';
}

function languageFromPath(path: string): string {
	const normalized = path.replace(/^[ab]\//, '');
	const extension = normalized.split('.').at(-1)?.toLowerCase() ?? '';

	switch (extension) {
		case 'ts':
		case 'tsx':
		case 'js':
		case 'jsx':
		case 'mjs':
		case 'cjs':
			return 'javascript';
		case 'json':
		case 'jsonc':
			return 'json';
		case 'rs':
			return 'rust';
		case 'sh':
		case 'zsh':
		case 'bash':
			return 'shell';
		default:
			return 'text';
	}
}

function looksLikeShellCommand(text: string): boolean {
	return /^[A-Za-z_.~/][A-Za-z0-9_./-]*(?:\s|$)/.test(text.trimStart());
}

function looksLikePathLine(text: string): boolean {
	return /^(?:~\/|\/|\.{1,2}\/)/.test(text.trimStart());
}

function keywordPattern(words: string[]): string {
	return String.raw`\b(?:${words.join('|')})\b`;
}

function rawPattern(source: string): string {
	return source;
}

function pattern(tone: HighlightTone, source: string, flags = ''): HighlightPattern {
	return {
		tone,
		regex: new RegExp(source, `${flags}y`)
	};
}

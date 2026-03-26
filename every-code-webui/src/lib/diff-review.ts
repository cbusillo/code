export type DiffLineKind = 'meta' | 'hunk' | 'addition' | 'deletion' | 'context';

export type DiffReviewFile = {
	path: string;
	additions: number;
	deletions: number;
	hunks: number;
};

export type DiffReviewSummary = {
	files: DiffReviewFile[];
	totalAdditions: number;
	totalDeletions: number;
	totalHunks: number;
};

type MutableDiffReviewFile = DiffReviewFile;

export function summarizeUnifiedDiff(patch: string, fallbackPath = ''): DiffReviewSummary {
	const lines = patch.split('\n');
	const files: MutableDiffReviewFile[] = [];
	let currentFile: MutableDiffReviewFile | null = null;

	const ensureCurrentFile = () => {
		if (currentFile) {
			return currentFile;
		}

		currentFile = {
			path: normalizeDiffPath(fallbackPath) || 'Workspace changes',
			additions: 0,
			deletions: 0,
			hunks: 0
		};
		files.push(currentFile);
		return currentFile;
	};

	for (const line of lines) {
		if (line.startsWith('diff --git ')) {
			const parts = line.split(' ');
			const nextPath =
				normalizeDiffPath(parts.at(3) ?? '') || normalizeDiffPath(parts.at(2) ?? '') || fallbackPath;
			currentFile = {
				path: nextPath || 'Workspace changes',
				additions: 0,
				deletions: 0,
				hunks: 0
			};
			files.push(currentFile);
			continue;
		}

		if (line.startsWith('rename to ')) {
			ensureCurrentFile().path = normalizeDiffPath(line.slice('rename to '.length)) || ensureCurrentFile().path;
			continue;
		}

		if (line.startsWith('+++ ')) {
			const nextPath = normalizeDiffPath(line.slice(4));
			if (nextPath) {
				ensureCurrentFile().path = nextPath;
			}
			continue;
		}

		if (line.startsWith('@@')) {
			ensureCurrentFile().hunks += 1;
			continue;
		}

		if (line.startsWith('+') && !line.startsWith('+++')) {
			ensureCurrentFile().additions += 1;
			continue;
		}

		if (line.startsWith('-') && !line.startsWith('---')) {
			ensureCurrentFile().deletions += 1;
		}
	}

	if (!files.length && patch.trim()) {
		files.push({
			path: normalizeDiffPath(fallbackPath) || 'Workspace changes',
			additions: countMatchingDiffLines(lines, 'addition'),
			deletions: countMatchingDiffLines(lines, 'deletion'),
			hunks: countMatchingDiffLines(lines, 'hunk')
		});
	}

	return {
		files,
		totalAdditions: files.reduce((sum, file) => sum + file.additions, 0),
		totalDeletions: files.reduce((sum, file) => sum + file.deletions, 0),
		totalHunks: files.reduce((sum, file) => sum + file.hunks, 0)
	};
}

export function classifyDiffLine(line: string): DiffLineKind {
	if (line.startsWith('@@')) {
		return 'hunk';
	}

	if (
		line.startsWith('diff --git ') ||
		line.startsWith('index ') ||
		line.startsWith('--- ') ||
		line.startsWith('+++ ') ||
		line.startsWith('rename ') ||
		line.startsWith('new file ') ||
		line.startsWith('deleted file ') ||
		line.startsWith('similarity index ') ||
		line.startsWith('old mode ') ||
		line.startsWith('new mode ')
	) {
		return 'meta';
	}

	if (line.startsWith('+')) {
		return 'addition';
	}

	if (line.startsWith('-')) {
		return 'deletion';
	}

	return 'context';
}

function normalizeDiffPath(path: string) {
	if (!path || path === '/dev/null') {
		return '';
	}

	if (path.startsWith('a/') || path.startsWith('b/')) {
		return path.slice(2);
	}

	return path;
}

function countMatchingDiffLines(lines: string[], kind: Exclude<DiffLineKind, 'meta' | 'context'>) {
	return lines.reduce((count, line) => count + Number(classifyDiffLine(line) === kind), 0);
}

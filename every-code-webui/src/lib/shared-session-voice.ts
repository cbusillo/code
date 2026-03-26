export type VoiceCaptureState = 'idle' | 'listening' | 'unsupported' | 'error';

type WindowLike = {
	SpeechRecognition?: unknown;
	webkitSpeechRecognition?: unknown;
	isSecureContext?: boolean;
};

export function supportsVoiceInput(windowLike: WindowLike | undefined) {
	if (!windowLike) {
		return false;
	}

	return Boolean(windowLike.SpeechRecognition || windowLike.webkitSpeechRecognition);
}

export function appendVoiceTranscript(draft: string, transcript: string) {
	const next = transcript.trim();
	if (!next) {
		return draft;
	}

	if (!draft.trim()) {
		return next;
	}

	const separator = /[\s\n]$/.test(draft) ? '' : ' ';
	return `${draft}${separator}${next}`;
}

export function voiceCaptureStatusLabel(
	state: VoiceCaptureState,
	interimTranscript: string,
	errorMessage: string
) {
	if (state === 'listening') {
		return interimTranscript
			? `Listening... ${interimTranscript}`
			: 'Listening for your next prompt...';
	}

	if (state === 'error') {
		return errorMessage || 'Voice capture hit a browser error.';
	}

	if (state === 'unsupported') {
		return 'Voice capture is not available in this browser.';
	}

	return 'Use dictation to capture the next prompt hands-free.';
}

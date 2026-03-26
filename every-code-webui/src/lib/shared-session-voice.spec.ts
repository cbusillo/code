import { describe, expect, it } from 'vitest';

import {
	appendVoiceTranscript,
	supportsVoiceInput,
	voiceCaptureStatusLabel
} from '$lib/shared-session-voice';

describe('shared-session-voice', () => {
	it('detects standard and prefixed browser voice support', () => {
		expect(supportsVoiceInput(undefined)).toBe(false);
		expect(supportsVoiceInput({})).toBe(false);
		expect(supportsVoiceInput({ SpeechRecognition: class {} })).toBe(true);
		expect(supportsVoiceInput({ webkitSpeechRecognition: class {} })).toBe(true);
	});

	it('appends dictation without crushing existing draft spacing', () => {
		expect(appendVoiceTranscript('', 'Ship the patch today')).toBe('Ship the patch today');
		expect(appendVoiceTranscript('Ship it', 'today')).toBe('Ship it today');
		expect(appendVoiceTranscript('Ship it\n', 'today')).toBe('Ship it\ntoday');
	});

	it('describes the current capture state for the UI', () => {
		expect(voiceCaptureStatusLabel('idle', '', '')).toContain('dictation');
		expect(voiceCaptureStatusLabel('listening', '', '')).toContain('Listening');
		expect(voiceCaptureStatusLabel('listening', 'drafting the next prompt', '')).toContain(
			'drafting the next prompt'
		);
		expect(voiceCaptureStatusLabel('unsupported', '', '')).toContain('not available');
		expect(voiceCaptureStatusLabel('error', '', 'Microphone access was denied.')).toBe(
			'Microphone access was denied.'
		);
	});
});

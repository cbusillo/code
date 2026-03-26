import { describe, expect, it } from 'vitest';

import { assessWebSocketConnection } from '$lib/shared-session-connection-policy';

describe('assessWebSocketConnection', () => {
	it('treats loopback ws targets as the safe default', () => {
		const assessment = assessWebSocketConnection('ws://127.0.0.1:8877');

		expect(assessment).toMatchObject({
			badge: 'Local only',
			canConnect: true,
			requiresAcknowledgement: false,
			tone: 'positive',
			normalizedUrl: 'ws://127.0.0.1:8877/'
		});
		expect(assessment.guide).toMatchObject({
			title: 'Loopback-first remote access',
			codeSampleLabel: 'SSH tunnel',
			docsPath: 'docs/shared-session-remote-access.md'
		});
		expect(assessment.guide.identityBoundary).toContain('same operator');
		expect(assessment.sshTunnelCommand).toBe(
			'ssh -L 8877:127.0.0.1:8877 user@remote-host'
		);
	});

	it('allows private-network ws targets with a warning', () => {
		const assessment = assessWebSocketConnection('ws://192.168.1.24:8877');

		expect(assessment).toMatchObject({
			badge: 'Trusted LAN only',
			canConnect: true,
			requiresAcknowledgement: true,
			tone: 'warning'
		});
		expect(assessment.acknowledgementKey).toBe('ws://192.168.1.24:8877/');
		expect(assessment.guide.title).toBe('Trusted LAN or VPN only');
		expect(assessment.guide.identityBoundary).toContain('your effective authority');
		expect(assessment.sshTunnelCommand).toBe(
			'ssh -L 8877:127.0.0.1:8877 user@192.168.1.24'
		);
	});

	it('blocks raw remote ws targets', () => {
		const assessment = assessWebSocketConnection('ws://code.example.com:8877');

		expect(assessment).toMatchObject({
			badge: 'Blocked',
			canConnect: false,
			requiresAcknowledgement: false,
			tone: 'danger'
		});
		expect(assessment.recommendation).toContain('wss://');
		expect(assessment.guide).toMatchObject({
			title: 'Authenticated wss:// proxy',
			codeSample: 'reverse_proxy 127.0.0.1:8877'
		});
		expect(assessment.guide.identityBoundary).toContain('not as an isolated second user');
		expect(assessment.guide.steps[2]?.detail).toContain('wss://code.example.com:8877/');
		expect(assessment.sshTunnelCommand).toBe(
			'ssh -L 8877:127.0.0.1:8877 user@code.example.com'
		);
	});

	it('allows encrypted remote targets', () => {
		const assessment = assessWebSocketConnection('wss://companion.example.com/socket');

		expect(assessment).toMatchObject({
			badge: 'Encrypted transport',
			canConnect: true,
			requiresAcknowledgement: true,
			tone: 'positive',
			normalizedUrl: 'wss://companion.example.com/socket'
		});
		expect(assessment.acknowledgementKey).toBe('wss://companion.example.com/socket');
		expect(assessment.acknowledgementLabel).toContain('single-user companion surface');
		expect(assessment.guide.steps[2]?.detail).toContain('wss://companion.example.com/socket');
		expect(assessment.sshTunnelCommand).toBeNull();
	});

	it('rejects non-websocket URLs', () => {
		const assessment = assessWebSocketConnection('https://companion.example.com');

		expect(assessment).toMatchObject({
			badge: 'Wrong scheme',
			canConnect: false,
			tone: 'danger',
			normalizedUrl: null
		});
		expect(assessment.guide.title).toBe('Authenticated wss:// proxy');
	});
});

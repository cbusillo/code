export type ConnectionAssessmentTone = 'neutral' | 'positive' | 'warning' | 'danger';

export type ConnectionGuideStep = {
	label: string;
	detail: string;
};

export type ConnectionGuide = {
	title: string;
	summary: string;
	identityBoundary: string;
	steps: ConnectionGuideStep[];
	docsPath: string;
	codeSampleLabel: string | null;
	codeSample: string | null;
};

export type ConnectionAssessment = {
	badge: string;
	headline: string;
	detail: string;
	recommendation: string;
	guide: ConnectionGuide;
	tone: ConnectionAssessmentTone;
	canConnect: boolean;
	requiresAcknowledgement: boolean;
	acknowledgementKey: string | null;
	acknowledgementLabel: string | null;
	normalizedUrl: string | null;
	sshTunnelCommand: string | null;
};

const LOOPBACK_HOSTS = new Set(['localhost', '127.0.0.1', '::1', '0:0:0:0:0:0:0:1']);
const REMOTE_ACCESS_DOC_PATH = 'docs/shared-session-remote-access.md';

export function assessWebSocketConnection(rawUrl: string): ConnectionAssessment {
	const trimmedUrl = rawUrl.trim();
	if (!trimmedUrl) {
		return {
			badge: 'Add address',
			headline: 'Enter a WebSocket address first',
			detail: 'The companion only connects over ws:// or wss:// endpoints.',
			recommendation:
				'Use ws://127.0.0.1:8877 for local-only work, or put remote access behind wss:// plus external access control.',
			guide: buildLoopbackGuide('8877', 'ssh -L 8877:127.0.0.1:8877 user@remote-host'),
			tone: 'neutral',
			canConnect: false,
			requiresAcknowledgement: false,
			acknowledgementKey: null,
			acknowledgementLabel: null,
			normalizedUrl: null,
			sshTunnelCommand: null
		};
	}

	let parsedUrl: URL;
	try {
		parsedUrl = new URL(trimmedUrl);
	} catch {
		return {
			badge: 'Invalid URL',
			headline: 'Use a full ws:// or wss:// URL',
			detail: 'Shared-session transport is WebSocket-native, so plain hostnames or http:// URLs will not work here.',
			recommendation:
				'Try ws://127.0.0.1:8877 for a local app-server, or wss://companion.example.com for a remote proxy.',
			guide: buildAuthenticatedProxyGuide('8877', 'wss://companion.example.com'),
			tone: 'danger',
			canConnect: false,
			requiresAcknowledgement: false,
			acknowledgementKey: null,
			acknowledgementLabel: null,
			normalizedUrl: null,
			sshTunnelCommand: null
		};
	}

	if (parsedUrl.protocol !== 'ws:' && parsedUrl.protocol !== 'wss:') {
		return {
			badge: 'Wrong scheme',
			headline: 'Only WebSocket URLs are supported',
			detail: 'The workspace speaks directly to the app-server over WebSockets.',
			recommendation:
				'Switch this address to ws:// or wss://. If you have an https:// origin, use its matching wss:// socket endpoint here.',
			guide: buildAuthenticatedProxyGuide(parsedUrl.port || '443', 'wss://companion.example.com/socket'),
			tone: 'danger',
			canConnect: false,
			requiresAcknowledgement: false,
			acknowledgementKey: null,
			acknowledgementLabel: null,
			normalizedUrl: null,
			sshTunnelCommand: null
		};
	}

	const normalizedUrl = parsedUrl.toString();
	const hostScope = classifyHost(parsedUrl.hostname);
	const remotePort = parsedUrl.port || (parsedUrl.protocol === 'wss:' ? '443' : '80');
	const acknowledgementKey = `${parsedUrl.protocol}//${parsedUrl.host}${parsedUrl.pathname}${parsedUrl.search}`;
	const sshHost =
		hostScope === 'loopback' ? 'user@remote-host' : `user@${parsedUrl.hostname}`;
	const sshTunnelCommand = `ssh -L ${remotePort}:127.0.0.1:${remotePort} ${sshHost}`;

	if (parsedUrl.protocol === 'wss:') {
		return {
			badge: 'Encrypted transport',
			headline: 'Remote transport is encrypted',
			detail:
				'This endpoint uses wss://, which is the minimum bar for companion access outside the local machine.',
			recommendation:
				'Keep it behind a trusted reverse proxy, VPN, or other access-control layer because the raw app-server itself does not authenticate browsers.',
			guide: buildAuthenticatedProxyGuide(remotePort, normalizedUrl),
			tone: 'positive',
			canConnect: true,
			requiresAcknowledgement: true,
			acknowledgementKey,
			acknowledgementLabel:
				'I understand this remote endpoint is still a single-user companion surface, and anyone who clears its outer boundary can act as me here.',
			normalizedUrl,
			sshTunnelCommand: null
		};
	}

	if (hostScope === 'loopback') {
		return {
			badge: 'Local only',
			headline: 'Safe default for desktop or SSH-tunneled use',
			detail:
				'This ws:// address stays on loopback, so it is not directly exposed to your network.',
			recommendation:
				'For remote access, keep the server bound to loopback and forward this port over SSH instead of exposing raw WebSockets.',
			guide: buildLoopbackGuide(remotePort, sshTunnelCommand),
			tone: 'positive',
			canConnect: true,
			requiresAcknowledgement: false,
			acknowledgementKey: null,
			acknowledgementLabel: null,
			normalizedUrl,
			sshTunnelCommand
		};
	}

	if (hostScope === 'private') {
		return {
			badge: 'Trusted LAN only',
			headline: 'Raw WebSocket on a private network',
			detail:
				'This address looks like a LAN or VPN target, but ws:// traffic is still unencrypted and unauthenticated.',
			recommendation:
				'Use this only on a network you trust. For internet or mobile access, prefer SSH port forwarding or a wss:// proxy with access control.',
			guide: buildTrustedLanGuide(remotePort, sshTunnelCommand),
			tone: 'warning',
			canConnect: true,
			requiresAcknowledgement: true,
			acknowledgementKey,
			acknowledgementLabel:
				'I understand this private-network endpoint is personal, and anyone else who can reach it can steer the same companion session with my effective authority.',
			normalizedUrl,
			sshTunnelCommand
		};
	}

	return {
		badge: 'Blocked',
		headline: 'Raw remote ws:// is not allowed',
		detail:
			'This address looks remote and would expose the browser companion over an unencrypted WebSocket.',
		recommendation:
			'Switch to wss:// behind a trusted proxy, or keep the browser on ws://127.0.0.1 via SSH tunneling instead.',
		guide: buildAuthenticatedProxyGuide(remotePort, `wss://${parsedUrl.host}${parsedUrl.pathname}${parsedUrl.search}`),
		tone: 'danger',
		canConnect: false,
		requiresAcknowledgement: false,
		acknowledgementKey: null,
		acknowledgementLabel: null,
		normalizedUrl,
		sshTunnelCommand
	};
}

function buildLoopbackGuide(remotePort: string, sshTunnelCommand: string): ConnectionGuide {
	return {
		title: 'Loopback-first remote access',
		summary:
			'The supported default is to keep code-app-server on 127.0.0.1 and bring the browser to it through a tunnel instead of exposing raw WebSockets.',
		identityBoundary:
			'This is still a single-user surface: any browser you connect through this tunnel is acting as the same operator, not as a separate collaborator.',
		steps: [
			{
				label: 'Keep the server local',
				detail: `Run code-app-server on 127.0.0.1:${remotePort} on the main machine.`
			},
			{
				label: 'Forward the port when away',
				detail: 'Use SSH port forwarding from the client device so the browser still talks to loopback.'
			},
			{
				label: 'Connect locally in the browser',
				detail: 'After the tunnel is up, connect the companion to ws://127.0.0.1 on the forwarded port.'
			}
		],
		docsPath: REMOTE_ACCESS_DOC_PATH,
		codeSampleLabel: 'SSH tunnel',
		codeSample: sshTunnelCommand
	};
}

function buildTrustedLanGuide(remotePort: string, sshTunnelCommand: string): ConnectionGuide {
	return {
		title: 'Trusted LAN or VPN only',
		summary:
			'Plain ws:// on a private address is acceptable only when the network itself is the trust boundary; it is not the supported internet-facing deployment.',
		identityBoundary:
			'Keep this personal. Anyone else who can reach the same private endpoint can steer the same companion session with your effective authority.',
		steps: [
			{
				label: 'Limit this to private networks',
				detail: 'Use this mode only on home LAN, tailnet, or another network you already trust.'
			},
			{
				label: 'Prefer a tunnel for travel',
				detail: 'If you are away from that network, switch back to loopback plus SSH tunneling instead of keeping ws:// exposed.'
			},
			{
				label: 'Move to wss:// for broader access',
				detail: `For mobile or internet reachability, put 127.0.0.1:${remotePort} behind an authenticated TLS proxy.`
			}
		],
		docsPath: REMOTE_ACCESS_DOC_PATH,
		codeSampleLabel: 'Safer tunnel fallback',
		codeSample: sshTunnelCommand
	};
}

function buildAuthenticatedProxyGuide(remotePort: string, remoteUrl: string): ConnectionGuide {
	return {
		title: 'Authenticated wss:// proxy',
		summary:
			'The supported remote/mobile topology is app-server on loopback, then TLS and access control at a reverse proxy or VPN edge, then the browser connects over wss://.',
		identityBoundary:
			'Access here should represent one trusted operator. Any person who clears that outer boundary can act as your companion session, not as an isolated second user.',
		steps: [
			{
				label: 'Bind app-server to loopback',
				detail: `Keep code-app-server on 127.0.0.1:${remotePort} so it is never directly exposed to the internet.`
			},
			{
				label: 'Terminate TLS and gate access externally',
				detail: 'Put HTTPS plus WebSocket upgrade handling in front, and require VPN, SSO, or another auth boundary there.'
			},
			{
				label: 'Point the browser at the proxy',
				detail: `Use ${remoteUrl} from the companion once the proxy is enforcing that outer boundary.`
			}
		],
		docsPath: REMOTE_ACCESS_DOC_PATH,
		codeSampleLabel: 'Proxy upstream target',
		codeSample: `reverse_proxy 127.0.0.1:${remotePort}`
	};
}

function classifyHost(hostname: string): 'loopback' | 'private' | 'public' {
	const normalizedHost = hostname.trim().toLowerCase();
	if (LOOPBACK_HOSTS.has(normalizedHost)) {
		return 'loopback';
	}

	const ipv4Octets = parseIpv4(normalizedHost);
	if (ipv4Octets) {
		if (ipv4Octets[0] === 127) {
			return 'loopback';
		}

		if (
			ipv4Octets[0] === 10 ||
			(ipv4Octets[0] === 172 && ipv4Octets[1] >= 16 && ipv4Octets[1] <= 31) ||
			(ipv4Octets[0] === 192 && ipv4Octets[1] === 168) ||
			(ipv4Octets[0] === 169 && ipv4Octets[1] === 254)
		) {
			return 'private';
		}

		return 'public';
	}

	if (normalizedHost.includes(':')) {
		if (normalizedHost === '::1' || normalizedHost === '0:0:0:0:0:0:0:1') {
			return 'loopback';
		}

		if (
			normalizedHost.startsWith('fc') ||
			normalizedHost.startsWith('fd') ||
			normalizedHost.startsWith('fe8') ||
			normalizedHost.startsWith('fe9') ||
			normalizedHost.startsWith('fea') ||
			normalizedHost.startsWith('feb')
		) {
			return 'private';
		}

		return 'public';
	}

	if (!normalizedHost.includes('.') || normalizedHost.endsWith('.local')) {
		return 'private';
	}

	return 'public';
}

function parseIpv4(hostname: string): number[] | null {
	const octets = hostname.split('.');
	if (octets.length !== 4) {
		return null;
	}

	const parsedOctets = octets.map((octet) => Number.parseInt(octet, 10));
	if (parsedOctets.some((octet) => Number.isNaN(octet) || octet < 0 || octet > 255)) {
		return null;
	}

	return parsedOctets;
}

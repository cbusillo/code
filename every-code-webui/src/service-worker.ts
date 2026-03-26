import { build, files, version } from '$service-worker';

const CACHE_NAME = `every-code-companion-${version}`;
const ASSETS = new Set([...build, ...files]);

self.addEventListener('install', (event) => {
	event.waitUntil(
		caches
			.open(CACHE_NAME)
			.then((cache) => cache.addAll([...ASSETS]))
			.then(() => self.skipWaiting())
	);
});

self.addEventListener('activate', (event) => {
	event.waitUntil(
		caches
			.keys()
			.then((keys) =>
				Promise.all(
					keys
						.filter((key) => key !== CACHE_NAME)
						.map((key) => caches.delete(key))
				)
			)
			.then(() => self.clients.claim())
	);
});

self.addEventListener('fetch', (event) => {
	if (event.request.method !== 'GET') {
		return;
	}

	const url = new URL(event.request.url);
	if (url.origin !== self.location.origin || !ASSETS.has(url.pathname)) {
		return;
	}

	event.respondWith(caches.match(event.request).then((response) => response ?? fetch(event.request)));
});

self.addEventListener('notificationclick', (event) => {
	event.notification.close();

	const targetUrl =
		typeof event.notification.data?.url === 'string'
			? event.notification.data.url
			: self.location.origin;

	event.waitUntil(focusOrOpenClient(targetUrl));
});

async function focusOrOpenClient(targetUrl: string) {
	const windowClients = await self.clients.matchAll({
		type: 'window',
		includeUncontrolled: true
	});

	for (const client of windowClients) {
		if (!('focus' in client)) {
			continue;
		}

		const windowClient = client as WindowClient;
		await windowClient.navigate(targetUrl);
		return windowClient.focus();
	}

	return self.clients.openWindow(targetUrl);
}

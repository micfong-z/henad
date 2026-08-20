// Offline fallback for the web build.
//
// Network first. A reachable server always wins and the cache only answers when the fetch fails.
// Nothing is precached and no artefact name appears here, so renaming a trunk output is a
// non-event.

// Bump to discard everything a previous worker stored.
const CACHE = 'henad-v1';

self.addEventListener('install', () => {
  // Take over from the previous worker now. Waiting for every tab to close leaves a user stuck
  // on a bad one with no way out but to find them all.
  self.skipWaiting();
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    (async () => {
      const names = await caches.keys();
      await Promise.all(names.filter((name) => name !== CACHE).map((name) => caches.delete(name)));
      await self.clients.claim();
    })(),
  );
});

self.addEventListener('fetch', (event) => {
  const request = event.request;
  if (request.method !== 'GET' || new URL(request.url).origin !== self.location.origin) {
    return;
  }

  event.respondWith(
    (async () => {
      try {
        const response = await fetch(request);
        // 200 only. `cache.put` rejects a 206, and a redirect or an error page is not worth
        // keeping for offline.
        if (response.status === 200) {
          const copy = response.clone();
          event.waitUntil(caches.open(CACHE).then((cache) => cache.put(request, copy)));
        }
        return response;
      } catch (error) {
        const cached = await caches.match(request);
        if (cached) {
          return cached;
        }
        throw error;
      }
    })(),
  );
});

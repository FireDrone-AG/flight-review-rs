// FireDrone: links to other tools, set on the server with --nav-link /
// FLIGHT_REVIEW_NAV_LINKS. Fetched once when first shown; none if the server
// has none (or is older and lacks the endpoint).
import { readable } from 'svelte/store';
import { browser } from '$app/environment';

export interface ToolLink {
	name: string;
	url: string;
}

export const toolLinks = readable<ToolLink[]>([], (set) => {
	if (!browser) return;
	fetch('/api/nav-links')
		.then((res) => (res.ok ? res.json() : []))
		.then((links) => set(Array.isArray(links) ? links : []))
		.catch(() => set([]));
});

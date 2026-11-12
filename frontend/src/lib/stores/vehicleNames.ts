// FireDrone: display names for vehicle names, e.g. "Dev Drone 1" for fd-0003
// (server: --vehicle-names-url / FLIGHT_REVIEW_VEHICLE_NAMES_URL, from
// FireDrone Fleet). Fetched once when first shown; without names, IDs show.
import { readable } from 'svelte/store';
import { browser } from '$app/environment';

export const vehicleNames = readable<Record<string, string>>({}, (set) => {
	if (!browser) return;
	fetch('/api/vehicle-names')
		.then((res) => (res.ok ? res.json() : {}))
		.then((names: unknown) =>
			set(names && typeof names === 'object' && !Array.isArray(names) ? (names as Record<string, string>) : {})
		)
		.catch(() => set({}));
});

/** The display name of a vehicle name, or the vehicle name itself. */
export function vehicleLabel(names: Record<string, string>, id: string): string {
	return names[id] || id;
}

<!-- FireDrone: time in each flight mode over the logs the page shows
     (GET /api/stats/flight-modes). Whole logs, as in the log viewer's
     flight mode bar: time on the ground before takeoff counts too. -->
<script lang="ts">
	import type { FlightModeStat } from '$lib/types';
	import { formatDuration } from '$lib/utils/formatters';
	import { getModeColor, getModeName } from '$lib/utils/modeColors';
	import { vehicleNames, vehicleLabel } from '$lib/stores/vehicleNames';

	let {
		data,
		loading = false,
		drone,
	}: {
		data: FlightModeStat[];
		loading?: boolean;
		drone?: string;
	} = $props();

	// The server stores mode names; the colors are by PX4 nav_state number.
	function color(mode: string): string {
		for (let id = 0; id < 32; id++) {
			if (getModeName(id) === mode) return getModeColor(id);
		}
		return getModeColor(-1);
	}

	const total = $derived(data.reduce((sum, m) => sum + m.total_s, 0));
	const longest = $derived(Math.max(...data.map((m) => m.total_s), 0));
</script>

<div class="rounded-lg bg-white p-6 ring-1 ring-gray-200">
	<h3 class="text-sm font-medium text-gray-500">
		Flight modes{#if drone}<span class="text-gray-400">{` · ${vehicleLabel($vehicleNames, drone)}`}</span>{/if}
	</h3>
	<p class="mt-1 text-xs text-gray-400">Time in each mode, whole logs</p>
	{#if loading}
		<div class="animate-pulse mt-4 space-y-3">
			{#each Array(4) as _}
				<div class="h-6 bg-gray-100 rounded w-full"></div>
			{/each}
		</div>
	{:else if data.length === 0}
		<p class="text-sm text-gray-400 py-8 text-center">No flight modes</p>
	{:else}
		<ul class="mt-4 space-y-3">
			{#each data as m (m.mode)}
				<li>
					<div class="flex items-baseline justify-between gap-3 text-sm">
						<span class="flex items-center gap-2 font-medium text-gray-900">
							<span class="size-2.5 rounded-full" style="background-color: {color(m.mode)}"></span>
							{m.mode}
						</span>
						<span class="text-gray-500 tabular-nums">
							{formatDuration(m.total_s)}
							<span class="text-gray-400">· {Math.round((m.total_s / total) * 100)}% · {m.logs} {m.logs === 1 ? 'log' : 'logs'}</span>
						</span>
					</div>
					<div class="mt-1 h-2 rounded-full bg-gray-100">
						<div class="h-2 rounded-full" style="width: {(m.total_s / longest) * 100}%; background-color: {color(m.mode)}"></div>
					</div>
				</li>
			{/each}
		</ul>
	{/if}
</div>

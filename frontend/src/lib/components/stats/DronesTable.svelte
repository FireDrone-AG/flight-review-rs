<!-- FireDrone: flights per drone (vehicle_name). Clicking a drone filters the
     whole page to it; clicking it again shows all drones. -->
<script lang="ts">
	import type { StatsDataPoint } from '$lib/types';
	import { formatDuration } from '$lib/utils/formatters';
	import { vehicleNames, vehicleLabel } from '$lib/stores/vehicleNames';

	let {
		data,
		loading = false,
		selected,
		onDroneClick,
	}: {
		data: StatsDataPoint[];
		loading?: boolean;
		selected?: string;
		onDroneClick: (drone: string | undefined) => void;
	} = $props();

	const hours = (d: StatsDataPoint) => (d.total_flight_hours ?? 0) * 3600;
</script>

<div class="rounded-lg bg-white ring-1 ring-gray-200 overflow-hidden">
	<div class="px-6 py-4">
		<h3 class="text-sm font-medium text-gray-500">Drones</h3>
	</div>
	{#if loading}
		<div class="animate-pulse px-6 pb-6 space-y-3">
			{#each Array(4) as _}
				<div class="h-6 bg-gray-100 rounded w-full"></div>
			{/each}
		</div>
	{:else if data.length === 0}
		<p class="text-sm text-gray-400 py-8 text-center">No logs from drones</p>
	{:else}
		<div class="overflow-x-auto">
			<table class="min-w-full divide-y divide-gray-200">
				<thead class="bg-gray-50">
					<tr>
						<th class="whitespace-nowrap px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">Drone</th>
						<th class="whitespace-nowrap px-4 py-3 text-right text-xs font-medium text-gray-500 uppercase tracking-wider">Flights</th>
						<th class="whitespace-nowrap px-4 py-3 text-right text-xs font-medium text-gray-500 uppercase tracking-wider">Flight time</th>
						<th class="whitespace-nowrap px-4 py-3 text-right text-xs font-medium text-gray-500 uppercase tracking-wider">Avg duration</th>
						<th class="px-4 py-3"><span class="sr-only">Logs</span></th>
					</tr>
				</thead>
				<tbody class="divide-y divide-gray-100">
					{#each data as item (item.group)}
						<tr class={item.group === selected ? 'bg-indigo-50' : 'hover:bg-gray-50'}>
							<td class="whitespace-nowrap px-4 py-3 text-sm">
								<button
									type="button"
									class="font-medium text-indigo-600 hover:text-indigo-500 hover:underline"
									title={item.group === selected ? 'Show all drones' : `Show only ${item.group}`}
									onclick={() => onDroneClick(item.group === selected ? undefined : item.group)}
								>
									{vehicleLabel($vehicleNames, item.group)}
								</button>
							</td>
							<td class="whitespace-nowrap px-4 py-3 text-sm text-gray-700 text-right">{item.count.toLocaleString()}</td>
							<td class="whitespace-nowrap px-4 py-3 text-sm text-gray-700 text-right">{formatDuration(hours(item))}</td>
							<td class="whitespace-nowrap px-4 py-3 text-sm text-gray-700 text-right">{formatDuration(item.avg_flight_duration_s ?? null)}</td>
							<td class="whitespace-nowrap px-4 py-3 text-sm text-right">
								<a href="/browse?search={encodeURIComponent(item.group)}" class="text-gray-500 hover:text-gray-700">Logs</a>
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	{/if}
</div>

<!-- FireDrone: links to other tools (see $lib/stores/toolLinks). "nav" for
     the sidebar and the mobile menu, "bar" for the log viewer's top bar. -->
<script lang="ts">
	import { toolLinks } from '$lib/stores/toolLinks';

	let { variant = 'nav', class: cls = '' } = $props<{ variant?: 'nav' | 'bar'; class?: string }>();

	// heroicons arrow-top-right-on-square
	const icon =
		'M13.5 6H5.25A2.25 2.25 0 0 0 3 8.25v10.5A2.25 2.25 0 0 0 5.25 21h10.5A2.25 2.25 0 0 0 18 18.75V10.5m-10.5 6L21 3m0 0h-5.25M21 3v5.25';
</script>

{#if $toolLinks.length > 0}
	{#if variant === 'bar'}
		<div class="flex items-center gap-4 {cls}">
			{#each $toolLinks as link}
				<a href={link.url} class="flex items-center gap-1.5 text-sm text-gray-500 hover:text-gray-700">
					{link.name}
					<svg class="size-4" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" aria-hidden="true">
						<path stroke-linecap="round" stroke-linejoin="round" d={icon} />
					</svg>
				</a>
			{/each}
		</div>
	{:else}
		<ul role="list" class={cls}>
			{#each $toolLinks as link}
				<li>
					<a
						href={link.url}
						class="group flex gap-x-3 rounded-md p-2 text-sm font-semibold leading-6 text-gray-700 hover:bg-gray-50 hover:text-indigo-600"
					>
						<svg class="size-6 shrink-0 text-gray-400 group-hover:text-indigo-600" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" aria-hidden="true">
							<path stroke-linecap="round" stroke-linejoin="round" d={icon} />
						</svg>
						{link.name}
					</a>
				</li>
			{/each}
		</ul>
	{/if}
{/if}

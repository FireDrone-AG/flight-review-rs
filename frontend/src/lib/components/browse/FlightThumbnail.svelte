<script lang="ts">
	import type { TrackPoint } from '$lib/api';
	import { getTrack } from '$lib/api';
	import { getModeColor } from '$lib/utils/modeColors';

	let { logId, width = 160, height = 100 }: { logId: string; width?: number; height?: number } = $props();

	let canvas: HTMLCanvasElement | undefined = $state();
	let sentinel: HTMLDivElement | undefined = $state();
	let status = $state<'idle' | 'loading' | 'loaded' | 'no-gps' | 'error'>('idle');
	// FireDrone: flights without GPS get their local position track instead
	// (metres from where the estimator started), with a scale bar.
	let scaleBar = $state<{ px: number; label: string } | null>(null);

	// Module-level cache shared across all instances
	const trackCache = new Map<string, TrackPoint[]>();

	/** A track point on the plane: `h` to the right (lon, or metres east),
	 *  `v` up (lat, or metres north); `b`: starts a new line. */
	type PlanePoint = { h: number; v: number; m: number; b: boolean };

	function toPlane(track: TrackPoint[]): PlanePoint[] {
		return track.map((p) =>
			'x' in p ? { h: p.y, v: p.x, m: p.m, b: !!p.b } : { h: p.lon, v: p.lat, m: p.m, b: false }
		);
	}

	/** Longest round length (m) that's at most `maxPx` long at `pxPerM`. */
	function scaleBarFor(pxPerM: number, maxPx: number) {
		const lengths = [0.1, 0.2, 0.5, 1, 2, 5, 10, 20, 50, 100, 200, 500, 1000, 2000, 5000];
		const m = lengths.filter((l) => l * pxPerM <= maxPx).pop() ?? lengths[0];
		return { px: m * pxPerM, label: m >= 1000 ? `${m / 1000} km` : `${m} m` };
	}

	/** Draws the track; returns the scale in px per unit. `minRange`: the
	 *  smallest extent shown, so a track that barely moves isn't blown up. */
	function drawTrack(ctx: CanvasRenderingContext2D, track: PlanePoint[], w: number, h: number, minRange: number) {
		if (track.length < 2) return 0;

		const pad = 8;
		let minLat = Infinity, maxLat = -Infinity;
		let minLon = Infinity, maxLon = -Infinity;
		for (const pt of track) {
			if (pt.v < minLat) minLat = pt.v;
			if (pt.v > maxLat) maxLat = pt.v;
			if (pt.h < minLon) minLon = pt.h;
			if (pt.h > maxLon) maxLon = pt.h;
		}

		const latRange = Math.max(maxLat - minLat, minRange);
		const lonRange = Math.max(maxLon - minLon, minRange);
		const usableW = w - 2 * pad;
		const usableH = h - 2 * pad;
		const scale = Math.min(usableW / lonRange, usableH / latRange);
		const cx = (minLon + maxLon) / 2;
		const cy = (minLat + maxLat) / 2;

		function toX(lon: number) { return pad + usableW / 2 + (lon - cx) * scale; }
		function toY(lat: number) { return pad + usableH / 2 - (lat - cy) * scale; }

		ctx.clearRect(0, 0, w, h);
		ctx.lineWidth = 1.5;
		ctx.lineCap = 'round';
		ctx.lineJoin = 'round';

		let prevMode = track[0].m;
		ctx.beginPath();
		ctx.strokeStyle = getModeColor(prevMode);
		ctx.moveTo(toX(track[0].h), toY(track[0].v));

		for (let i = 1; i < track.length; i++) {
			const pt = track[i];
			const x = toX(pt.h);
			const y = toY(pt.v);
			if (pt.b) {
				// don't join across an estimator reset
				ctx.stroke();
				ctx.beginPath();
				ctx.strokeStyle = getModeColor(pt.m);
				ctx.moveTo(x, y);
				prevMode = pt.m;
			} else if (pt.m !== prevMode) {
				ctx.lineTo(x, y);
				ctx.stroke();
				ctx.beginPath();
				ctx.strokeStyle = getModeColor(pt.m);
				ctx.moveTo(x, y);
				prevMode = pt.m;
			} else {
				ctx.lineTo(x, y);
			}
		}
		ctx.stroke();

		// Start marker
		const sx = toX(track[0].h);
		const sy = toY(track[0].v);
		ctx.beginPath();
		ctx.arc(sx, sy, 3, 0, Math.PI * 2);
		ctx.fillStyle = '#22c55e';
		ctx.fill();

		// End marker
		const ex = toX(track[track.length - 1].h);
		const ey = toY(track[track.length - 1].v);
		ctx.beginPath();
		ctx.arc(ex, ey, 3, 0, Math.PI * 2);
		ctx.fillStyle = '#ef4444';
		ctx.fill();

		return scale;
	}

	async function loadAndDraw() {
		if (status !== 'idle') return;
		status = 'loading';
		try {
			let track = trackCache.get(logId);
			if (!track) {
				track = await getTrack(logId);
				trackCache.set(logId, track);
			}
			if (track.length < 2) {
				status = 'no-gps';
				return;
			}
			if (canvas) {
				const ctx = canvas.getContext('2d');
				if (ctx) {
					const dpr = window.devicePixelRatio || 1;
					canvas.width = width * dpr;
					canvas.height = height * dpr;
					ctx.scale(dpr, dpr);
					const local = 'x' in track[0];
					// GPS: as upstream (0.0001° minimum); local: at least 1 m
					const scale = drawTrack(ctx, toPlane(track), width, height, local ? 1 : 0.0001);
					scaleBar = local && scale > 0 ? scaleBarFor(scale, width * 0.4) : null;
				}
			}
			status = 'loaded';
		} catch {
			status = 'error';
		}
	}

	$effect(() => {
		if (!sentinel) return;
		const observer = new IntersectionObserver(
			(entries) => {
				if (entries[0]?.isIntersecting) {
					loadAndDraw();
					observer.disconnect();
				}
			},
			{ rootMargin: '200px' }
		);
		observer.observe(sentinel);
		return () => observer.disconnect();
	});
</script>

<div
	bind:this={sentinel}
	class="relative rounded bg-gray-50 ring-1 ring-gray-200 overflow-hidden flex items-center justify-center"
	style="width: {width}px; height: {height}px;"
	title={scaleBar ? 'No GPS: local position, from where the drone started' : undefined}
>
	{#if status === 'idle' || status === 'loading'}
		<div class="text-xs text-gray-300">
			{#if status === 'loading'}
				<svg class="animate-spin size-4" fill="none" viewBox="0 0 24 24">
					<circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
					<path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8v4a4 4 0 00-4 4H4z"></path>
				</svg>
			{/if}
		</div>
	{:else if status === 'no-gps'}
		<span class="text-xs text-gray-300">No GPS</span>
	{:else if status === 'error'}
		<span class="text-xs text-red-300">Error</span>
	{/if}
	<canvas
		bind:this={canvas}
		style="width: {width}px; height: {height}px; {status === 'loaded' ? '' : 'display: none;'}"
	></canvas>
	{#if status === 'loaded' && scaleBar}
		<div class="pointer-events-none absolute bottom-1 left-1.5 flex flex-col items-start gap-px text-[9px] leading-none text-gray-500">
			<span>{scaleBar.label}</span>
			<span class="block h-0.5 rounded-full bg-gray-400" style="width: {scaleBar.px}px"></span>
		</div>
	{/if}
</div>

// FireDrone: canvas colors that follow the theme. dark.css swaps the gray
// variables in dark mode, so reading them at draw time gives the right shade
// (the fallbacks are the light-mode values).
function themeColor(variable: string, fallback: string): string {
	if (typeof document === 'undefined') return fallback;
	return getComputedStyle(document.documentElement).getPropertyValue(variable).trim() || fallback;
}

/** Grid lines (gray-200) */
export const gridColor = () => themeColor('--color-gray-200', '#e5e7eb');
/** Plot area border (gray-300) */
export const borderColor = () => themeColor('--color-gray-300', '#d1d5db');

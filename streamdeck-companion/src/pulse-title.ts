/** Formats a real weekly total without turning every duration under an hour into "0 h". */
export function formatPulseSummaryTitle(seconds: number): string {
  const safeSeconds = Number.isFinite(seconds) ? Math.max(0, Math.floor(seconds)) : 0;
  const totalMinutes = Math.floor(safeSeconds / 60);
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;

  if (hours === 0) return `${totalMinutes} min\nCette semaine`;
  if (minutes === 0) return `${hours} h\nCette semaine`;
  return `${hours} h ${minutes} min\nCette semaine`;
}

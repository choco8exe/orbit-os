import { defaultSettings, type OrbitSettings, type SettingsService } from './contracts';

const KEY = 'orbit.settings.v1';

export function validateOrbitSettings(value: unknown): OrbitSettings {
  if (!value || typeof value !== 'object') return defaultSettings;
  const input = value as Partial<OrbitSettings>;
  const refresh = input.coreRefreshSeconds;
  const history = input.coreHistoryMinutes;
  const theme = isOrbitTheme(input.theme) ? input.theme : 'sidereal';
  const profiles = normalizeThemeProfiles(input.themeColorProfiles);
  const activeProfile = profiles[theme];
  return {
    ...defaultSettings,
    ...input,
    version: 1,
    locale: 'fr',
    onboardingComplete: Boolean(input.onboardingComplete),
    theme,
    accentColor: isAccessibleAccentColor(input.accentColor, theme) ? input.accentColor.toUpperCase() : activeProfile?.accentColor ?? '',
    panelColor: isAccessibleSurfaceColor(input.panelColor, theme === 'lunar') ? input.panelColor.toUpperCase() : activeProfile?.panelColor ?? '',
    raisedColor: isAccessibleSurfaceColor(input.raisedColor, theme === 'lunar') ? input.raisedColor.toUpperCase() : activeProfile?.raisedColor ?? '',
    themeColorProfiles: profiles,
    orbitalVisualization: isOrbitalVisualization(input.orbitalVisualization) ? input.orbitalVisualization : 'earth',
    orbitalEnabled: typeof input.orbitalEnabled === 'boolean' ? input.orbitalEnabled : defaultSettings.orbitalEnabled,
    orbitalRotationSpeed: input.orbitalRotationSpeed === 0.5 || input.orbitalRotationSpeed === 1.5 ? input.orbitalRotationSpeed : 1,
    orbitalParticlesEnabled: typeof input.orbitalParticlesEnabled === 'boolean' ? input.orbitalParticlesEnabled : defaultSettings.orbitalParticlesEnabled,
    orbitalIntensity: input.orbitalIntensity === 'low' || input.orbitalIntensity === 'high' ? input.orbitalIntensity : 'balanced',
    coreRefreshSeconds: refresh === 5 || refresh === 10 ? refresh : 2,
    coreHistoryMinutes: history === 5 || history === 60 ? history : 15,
    timelineRetentionDays: input.timelineRetentionDays === 7 || input.timelineRetentionDays === 90 ? input.timelineRetentionDays : 30,
    alerts: {
      cpuPercent: bounded(input.alerts?.cpuPercent, 10, 100, 90),
      memoryPercent: bounded(input.alerts?.memoryPercent, 10, 100, 90),
      diskFreePercent: bounded(input.alerts?.diskFreePercent, 1, 50, 10),
    },
    widgetOrder: Array.isArray(input.widgetOrder) ? input.widgetOrder.filter((x): x is string => typeof x === 'string') : defaultSettings.widgetOrder,
    hiddenWidgets: Array.isArray(input.hiddenWidgets) ? input.hiddenWidgets.filter((x): x is string => typeof x === 'string') : [],
  };
}
function isOrbitalVisualization(value: unknown): value is OrbitSettings['orbitalVisualization'] {
  return value === 'earth' || value === 'ringed-planet' || value === 'lunar-station' || value === 'nebula' || value === 'core' || value === 'wireframe' || value === 'constellations';
}
const orbitThemes = ['sidereal', 'nebula', 'obsidian', 'lunar', 'aurora', 'solar', 'abyss'] as const;
function isOrbitTheme(value: unknown): value is OrbitSettings['theme'] {
  return typeof value === 'string' && orbitThemes.some(theme => theme === value);
}
function normalizeThemeProfiles(value: unknown): OrbitSettings['themeColorProfiles'] {
  if (!value || typeof value !== 'object') return {};
  const profiles: OrbitSettings['themeColorProfiles'] = {};
  for (const theme of orbitThemes) {
    const profile = (value as Record<string, unknown>)[theme];
    if (!profile || typeof profile !== 'object') continue;
    const colors = profile as Record<string, unknown>;
    profiles[theme] = {
      accentColor: isAccessibleAccentColor(colors.accentColor, theme) ? colors.accentColor.toUpperCase() : '',
      panelColor: isAccessibleSurfaceColor(colors.panelColor, theme === 'lunar') ? colors.panelColor.toUpperCase() : '',
      raisedColor: isAccessibleSurfaceColor(colors.raisedColor, theme === 'lunar') ? colors.raisedColor.toUpperCase() : '',
    };
  }
  return profiles;
}
export function isAccessibleAccentColor(value: unknown, theme: OrbitSettings['theme'] | boolean = 'sidereal'): value is string {
  if (typeof value !== 'string' || !/^#[\da-f]{6}$/i.test(value)) return false;
  const lightTheme = theme === true || theme === 'lunar';
  const luminance = (hex: string) => {
    const channels = hex.slice(1).match(/.{2}/g)?.map(channel => parseInt(channel, 16) / 255) ?? [];
    const linear = channels.map(channel => channel <= .04045 ? channel / 12.92 : ((channel + .055) / 1.055) ** 2.4);
    return linear[0] * .2126 + linear[1] * .7152 + linear[2] * .0722;
  };
  const foreground = luminance(lightTheme ? toneDownForLightTheme(value) : value);
  // User accents are used as text against the selected theme's canvas. A
  // color from the opposite luminance range is discarded when themes change.
  const canvasByTheme: Record<OrbitSettings['theme'], string> = { sidereal: '#080b0a', nebula: '#090815', obsidian: '#030404', lunar: '#f5f0e7', aurora: '#071311', solar: '#17100b', abyss: '#030b10' };
  const canvas = luminance(lightTheme ? '#f5f0e7' : typeof theme === 'string' ? canvasByTheme[theme] : '#030404');
  return (Math.max(foreground, canvas) + .05) / (Math.min(foreground, canvas) + .05) >= 4.5;
}
function toneDownForLightTheme(hex: string): string {
  const source = hex.slice(1).match(/.{2}/g)?.map(channel => parseInt(channel, 16)) ?? [];
  const ink = [28, 26, 22];
  return `#${source.map((channel, index) => Math.round(channel * .3 + ink[index] * .7).toString(16).padStart(2, '0')).join('')}`;
}
const DARK_THEME_TEXT_COLORS = ['#f5f1e8', '#c9c7bc', '#f1edff', '#c4bfd9', '#fffdf6', '#d5d6cf'];
const LIGHT_THEME_TEXT_COLORS = ['#28251f', '#5b554a'];
function relativeLuminance(hex: string): number {
  const channels = hex.slice(1).match(/.{2}/g)?.map(channel => parseInt(channel, 16) / 255) ?? [];
  const linear = channels.map(channel => channel <= .04045 ? channel / 12.92 : ((channel + .055) / 1.055) ** 2.4);
  return linear[0] * .2126 + linear[1] * .7152 + linear[2] * .0722;
}
/** Both normal and secondary text must retain WCAG AA contrast in each built-in theme. */
export function isAccessibleSurfaceColor(value: unknown, lightTheme = false): value is string {
  if (typeof value !== 'string' || !/^#[\da-f]{6}$/i.test(value)) return false;
  const background = relativeLuminance(value);
  return (lightTheme ? LIGHT_THEME_TEXT_COLORS : DARK_THEME_TEXT_COLORS).every(color => {
    const foreground = relativeLuminance(color);
    return (Math.max(foreground, background) + .05) / (Math.min(foreground, background) + .05) >= 4.5;
  });
}
function bounded(n: unknown, min: number, max: number, fallback: number): number {
  return typeof n === 'number' && Number.isFinite(n) ? Math.min(max, Math.max(min, n)) : fallback;
}

export const settingsService: SettingsService = {
  async read() {
      try { return validateOrbitSettings(JSON.parse(localStorage.getItem(KEY) ?? 'null')); }
    catch { return defaultSettings; }
  },
  async update(patch) {
    const current = await this.read();
    const profiles = { ...current.themeColorProfiles, [current.theme]: { accentColor: current.accentColor, panelColor: current.panelColor, raisedColor: current.raisedColor } };
    const themeColors = patch.theme !== undefined && patch.theme !== current.theme ? profiles[patch.theme] : undefined;
    const next = validateOrbitSettings({ ...current, ...patch, ...(themeColors ? themeColors : {}), themeColorProfiles: profiles, alerts: { ...current.alerts, ...patch.alerts } });
    next.themeColorProfiles[next.theme] = { accentColor: next.accentColor, panelColor: next.panelColor, raisedColor: next.raisedColor };
    localStorage.setItem(KEY, JSON.stringify(next));
    return next;
  },
  async reset() { localStorage.removeItem(KEY); return defaultSettings; },
};

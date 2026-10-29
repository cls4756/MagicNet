export type SettingsRoute = "proxy" | "dns" | "kernel" | "apps" | "outbound" | "builtin-outbound" | "logs" | "maint" | "service";

export const SETTINGS_ROUTES: readonly SettingsRoute[] = [
  "proxy",
  "dns",
  "kernel",
  "apps",
  "outbound",
  "builtin-outbound",
  "logs",
  "maint",
  "service",
];

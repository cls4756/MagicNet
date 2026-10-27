export type SettingsRoute = "proxy" | "kernel" | "apps" | "outbound" | "builtin-outbound" | "logs" | "maint" | "service";

export const SETTINGS_ROUTES: readonly SettingsRoute[] = [
  "proxy",
  "kernel",
  "apps",
  "outbound",
  "builtin-outbound",
  "logs",
  "maint",
  "service",
];

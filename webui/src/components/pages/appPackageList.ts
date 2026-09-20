import type { PackageInfo } from "@/types";

/**
 * App rows arrive from two places: the root CLI (`pm list packages`) and the
 * KernelSU package bridge. Only the bridge knows display names and can feed the
 * `ksu://icon/<package>` scheme, so every row is normalized here and each helper
 * degrades to the package name instead of inventing metadata.
 */
const PACKAGE_NAME_PATTERN = /^[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)+$/;

export function isPackageName(value: unknown): value is string {
  return typeof value === "string" && PACKAGE_NAME_PATTERN.test(value);
}

/** Mirrors the CLI parser: unknown metadata stays empty, never guessed. */
export function fallbackPackageInfo(packageName: string): PackageInfo {
  return {
    packageName,
    versionName: "",
    versionCode: 0,
    appLabel: packageName,
    isSystem: false,
    uid: 0,
  };
}

/**
 * Accepts one bridge record. A record without a usable package name is dropped;
 * a missing or blank label keeps the package name so the row is still readable.
 */
export function normalizePackageInfo(value: unknown, fallbackName = ""): PackageInfo | null {
  if (typeof value !== "object" || value === null) return null;
  const record = value as Record<string, unknown>;
  const packageName = isPackageName(record.packageName)
    ? record.packageName
    : isPackageName(fallbackName)
      ? fallbackName
      : "";
  if (!packageName) return null;
  const label = typeof record.appLabel === "string" ? record.appLabel.trim() : "";
  const versionName = typeof record.versionName === "string" ? record.versionName : "";
  return {
    packageName,
    versionName,
    versionCode: nonNegativeInteger(record.versionCode),
    appLabel: label || packageName,
    isSystem: record.isSystem === true,
    uid: nonNegativeInteger(record.uid),
  };
}

function nonNegativeInteger(value: unknown): number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : 0;
}

/**
 * The package list decides order and membership; device metadata only fills in
 * the display fields. Unknown packages keep the CLI values and duplicate or
 * invalid entries are dropped.
 */
export function mergePackageInfo(
  packageNames: readonly string[],
  deviceInfo: readonly unknown[],
): PackageInfo[] {
  const byName = new Map<string, PackageInfo>();
  for (const entry of deviceInfo) {
    const info = normalizePackageInfo(entry);
    if (info) byName.set(info.packageName, info);
  }
  const merged: PackageInfo[] = [];
  const seen = new Set<string>();
  for (const packageName of packageNames) {
    if (!isPackageName(packageName) || seen.has(packageName)) continue;
    seen.add(packageName);
    merged.push(byName.get(packageName) ?? fallbackPackageInfo(packageName));
  }
  return merged;
}

export function packageDisplayName(app: PackageInfo): string {
  const label = app.appLabel.trim();
  return label || app.packageName;
}

/** The list is searchable by display name and by package name. */
export function packageMatchesQuery(app: PackageInfo, query: string): boolean {
  const needle = query.trim().toLowerCase();
  if (!needle) return true;
  return (
    app.packageName.toLowerCase().includes(needle) ||
    packageDisplayName(app).toLowerCase().includes(needle)
  );
}

export function filterVisiblePackages(
  packages: readonly PackageInfo[],
  query: string,
  limit = 120,
): PackageInfo[] {
  const visible: PackageInfo[] = [];
  for (const app of packages) {
    if (!packageMatchesQuery(app, query)) continue;
    visible.push(app);
    if (visible.length >= limit) break;
  }
  return visible;
}

/**
 * Applied entries come first inside a list box; everything else keeps a stable
 * alphabetical order so a freshly checked row does not jump to the top before
 * the change is written to the device.
 */
export function comparePackagesByLabel(a: PackageInfo, b: PackageInfo): number {
  const byLabel = packageDisplayName(a).localeCompare(packageDisplayName(b), undefined, {
    sensitivity: "base",
  });
  if (byLabel !== 0) return byLabel;
  return a.packageName.localeCompare(b.packageName);
}

/** Fallback badge for apps whose icon the manager cannot serve. */
export function packageInitial(app: PackageInfo): string {
  const label = packageDisplayName(app).trim();
  return label ? label.slice(0, 1).toUpperCase() : "?";
}

/**
 * KernelSU serves app icons through `ksu://icon/<package>` and only guarantees
 * the scheme when the package list API exists, so callers gate on the bridge.
 */
export function packageIconUrl(packageName: string, iconsAvailable: boolean): string | null {
  if (!iconsAvailable || !isPackageName(packageName)) return null;
  return `ksu://icon/${encodeURIComponent(packageName)}`;
}

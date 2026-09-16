import type { PackageInfo } from "@/types";
import { isPackageName, mergePackageInfo } from "@/components/pages/appPackageList";

/**
 * KernelSU injects `window.ksu`; the package helpers exchange JSON text and are
 * only present on managers that ship the package list API. The Module WebUI also
 * runs in ordinary browsers and through the HTTP API, so a missing or broken
 * bridge must report "no device data" instead of throwing.
 */
type PackageBridge = {
  listPackages?(type: string): unknown;
  getPackagesInfo?(packages: string): unknown;
};

function packageBridge(): PackageBridge | undefined {
  const bridge = (globalThis as { ksu?: PackageBridge }).ksu;
  if (!bridge) return undefined;
  return typeof bridge.listPackages === "function" || typeof bridge.getPackagesInfo === "function"
    ? bridge
    : undefined;
}

function decodeList(value: unknown): unknown[] {
  if (Array.isArray(value)) return value;
  if (typeof value !== "string" || !value.trim()) return [];
  try {
    const parsed: unknown = JSON.parse(value);
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

/** The icon scheme ships together with the package list API. */
export function devicePackageIconsAvailable(): boolean {
  return typeof packageBridge()?.listPackages === "function";
}

/**
 * Installed packages with their display names when the manager can answer, and
 * an empty list when it cannot. Callers keep the root CLI as the fallback.
 */
export function readDevicePackages(): PackageInfo[] {
  const bridge = packageBridge();
  if (!bridge?.listPackages) return [];
  let names: string[];
  try {
    names = decodeList(bridge.listPackages("all")).filter(isPackageName);
  } catch {
    return [];
  }
  if (!names.length) return [];
  let info: unknown[] = [];
  if (typeof bridge.getPackagesInfo === "function") {
    try {
      info = decodeList(bridge.getPackagesInfo(JSON.stringify(names)));
    } catch {
      info = [];
    }
  }
  return mergePackageInfo(names, info);
}

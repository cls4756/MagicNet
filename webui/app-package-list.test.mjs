import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { afterEach, test } from "node:test";
import {
  fallbackPackageInfo,
  filterVisiblePackages,
  isPackageName,
  mergePackageInfo,
  normalizePackageInfo,
  packageDisplayName,
  packageIconUrl,
  packageInitial,
  packageMatchesQuery,
} from "./src/components/pages/appPackageList.ts";
import { devicePackageIconsAvailable, readDevicePackages } from "./src/composables/devicePackages.ts";

afterEach(() => {
  delete globalThis.ksu;
});

const app = (packageName, appLabel) => ({
  packageName,
  appLabel,
  versionName: "",
  versionCode: 0,
  isSystem: false,
  uid: 0,
});

test("app rows are searchable by display name, not only by package name", () => {
  const wechat = app("com.tencent.mm", "微信");
  const chrome = app("com.android.chrome", "Chrome");
  assert.equal(packageMatchesQuery(wechat, "微信"), true, "label match must survive a package-name miss");
  assert.equal(packageMatchesQuery(wechat, "Tencent"), true, "package names stay searchable case-insensitively");
  assert.equal(packageMatchesQuery(wechat, "weixin"), false, "a miss must stay a miss");
  assert.equal(packageMatchesQuery(wechat, "   "), true, "a blank query is not a filter");
  assert.deepEqual(
    filterVisiblePackages([wechat, chrome], "chrome", 120).map((item) => item.packageName),
    ["com.android.chrome"],
  );
  assert.deepEqual(
    filterVisiblePackages([wechat, chrome], "微", 120).map((item) => item.packageName),
    ["com.tencent.mm"],
  );
  assert.deepEqual(
    filterVisiblePackages([wechat, chrome], "", 1).map((item) => item.packageName),
    ["com.tencent.mm"],
    "the visible cap keeps list order",
  );
  assert.equal(packageDisplayName(app("com.example.blank", "  ")), "com.example.blank");
  assert.equal(packageInitial(wechat), "微");
  assert.equal(packageInitial(app("com.example.blank", "")), "C");
});

test("icons are only emitted for a capable manager and a valid package name", () => {
  assert.equal(packageIconUrl("com.tencent.mm", true), "ksu://icon/com.tencent.mm");
  assert.equal(packageIconUrl("com.tencent.mm", false), null, "no bridge means no icon scheme");
  assert.equal(packageIconUrl("not a package", true), null);
  assert.equal(packageIconUrl("", true), null);
  assert.equal(isPackageName("com.example.app"), true);
  assert.equal(isPackageName("com"), false);
  assert.equal(isPackageName("com..example"), false);
});

test("device packages carry labels and degrade to an empty list without a bridge", () => {
  assert.deepEqual(readDevicePackages(), []);
  assert.equal(devicePackageIconsAvailable(), false);

  globalThis.ksu = {
    listPackages: (type) => {
      assert.equal(type, "all");
      return JSON.stringify(["com.tencent.mm", "com.android.chrome"]);
    },
    getPackagesInfo: (packages) => {
      assert.deepEqual(JSON.parse(packages), ["com.tencent.mm", "com.android.chrome"]);
      return JSON.stringify([
        {
          packageName: "com.tencent.mm",
          appLabel: "微信",
          isSystem: false,
          uid: 10234,
          versionName: "8.0.56",
          versionCode: 2600,
        },
        { packageName: "com.android.chrome", appLabel: "Chrome" },
      ]);
    },
  };

  assert.equal(devicePackageIconsAvailable(), true);
  const packages = readDevicePackages();
  assert.deepEqual(packages.map((item) => item.appLabel), ["微信", "Chrome"]);
  assert.deepEqual(packages[0], {
    packageName: "com.tencent.mm",
    versionName: "8.0.56",
    versionCode: 2600,
    appLabel: "微信",
    isSystem: false,
    uid: 10234,
  });
  assert.equal(packages[1].uid, 0, "missing metadata must not be invented");
});

test("a broken or narrow bridge never fabricates labels", () => {
  globalThis.ksu = { listPackages: () => "not json" };
  assert.deepEqual(readDevicePackages(), []);

  globalThis.ksu = { listPackages: () => { throw new Error("bridge gone"); } };
  assert.deepEqual(readDevicePackages(), []);

  globalThis.ksu = { listPackages: () => JSON.stringify(["com.example.app", "com", "com.example.app"]) };
  assert.deepEqual(
    readDevicePackages().map((item) => item.packageName),
    ["com.example.app"],
    "invalid and duplicate names are dropped",
  );
  assert.equal(readDevicePackages()[0].appLabel, "com.example.app", "a manager without package info keeps names readable");

  globalThis.ksu = {
    listPackages: () => JSON.stringify(["com.example.app"]),
    getPackagesInfo: () => { throw new Error("package info unavailable"); },
  };
  assert.equal(readDevicePackages()[0].appLabel, "com.example.app");
});

test("device metadata enriches CLI rows without reordering or inventing entries", () => {
  const merged = mergePackageInfo(
    ["com.tencent.mm", "com.android.chrome", "com.example.unknown", "com.tencent.mm", "bad name"],
    [
      { packageName: "com.tencent.mm", appLabel: "微信", uid: 10234, isSystem: false },
      { packageName: "com.android.chrome", appLabel: "   " },
      { packageName: "com.example.other", appLabel: "Other" },
      { packageName: "com.example.bad", appLabel: "Bad", uid: -1, versionCode: 1.5, isSystem: "yes" },
    ],
  );
  assert.deepEqual(
    merged.map((item) => item.packageName),
    ["com.tencent.mm", "com.android.chrome", "com.example.unknown"],
  );
  assert.equal(merged[0].appLabel, "微信");
  assert.equal(merged[1].appLabel, "com.android.chrome", "a blank label falls back to the package name");
  assert.deepEqual(merged[2], fallbackPackageInfo("com.example.unknown"));

  const rejected = normalizePackageInfo({ packageName: "com.example.bad", uid: -1, versionCode: 1.5, isSystem: "yes" });
  assert.equal(rejected.uid, 0);
  assert.equal(rejected.versionCode, 0);
  assert.equal(rejected.isSystem, false);
  assert.equal(normalizePackageInfo(null), null);
  assert.equal(normalizePackageInfo({ appLabel: "no package name" }), null);
  assert.equal(normalizePackageInfo({ appLabel: "fallback" }, "com.example.app").packageName, "com.example.app");
});

test("the apps page renders names and icons and searches by both", () => {
  const page = readFileSync(new URL("./src/components/pages/AppsPage.vue", import.meta.url), "utf8");
  assert.match(page, /appIcon\(app\.packageName\)/);
  assert.match(page, /packageDisplayName\(app\)/);
  assert.match(page, /packageInitial\(app\)/);
  assert.match(page, /packageLabel\(pkg\)/, "already selected apps must show their name too");
  assert.match(page, /t\('搜索应用名称或包名'\)/);
});

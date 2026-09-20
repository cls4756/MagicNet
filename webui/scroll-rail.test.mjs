import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { SCROLL_RAIL_MIN_THUMB, scrollRailGeometry } from "./src/lib/scrollRail.ts";

const scrollBox = readFileSync(new URL("./src/components/ui/ScrollBox.vue", import.meta.url), "utf8");

test("a list that fits shows no rail", () => {
  assert.deepEqual(scrollRailGeometry({ scrollTop: 0, scrollHeight: 300, clientHeight: 320 }), {
    visible: false,
    height: 0,
    offset: 0,
  });
  assert.deepEqual(scrollRailGeometry({ scrollTop: 0, scrollHeight: 0, clientHeight: 0 }), {
    visible: false,
    height: 0,
    offset: 0,
  });
});

test("a longer list shows a thumb that tracks the scroll position", () => {
  const metrics = { scrollHeight: 2000, clientHeight: 320 };
  const top = scrollRailGeometry({ ...metrics, scrollTop: 0 });
  assert.equal(top.visible, true);
  assert.equal(top.offset, 0);
  assert.equal(top.height, Math.round((320 / 2000) * 320));

  const bottom = scrollRailGeometry({ ...metrics, scrollTop: 2000 - 320 });
  assert.equal(bottom.height, top.height);
  assert.equal(bottom.offset, 320 - top.height);

  const middle = scrollRailGeometry({ ...metrics, scrollTop: (2000 - 320) / 2 });
  assert.ok(middle.offset > 0 && middle.offset < bottom.offset);
});

test("a very long list keeps a grabbable thumb", () => {
  const rail = scrollRailGeometry({ scrollTop: 0, scrollHeight: 200000, clientHeight: 320 });
  assert.equal(rail.height, SCROLL_RAIL_MIN_THUMB);
  assert.ok(SCROLL_RAIL_MIN_THUMB < 320, "the minimum thumb must still fit a short viewport");
});

test("an out-of-range scroll offset stays inside the track", () => {
  const rail = scrollRailGeometry({ scrollTop: 99999, scrollHeight: 2000, clientHeight: 320 });
  assert.equal(rail.offset, 320 - rail.height);
});

test("ScrollBox draws the rail only where the platform reserves no scrollbar space", () => {
  assert.match(scrollBox, /class="mn-scroll-box"/);
  assert.match(scrollBox, /mn-scroll-box__rail" aria-hidden="true"/);
  assert.match(scrollBox, /mn-scroll-box__thumb/);
  assert.match(scrollBox, /element\.offsetWidth - element\.clientWidth - border > 1/);
  assert.match(scrollBox, /@scroll="sync"/);
  assert.match(scrollBox, /new ResizeObserver\(sync\)/);
});

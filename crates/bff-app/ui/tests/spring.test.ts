import { test } from "node:test";
import assert from "node:assert/strict";
import { PLAYFUL, rubberBand, settled, STANDARD, step, stretch } from "../src/spring.ts";

function run(target: number, o = STANDARD, from = { x: 0, v: 0 }, seconds = 2) {
  let s = from;
  let peak = from.x;
  for (let i = 0; i < seconds * 120; i++) {
    s = step(s, target, 1 / 120, o);
    peak = Math.max(peak, s.x);
  }
  return { s, peak };
}

test("the standard spring settles on its target with only a little overshoot", () => {
  const { s, peak } = run(1);
  assert.ok(settled(s, 1, 0.002), `${s.x}`);
  assert.ok(peak > 1 && peak < 1.03, `overshoot ${peak}`);
});

test("the playful spring overshoots more, and still settles", () => {
  const std = run(1).peak;
  const { s, peak } = run(1, PLAYFUL, undefined, 3);
  assert.ok(peak > std + 0.05, `${peak} vs ${std}`);
  assert.ok(settled(s, 1, 0.002));
});

test("a new target keeps the speed it had, so the motion does not restart", () => {
  let s = { x: 0, v: 0 };
  for (let i = 0; i < 12; i++) s = step(s, 1, 1 / 120, STANDARD);
  assert.ok(s.v > 0, "moving toward the first target");
  const redirected = step(s, 0, 1 / 120, STANDARD);
  assert.ok(Math.abs(redirected.x - s.x) < 0.05, "no jump");
  assert.ok(redirected.v < s.v, "slowing, but from where it was");
  assert.ok(run(0, STANDARD, s).s.x < 0.01 && settled(run(0, STANDARD, s).s, 0, 0.002));
});

test("past its limit a drag moves less and less", () => {
  assert.equal(rubberBand(50, 100), 50);
  const a = rubberBand(150, 100), b = rubberBand(300, 100);
  assert.ok(a > 100 && a < 150);
  assert.ok(b > a && b < 300 && b - a < 150, `${a} ${b}`);
});

test("stretch grows with speed, is capped, and keeps the volume", () => {
  const slow = stretch(5), fast = stretch(1000);
  assert.ok(fast.sx > slow.sx && fast.sx <= 1.3 + 1e-9);
  for (const t of [slow, fast]) assert.ok(Math.abs(t.sx * t.sy - 1) < 1e-9);
});

import { test } from "node:test";
import assert from "node:assert/strict";
import { appOf, clock, fold, initialStatus } from "../src/status.ts";

// Lines as castable-receiver --log json writes them.
const at = (t: string) => `"timestamp":"2026-11-27T${t}Z"`;
const lines = [
  `{${at("20:54:00")},"level":"INFO","message":"ready","name":"My Mac","player":"mpv","control":"full","url":"http://192.168.31.134:49152/desc.xml","log_file":"/u/.config/castable/logs/receiver.log"}`,
  `{${at("20:54:01")},"level":"INFO","message":"cast from","peer":"192.168.31.20","user_agent":"Bilibili/7.62.0 (Linux; Android 14)"}`,
  `{${at("20:54:01")},"level":"INFO","message":"player command","command":"load","gen":1,"url":"https://cdn.x/hls/a.m3u8","title":"头号玩家 原声版","host":"cdn.x","format":"hls","live":false}`,
  `{${at("20:54:01")},"level":"INFO","message":"transport","from":"NO_MEDIA_PRESENT","to":"TRANSITIONING","gen":1}`,
  `not json at all`,
  `{${at("20:54:03")},"level":"INFO","message":"media info","event":"info","gen":1,"summary":"1080p · 1920×1080 · H.264 · 23.98 fps | AAC · 48 kHz · stereo","decoder":"videotoolbox"}`,
  `{${at("20:54:03")},"level":"INFO","message":"transport","from":"TRANSITIONING","to":"PLAYING","gen":1}`,
  `{${at("20:54:04")},"level":"INFO","message":"player command","command":"seek","to_s":15.0}`,
  `{${at("20:55:05")},"level":"TRACE","message":"player event","event":"progress","gen":1,"position_s":75.5,"duration_s":8400.0}`,
];

test("folds a real cast into the status screen", () => {
  const s = lines.reduce(fold, initialStatus);
  assert.equal(s.ready?.name, "My Mac");
  assert.equal(s.state, "PLAYING");
  assert.deepEqual(s.now, {
    title: "头号玩家 原声版",
    url: "https://cdn.x/hls/a.m3u8",
    host: "cdn.x",
    format: "hls",
    live: false,
    from: { ip: "192.168.31.20", app: "Bilibili 7.62.0" },
  });
  assert.equal(s.quality?.summary, "1080p · 1920×1080 · H.264 · 23.98 fps | AAC · 48 kHz · stereo");
  assert.equal(s.quality?.decoder, "videotoolbox");
  assert.deepEqual(s.position, { secs: 75.5, of: 8400 });
  assert.deepEqual(
    s.recent.map((r) => r.text),
    [
      "cast: 头号玩家 原声版",
      "NO_MEDIA_PRESENT → TRANSITIONING",
      "quality: 1080p · 1920×1080 · H.264 · 23.98 fps | AAC · 48 kHz · stereo",
      "TRANSITIONING → PLAYING",
      "seek to 0:15",
    ],
  );
  assert.match(s.recent[0]!.at, /^\d\d:\d\d:\d\d$/);
  assert.equal(s.problem, null);
});

test("a new cast clears the last one's quality, position and problem", () => {
  let s = lines.reduce(fold, initialStatus);
  s = fold(s, `{"level":"WARN","message":"player event","event":"load_failed","gen":1,"code":"E404","error":"not_found","detail":"HTTP error 404 Not Found"}`);
  assert.equal(s.problem, "E404 not_found: HTTP error 404 Not Found");
  assert.equal(s.recent.at(-1)?.tone, "warn");
  s = fold(s, lines[2]!);
  assert.deepEqual([s.quality, s.position, s.problem, s.now?.from], [null, null, null, null]);
});

test("any coded error reads the same, with the summary when there's no detail", () => {
  const s = fold(initialStatus, `{"level":"ERROR","message":"can't start the player","code":"E101","error":"player_missing","summary":"the player isn't installed or can't start"}`);
  assert.equal(s.problem, "E101 player_missing: the player isn't installed or can't start");
});

test("refusals and notes show in the list", () => {
  let s = fold(initialStatus, `{"level":"INFO","message":"control refused","action":"Seek","code":"E605","error":"seek_not_supported","summary":"this video can't be seeked (a live stream)"}`);
  assert.equal(s.recent.at(-1)?.text, "refused Seek: E605 seek_not_supported: this video can't be seeked (a live stream)");
  s = fold(s, `{"level":"INFO","message":"note","text":"opened the log"}`);
  assert.deepEqual([s.recent.at(-1)?.text, s.problem], ["opened the log", null]);
  s = fold(s, `{"level":"WARN","message":"note","text":"couldn't write a report"}`);
  assert.equal(s.problem, "couldn't write a report");
});

test("a closed player says how to get it back", () => {
  const s = fold(initialStatus, `{"level":"INFO","message":"player event","event":"closed","gen":1}`);
  assert.equal(s.recent.at(-1)?.text, "player closed: Play opens it again");
});

test("helpers", () => {
  assert.equal(appOf("Bilibili/7.62.0 (Linux; Android 14)"), "Bilibili 7.62.0");
  assert.equal(appOf(""), "");
  assert.equal(clock(724), "12:04");
  assert.equal(clock(3723), "1:02:03");
});

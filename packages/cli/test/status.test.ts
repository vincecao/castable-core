import { test } from "node:test";
import assert from "node:assert/strict";
import { fold, initialStatus } from "../src/status.ts";

// Real lines from castable-receiver --log json.
const lines = [
  `{"level":"INFO","message":"ready","name":"My Mac","player":"mpv","control":"full","url":"http://192.168.31.134:49152/desc.xml"}`,
  `{"level":"INFO","message":"control","action":"SetAVTransportURI"}`,
  `{"level":"INFO","message":"player command","command":"load","gen":1,"url":"https://x/index.m3u8","title":"后会友期","mime":"video/*","live":false}`,
  `{"level":"INFO","message":"transport","from":"NO_MEDIA_PRESENT","to":"TRANSITIONING","gen":1}`,
  `{"level":"INFO","message":"player command","command":"seek","to_s":30.0}`,
  `not json at all`,
  `{"level":"INFO","message":"transport","from":"TRANSITIONING","to":"PLAYING","gen":1}`,
];

test("folds a real cast into the status screen", () => {
  const s = lines.reduce(fold, initialStatus);
  assert.equal(s.ready?.name, "My Mac");
  assert.equal(s.state, "PLAYING");
  assert.equal(s.now?.title, "后会友期");
  assert.deepEqual(s.recent, ["cast: 后会友期", "NO_MEDIA_PRESENT → TRANSITIONING", "seek to 30s", "TRANSITIONING → PLAYING"]);
  assert.equal(s.problem, null);
});

test("a failure shows until the next cast", () => {
  let s = fold(
    initialStatus,
    `{"level":"WARN","message":"player event","event":"load_failed","gen":1,"code":"E404","error":"not_found","category":"media","detail":"HTTP error 404 Not Found"}`,
  );
  assert.equal(s.problem, "E404 not_found: HTTP error 404 Not Found");
  assert.equal(s.recent.at(-1), "failed: E404 not_found: HTTP error 404 Not Found");
  s = fold(s, lines[2]!);
  assert.equal(s.problem, null);
});

test("without a title, the file name stands in", async () => {
  const { titleOf } = await import("../src/status.ts");
  assert.equal(titleOf("", "http://x/movies/%E5%90%8E%E4%BC%9A.mp4?t=1"), "后会");
  assert.equal(titleOf("", "http://x/"), "http://x/");
  assert.equal(titleOf("Episode 1", "http://x/a.mp4"), "Episode 1");
});

test("a closed player says how to get it back", () => {
  const s = fold(initialStatus, `{"level":"INFO","message":"player event","event":"closed","gen":1}`);
  assert.equal(s.recent.at(-1), "player closed: Play opens it again");
});

test("any coded error reads the same, with the summary when there's no detail", () => {
  const s = fold(initialStatus, `{"level":"ERROR","message":"can't start the player","code":"E101","error":"player_missing","summary":"the player isn't installed or can't start"}`);
  assert.equal(s.problem, "E101 player_missing: the player isn't installed or can't start");
});

test("the ready line carries the log file; shortcut notes show in the list", () => {
  let s = fold(initialStatus, `{"level":"INFO","message":"ready","name":"Mac","player":"mpv","control":"full","url":"http://x/desc.xml","log_file":"/u/.config/castable/logs/receiver.log"}`);
  assert.equal(s.ready?.logFile, "/u/.config/castable/logs/receiver.log");
  s = fold(s, `{"level":"INFO","message":"note","text":"opened the log: /u/.config/castable/logs/receiver.log"}`);
  assert.equal(s.recent.at(-1), "opened the log: /u/.config/castable/logs/receiver.log");
  assert.equal(s.problem, null);
  s = fold(s, `{"level":"WARN","message":"note","text":"couldn't write a report"}`);
  assert.equal(s.problem, "couldn't write a report");
});


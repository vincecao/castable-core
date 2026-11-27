#!/bin/sh
# Makes a fake test video, so testing never needs real content:
# a moving test pattern with a counter and a 440 Hz tone. Needs ffmpeg
# (macOS: brew install ffmpeg).
#
#   scripts/make-sample.sh            # samples/sample.mp4 (720p, 30 s, H.264 + AAC)
#   scripts/make-sample.sh --hls      # also samples/hls/index.m3u8 (HLS, 6 s segments)
#
# Then serve it and cast it:
#   python3 -m http.server 8000 --directory samples &
#   scripts/fake-sender.sh cast "http://$(ipconfig getifaddr en0):8000/sample.mp4"
set -eu
cd "$(dirname "$0")/.."
command -v ffmpeg >/dev/null 2>&1 || { echo "ffmpeg is needed: brew install ffmpeg (or your package manager)"; exit 1; }
mkdir -p samples
ffmpeg -loglevel error -y \
  -f lavfi -i "testsrc2=size=1280x720:rate=30:duration=30" \
  -f lavfi -i "sine=frequency=440:sample_rate=48000:duration=30" \
  -c:v libx264 -pix_fmt yuv420p -preset veryfast -c:a aac -ac 2 -b:a 128k -movflags +faststart \
  -metadata title="Castable test pattern" samples/sample.mp4
echo "samples/sample.mp4"
if [ "${1:-}" = "--hls" ]; then
  mkdir -p samples/hls
  ffmpeg -loglevel error -y -i samples/sample.mp4 -c copy -f hls -hls_time 6 -hls_playlist_type vod \
    -hls_segment_filename "samples/hls/segment-%03d.ts" samples/hls/index.m3u8
  echo "samples/hls/index.m3u8"
fi

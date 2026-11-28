#!/bin/sh
# Acts like a phone's cast button, so the receiver can be tested without one.
#
#   scripts/fake-sender.sh cast http://192.0.2.20:8000/video.mp4
#   scripts/fake-sender.sh play | pause | stop | status
#   scripts/fake-sender.sh seek 0:01:30
#
# RECEIVER defaults to http://localhost:49494 (Castable's own port).
set -eu
RECEIVER="${RECEIVER:-http://localhost:49494}"

soap() {
  curl -sS "$RECEIVER/ctl/avt" -H 'Content-Type: text/xml; charset="utf-8"' \
    -H "SOAPACTION: \"urn:schemas-upnp-org:service:AVTransport:1#$1\"" \
    -d "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"><s:Body><u:$1 xmlns:u=\"urn:schemas-upnp-org:service:AVTransport:1\"><InstanceID>0</InstanceID>$2</u:$1></s:Body></s:Envelope>"
  echo
}

# XML-escapes a value (a URL's & must travel as &amp;, as a real phone sends it).
xml_escape() {
  printf '%s' "$1" | sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g'
}

case "${1:-}" in
  cast) soap SetAVTransportURI "<CurrentURI>$(xml_escape "${2:?video URL}")</CurrentURI><CurrentURIMetaData></CurrentURIMetaData>" ;;
  play) soap Play "<Speed>1</Speed>" ;;
  pause) soap Pause "" ;;
  stop) soap Stop "" ;;
  seek) soap Seek "<Unit>REL_TIME</Unit><Target>${2:?time like 0:01:30}</Target>" ;;
  status) soap GetTransportInfo "" ;;
  *) sed -n '2,9p' "$0"; exit 1 ;;
esac

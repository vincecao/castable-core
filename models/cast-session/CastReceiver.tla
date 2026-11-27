---------------------------- MODULE CastReceiver ----------------------------
\* The receiver's cast session: a sender (phone app) sets a URL, presses
\* play/pause/stop; the player loads media asynchronously and can fail.
\* Fixed = FALSE models the naive design, Fixed = TRUE the planned design.
EXTENDS Naturals, FiniteSets

CONSTANT Fixed
Casts == {1, 2}            \* two casts in a row (cast 2 replaces cast 1)

VARIABLES
  uri,       \* current AVTransportURI (0 = none)
  state,     \* TransportState reported to senders
  inflight,  \* casts the player is still loading
  shown,     \* cast whose media is on screen (0 = none)
  playing,   \* is the player actually playing
  asked,     \* sender pressed Play for the current cast and has not paused/stopped since
  sent       \* casts already sent (bounds the model)

vars == <<uri, state, inflight, shown, playing, asked, sent>>

States == {"NoMedia", "Stopped", "Transitioning", "Playing", "Paused"}

TypeOK ==
  /\ uri \in {0} \cup Casts
  /\ state \in States
  /\ inflight \subseteq Casts
  /\ shown \in {0} \cup Casts
  /\ playing \in BOOLEAN
  /\ asked \in BOOLEAN
  /\ sent \subseteq Casts

Init ==
  /\ uri = 0 /\ state = "NoMedia" /\ inflight = {} /\ shown = 0
  /\ playing = FALSE /\ asked = FALSE /\ sent = {}

\* Sender sends SetAVTransportURI for a new cast.
SenderCasts(c) ==
  /\ c \notin sent
  /\ sent' = sent \cup {c}
  /\ uri' = c
  /\ state' = "Transitioning"
  /\ asked' = FALSE
  /\ IF Fixed
       THEN /\ inflight' = {c}          \* cancel any older load
            /\ shown' = 0 /\ playing' = FALSE
       ELSE /\ inflight' = inflight \cup {c}
            /\ UNCHANGED <<shown, playing>>

\* Sender sends Play.
SenderPlays ==
  /\ uri # 0
  /\ state # "Playing"
  /\ asked' = TRUE
  /\ UNCHANGED <<uri, sent>>
  /\ IF shown = uri /\ uri \notin inflight
       THEN /\ playing' = TRUE /\ state' = "Playing"
            /\ UNCHANGED <<inflight, shown>>
       ELSE IF Fixed
         THEN /\ state' = "Transitioning"   \* remember the Play; reload if needed
              /\ inflight' = {uri}
              /\ UNCHANGED <<shown, playing>>
         ELSE UNCHANGED <<state, inflight, shown, playing>>  \* 701: rejected

SenderPauses ==
  /\ state = "Playing"
  /\ playing' = FALSE /\ state' = "Paused" /\ asked' = FALSE
  /\ UNCHANGED <<uri, sent, inflight, shown>>

SenderStops ==
  /\ uri # 0
  /\ state \notin {"Stopped", "NoMedia"}
  /\ playing' = FALSE /\ state' = "Stopped" /\ asked' = FALSE
  /\ inflight' = IF Fixed THEN {} ELSE inflight
  /\ UNCHANGED <<uri, sent, shown>>

LoadDone(c) ==
  /\ c \in inflight
  /\ inflight' = inflight \ {c}
  /\ shown' = c
  /\ UNCHANGED <<uri, sent, asked>>
  /\ IF Fixed /\ asked
       THEN playing' = TRUE /\ state' = "Playing"
       ELSE playing' = FALSE /\ state' = "Stopped"

LoadFails(c) ==
  /\ c \in inflight
  /\ inflight' = inflight \ {c}
  /\ UNCHANGED <<uri, sent, shown, playing>>
  /\ IF Fixed
       THEN state' = "Stopped" /\ asked' = FALSE
       ELSE UNCHANGED <<state, asked>>

VideoEnds ==
  /\ playing
  /\ playing' = FALSE /\ state' = "Stopped" /\ asked' = FALSE
  /\ UNCHANGED <<uri, sent, inflight, shown>>

Next ==
  \/ \E c \in Casts : SenderCasts(c)
  \/ SenderPlays
  \/ SenderPauses
  \/ SenderStops
  \/ \E c \in Casts : LoadDone(c)
  \/ \E c \in Casts : LoadFails(c)
  \/ VideoEnds

Spec == Init /\ [][Next]_vars

\* ---- Rules the user agrees to ----

\* What senders see matches what is on screen.
ScreenMatchesState == playing <=> state = "Playing"

\* A replaced cast never stays on screen.
NeverShowsOldCast == shown \in {0, uri}

\* If you pressed Play, the video plays once it has loaded.
PlayIsNeverLost == (asked /\ shown = uri /\ uri \notin inflight) => playing

\* "Loading" is shown only while something is really loading.
NeverStuckLoading == state = "Transitioning" => uri \in inflight
=============================================================================

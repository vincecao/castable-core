---------------------------- MODULE CastSystem ----------------------------
\* The whole Castable receiver plan, end to end (docs/ARCHITECTURE.md in both
\* repos): a phone and the receiver's own screen send commands to the core
\* session; the core sends PlayerCommands to a player over a queue; the player
\* reports PlayerEvents back over another queue; the screen shows the core's
\* view after a delay. models/cast-session checks the session alone; this
\* model checks how the parts work together.
\*
\*   Kind = "process"  an external player (mpv, VLC): its process exits when
\*                     the video ends or you close its window
\*   Kind = "web"      @castable/player-web: the <video> stays loaded
\*   Fixed = TRUE      the code as it is (current-*.cfg)
\*   Fixed = FALSE     the design before the three fixes this model found
\*                     (before-fixes-*.cfg); kept so CI shows the model still
\*                     catches those bugs
EXTENDS Naturals, Sequences

CONSTANTS Kind, Fixed, MaxGen, MaxQ

VARIABLES
  phase,    \* core: "idle" "loading" "stopped" "playing" "paused" "unloaded"
  then,     \* core, while loading: "hold" or "play"
  gen,      \* core: the current cast's generation (0 = none)
  lastGen,  \* core: the newest generation handed out
  cmdQ,     \* PlayerCommands in flight, core -> player
  evQ,      \* PlayerEvents in flight, player -> core
  pGen,     \* player: the generation it has loaded
  pState,   \* player: "none" "loading" "paused" "playing" "gone"
  uiGen,    \* screen: the cast it shows
  uiPhase,  \* screen: the phase it shows
  uiQ,      \* screen presses in flight to the core: [k, g] (g = the cast shown)
  misapplied \* a press made on one cast was applied to another

vars == <<phase, then, gen, lastGen, cmdQ, evQ, pGen, pState, uiGen, uiPhase, uiQ, misapplied>>
player == <<pGen, pState>>
screen == <<uiGen, uiPhase>>

Cmd(c, g) == [c |-> c, g |-> g]
Loaded == {"stopped", "playing", "paused"}

Init ==
  /\ phase = "idle" /\ then = "hold" /\ gen = 0 /\ lastGen = 0
  /\ cmdQ = <<>> /\ evQ = <<>>
  /\ pGen = 0 /\ pState = "none"
  /\ uiGen = 0 /\ uiPhase = "idle" /\ uiQ = <<>>
  /\ misapplied = FALSE

\* ---- The core session (crates/castable-core/src/session.rs) ----

\* The result of a sender command k: <<phase, then, gen, lastGen, commands>>,
\* or "refused".
Do(k) ==
  CASE phase = "idle" -> "refused"
    [] k = "play" /\ phase = "loading" -> <<"loading", "play", gen, lastGen, <<>>>>
    [] k = "play" /\ phase \in {"stopped", "paused"} -> <<"playing", then, gen, lastGen, <<Cmd("play", gen)>>>>
    [] k = "play" /\ phase = "playing" -> <<phase, then, gen, lastGen, <<>>>>
    [] k = "play" /\ phase = "unloaded" -> <<"loading", "play", lastGen + 1, lastGen + 1, <<Cmd("load", lastGen + 1)>>>>
    [] k = "pause" /\ phase = "loading" -> <<"loading", "hold", gen, lastGen, <<>>>>
    [] k = "pause" /\ phase = "playing" -> <<"paused", then, gen, lastGen, <<Cmd("pause", gen)>>>>
    [] k = "pause" /\ phase = "paused" -> <<phase, then, gen, lastGen, <<>>>>
    [] k = "pause" -> "refused"
    [] k = "stop" /\ phase = "loading" -> <<"unloaded", then, gen, lastGen, <<Cmd("stop", gen)>>>>
    [] k = "stop" /\ phase \in {"playing", "paused"} -> <<"stopped", then, gen, lastGen, <<Cmd("stop", gen)>>>>
    [] k = "stop" -> <<phase, then, gen, lastGen, <<>>>>
    [] k = "seek" /\ phase = "loading" -> <<phase, then, gen, lastGen, <<>>>>
    [] k = "seek" /\ phase \in Loaded -> <<phase, then, gen, lastGen, <<Cmd("seek", gen)>>>>
    [] k = "seek" -> "refused"

Apply(r) ==
  /\ phase' = r[1] /\ then' = r[2] /\ gen' = r[3] /\ lastGen' = r[4]
  /\ cmdQ' = cmdQ \o r[5]

\* The phone's SOAP call is answered at once (a refusal changes nothing).
PhoneCasts ==
  /\ lastGen < MaxGen
  /\ phase' = "loading" /\ then' = "hold" /\ gen' = lastGen + 1 /\ lastGen' = lastGen + 1
  /\ cmdQ' = Append(cmdQ, Cmd("load", lastGen + 1))
  /\ UNCHANGED <<evQ, player, screen, uiQ, misapplied>>

PhoneSends(k) ==
  /\ Do(k) # "refused"
  /\ Apply(Do(k))
  /\ UNCHANGED <<evQ, player, screen, uiQ, misapplied>>

PhonePlays == PhoneSends("play")
PhonePauses == PhoneSends("pause")
PhoneStops == PhoneSends("stop")

\* A press on the receiver's own screen (packages/ui). Fix 1: it is a
\* ScreenCommand naming the cast shown, and the core drops it if that cast
\* has been replaced. Before: a plain SenderCommand.
CorePress ==
  /\ uiQ # <<>>
  /\ LET k == Head(uiQ)[1]
         g == Head(uiQ)[2]
         stale == g # gen
     IN IF Fixed /\ stale
        THEN /\ UNCHANGED <<phase, then, gen, lastGen, cmdQ, misapplied>>
        ELSE IF Do(k) = "refused"
             THEN UNCHANGED <<phase, then, gen, lastGen, cmdQ, misapplied>>
             ELSE /\ Apply(Do(k))
                  /\ misapplied' = (misapplied \/ stale)
  /\ uiQ' = Tail(uiQ)
  /\ UNCHANGED <<evQ, player, screen>>

\* A player event; events from an older generation are dropped.
CoreEvent ==
  /\ evQ # <<>>
  /\ LET e == Head(evQ)
     IN IF e.g # gen THEN UNCHANGED <<phase, then, gen, lastGen, cmdQ>>
        ELSE /\ (CASE e.e = "loaded" /\ phase = "loading" ->
                    IF then = "play"
                    THEN phase' = "playing" /\ cmdQ' = Append(cmdQ, Cmd("play", gen))
                    ELSE phase' = "stopped" /\ cmdQ' = cmdQ
               [] e.e = "failed" /\ phase = "loading" -> phase' = "unloaded" /\ cmdQ' = cmdQ
               [] e.e = "failed" /\ phase \in Loaded ->
                    phase' = "unloaded" /\ cmdQ' = Append(cmdQ, Cmd("stop", gen))
               [] e.e = "ended" /\ phase = "playing" ->
                    \* Fix 3: the core also sends Stop, so a Play that crossed
                    \* the end on its way can't leave the video running.
                    phase' = "stopped" /\ cmdQ' = IF Fixed THEN Append(cmdQ, Cmd("stop", gen)) ELSE cmdQ
               [] e.e = "closed" /\ phase \in Loaded \cup {"loading"} ->
                    \* Fix 2: the player is gone, so nothing is loaded; Play
                    \* loads it again.
                    phase' = "unloaded" /\ cmdQ' = cmdQ
               [] OTHER -> UNCHANGED <<phase, cmdQ>>)
             /\ UNCHANGED <<then, gen, lastGen>>
  /\ evQ' = Tail(evQ)
  /\ UNCHANGED <<player, screen, uiQ, misapplied>>

\* ---- The player (castable-players, @castable/player-web) ----

PlayerTakes ==
  /\ cmdQ # <<>>
  /\ LET c == Head(cmdQ)
     IN CASE c.c = "load" -> pGen' = c.g /\ pState' = "loading"
          [] c.c = "play" /\ pState \in {"paused", "playing"} -> pState' = "playing" /\ pGen' = pGen
          [] c.c = "pause" /\ pState = "playing" -> pState' = "paused" /\ pGen' = pGen
          [] c.c = "stop" /\ pState = "playing" -> pState' = "paused" /\ pGen' = pGen
          [] OTHER -> UNCHANGED player   \* includes any command to a player that is gone
  /\ cmdQ' = Tail(cmdQ)
  /\ UNCHANGED <<phase, then, gen, lastGen, evQ, screen, uiQ, misapplied>>

Report(e) == evQ' = Append(evQ, [e |-> e, g |-> pGen])

LoadDone ==
  /\ pState = "loading" /\ pState' = "paused" /\ Report("loaded")
  /\ UNCHANGED <<phase, then, gen, lastGen, cmdQ, pGen, screen, uiQ, misapplied>>

LoadFails ==
  /\ pState = "loading" /\ pState' = "none" /\ Report("failed")
  /\ UNCHANGED <<phase, then, gen, lastGen, cmdQ, pGen, screen, uiQ, misapplied>>

\* An external player reports the end, then its process exits (mpv runs with
\* --keep-open=no). Before fix 2 a clean exit was reported as ended too.
VideoEnds ==
  /\ pState = "playing"
  /\ pState' = IF Kind = "process" THEN "gone" ELSE "paused"
  /\ evQ' = IF Kind = "process" /\ Fixed
            THEN evQ \o <<[e |-> "ended", g |-> pGen], [e |-> "closed", g |-> pGen]>>
            ELSE Append(evQ, [e |-> "ended", g |-> pGen])
  /\ UNCHANGED <<phase, then, gen, lastGen, cmdQ, pGen, screen, uiQ, misapplied>>

\* You close the external player's window: a clean exit, reported as closed
\* (before fix 2: as ended).
WindowClosed ==
  /\ Kind = "process" /\ pState \in {"paused", "playing"}
  /\ pState' = "gone" /\ Report(IF Fixed THEN "closed" ELSE "ended")
  /\ UNCHANGED <<phase, then, gen, lastGen, cmdQ, pGen, screen, uiQ, misapplied>>

\* ---- The screen (packages/ui) ----

ViewArrives ==
  /\ <<uiGen, uiPhase>> # <<gen, phase>>
  /\ uiGen' = gen /\ uiPhase' = phase
  /\ UNCHANGED <<phase, then, gen, lastGen, cmdQ, evQ, player, uiQ, misapplied>>

ScreenPress(k) ==
  /\ uiQ = <<>>
  /\ uiQ' = <<<<k, uiGen>>>>
  /\ UNCHANGED <<phase, then, gen, lastGen, cmdQ, evQ, player, screen, misapplied>>

YouPressPlay == uiPhase \in {"stopped", "paused", "unloaded"} /\ ScreenPress("play")
YouPressPause == uiPhase = "playing" /\ ScreenPress("pause")
YouSeek == uiPhase \in Loaded /\ ScreenPress("seek")

Next ==
  \/ PhoneCasts \/ PhonePlays \/ PhonePauses \/ PhoneStops
  \/ CorePress \/ CoreEvent
  \/ PlayerTakes \/ LoadDone \/ LoadFails \/ VideoEnds \/ WindowClosed
  \/ ViewArrives \/ YouPressPlay \/ YouPressPause \/ YouSeek

Spec == Init /\ [][Next]_vars

Bounded == Len(cmdQ) <= MaxQ /\ Len(evQ) <= MaxQ /\ lastGen <= MaxGen

\* ---- Rules ----

Settled == cmdQ = <<>> /\ evQ = <<>>

\* Once every message has arrived, the video plays exactly when the core
\* (and so every phone and the screen) says it plays, and it is the current cast.
PlaysWhatCoreSays ==
  Settled => ((phase = "playing") <=> (pState = "playing" /\ pGen = gen))

\* A press on the screen acts on the cast you saw when you pressed it.
PressActsOnItsCast == ~misapplied

=============================================================================

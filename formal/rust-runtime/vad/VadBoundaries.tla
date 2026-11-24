-------------------------- MODULE VadBoundaries --------------------------
EXTENDS Integers, Sequences

CONSTANT Fault
VARIABLES scenario, phase, inferred, i, candidate, speech, quiet, regions
vars == <<scenario, phase, inferred, i, candidate, speech, quiet, regions>>

Cases == {"short", "minimum", "brokenCandidate", "gaps", "silence"}
Samples == CASE scenario = "short" -> 2639 [] scenario = "minimum" -> 2640
             [] scenario = "brokenCandidate" -> 3120 [] scenario = "gaps" -> 28720
             [] OTHER -> 16000
Frames == 1 + (Samples - 400) \div 160
\* The filter and network are outside this model. For "gaps", these are the
\* thresholded flags after causal five-frame smoothing of the ONNX test sequence.
Voiced(k) == CASE scenario = "gaps" -> k \in (22..35 \cup 56..70 \cup 100..114 \cup 145..159)
               [] scenario = "brokenCandidate" -> k \in (0..7 \cup 9..17)
               [] scenario = "silence" -> FALSE
               [] OTHER -> TRUE
Expected == CASE scenario = "minimum" -> <<<<0,2640>>>>
              [] scenario = "gaps" -> <<<<7040,20320>>, <<21280,28720>>>>
              [] OTHER -> <<>>
Min(a,b) == IF a < b THEN a ELSE b
Max(a,b) == IF a > b THEN a ELSE b
Region(a,b) == <<Max(0, 160*a - 1920), Min(Samples, 160*b + 1920)>>

Init == /\ scenario \in Cases /\ phase = "start" /\ inferred = FALSE
        /\ i = 0 /\ candidate = -1 /\ speech = -1 /\ quiet = -1 /\ regions = <<>>
Admit == /\ phase = "start"
         /\ LET eligible == Samples >= (IF Fault = "earlyWork" THEN 2639 ELSE 2640) IN
                /\ inferred' = eligible /\ phase' = IF eligible THEN "scan" ELSE "done"
         /\ UNCHANGED <<scenario,i,candidate,speech,quiet,regions>>

Search == /\ speech = -1
          /\ LET start == IF candidate = -1 THEN i ELSE candidate IN
              /\ candidate' = IF Voiced(i) THEN start
                              ELSE IF Fault = "staleCandidate" THEN candidate ELSE -1
              /\ speech' = IF Voiced(i) /\ i-start+1 >= 15
                           THEN IF Fault = "lateStart" THEN i ELSE start
                           ELSE -1
          /\ UNCHANGED <<quiet,regions>>
Continue == /\ speech # -1 /\ Voiced(i)
            /\ quiet' = IF Fault = "staleSilence" THEN quiet ELSE -1
            /\ UNCHANGED <<candidate,speech,regions>>
Pause == /\ speech # -1 /\ ~Voiced(i)
         /\ IF quiet # -1 /\ i-quiet+1 >= (IF Fault = "shortGap" THEN 29 ELSE 30)
            THEN /\ regions' = Append(regions,Region(speech,quiet))
                 /\ candidate' = -1 /\ speech' = -1 /\ quiet' = -1
            ELSE /\ quiet' = IF quiet = -1 THEN i ELSE quiet
                 /\ UNCHANGED <<candidate,speech,regions>>
Scan == /\ phase = "scan" /\ i < Frames /\ i' = i+1
        /\ (Search \/ Continue \/ Pause)
        /\ UNCHANGED <<scenario,phase,inferred>>
Finish == /\ phase = "scan" /\ i = Frames /\ phase' = "done"
          /\ LET last == IF Fault = "truncateTail" /\ quiet # -1 THEN quiet ELSE Frames IN
             regions' = IF speech # -1 THEN Append(regions,Region(speech,last)) ELSE regions
          /\ UNCHANGED <<scenario,i,inferred,candidate,speech,quiet>>
Next == Admit \/ Scan \/ Finish \/ (phase = "done" /\ UNCHANGED vars)
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)
Types == /\ scenario \in Cases /\ phase \in {"start","scan","done"} /\ inferred \in BOOLEAN
         /\ i \in 0..Frames /\ candidate \in -1..Frames /\ speech \in -1..Frames
         /\ quiet \in -1..Frames /\ regions \in Seq((0..Samples) \X (0..Samples))
EligibleWork == phase # "start" => inferred = (Samples >= 2640)
ExactRegions == phase = "done" => regions = Expected
Terminates == <>(phase = "done")
=============================================================================

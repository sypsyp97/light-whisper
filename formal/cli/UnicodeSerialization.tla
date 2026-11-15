---------------------- MODULE UnicodeSerialization ----------------------
EXTENDS Integers, Sequences

(* Bounded, pure decoded-string -> JSON -> UTF-8 pipe -> decoded-string model.
   Code/test binding and abstraction limits: UnicodeSerialization.md.
   Each token represents one Python scalar character or one unpaired surrogate.
   An escaped token abbreviates an ASCII JSON escape (two escapes for a scalar
   supplementary character); ASCII characters remain literal in both modes.
   JSON punctuation/mandatory quote and backslash escaping are abstracted away.
   This is not a send-worker, native-backend acceptance or arbitrary-JSON proof.
*)

CONSTANTS EnsureASCII, Witness
VARIABLES phase, inputClass, location, source, encoded, piped, decoded
vars == <<phase, inputClass, location, source, encoded, piped, decoded>>

Classes == {"scalar", "high", "low", "mixed"}
Units == {"ascii", "bmp", "supplementary", "high", "low", "question"}
Surrogates == {"high", "low"}
Tokens == [form : {"escaped", "literal"}, unit : Units]
Inputs(kind) ==
    CASE kind = "scalar" -> {<<>>, <<"ascii", "bmp", "supplementary", "question">>}
      [] kind = "high" -> {<<"high">>}
      [] kind = "low" -> {<<"low">>}
      [] kind = "mixed" -> {<<"bmp", "high", "ascii", "low", "supplementary">>}

Init ==
    /\ phase = "input"
    /\ inputClass \in Classes
    /\ location \in {"value", "key"}
    /\ source \in Inputs(inputClass)
    /\ encoded = <<>> /\ piped = <<>> /\ decoded = <<>>

Encode ==
    /\ phase = "input"
    /\ encoded' = [i \in 1..Len(source) |->
          [form |-> IF EnsureASCII /\ source[i] \notin {"ascii", "question"}
                    THEN "escaped" ELSE "literal", unit |-> source[i]]]
    /\ phase' = "encoded"
    /\ UNCHANGED <<inputClass, location, source, piped, decoded>>

Pipe ==
    /\ phase = "encoded"
    /\ piped' = [i \in 1..Len(encoded) |->
          IF encoded[i].form = "literal" /\ encoded[i].unit \in Surrogates
          THEN [encoded[i] EXCEPT !.unit = "question"] ELSE encoded[i]]
    /\ phase' = "piped"
    /\ UNCHANGED <<inputClass, location, source, encoded, decoded>>

Decode ==
    /\ phase = "piped"
    /\ decoded' = [i \in 1..Len(piped) |-> piped[i].unit]
    /\ phase' = "done"
    /\ UNCHANGED <<inputClass, location, source, encoded, piped>>

Done == /\ phase = "done" /\ UNCHANGED vars
Next == Encode \/ Pipe \/ Decode \/ Done

TypeOK ==
    /\ EnsureASCII \in BOOLEAN
    /\ Witness \in Classes \cup {"none"}
    /\ phase \in {"input", "encoded", "piped", "done"}
    /\ inputClass \in Classes /\ location \in {"value", "key"}
    /\ source \in Inputs(inputClass)
    /\ encoded \in Seq(Tokens) /\ Len(encoded) <= 5
    /\ piped \in Seq(Tokens) /\ Len(piped) <= 5
    /\ decoded \in Seq(Units) /\ Len(decoded) <= 5

DecodedStringsPreserved == phase = "done" => decoded = source
WitnessNotReached == ~(phase = "done" /\ inputClass = Witness)
=============================================================================

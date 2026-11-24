"""Observe preview suppression, publication and adaptive cadence through JSON stats and text."""
import time

from engine_protocol import match, pcm64, require, stream_reply
from engine_scenario import run_scenario


def preview(wire,args,path,pcm,report):
    require(args.engine == "qwen3-asr-0.6b","Qwen3 preview is engine-specific")
    request = wire.request
    req_id = 1
    request("stream_start",session_id=req_id, language="zh")
    offset = 0
    # Repeat the same natural utterance to cross the four-second cadence change.
    source = pcm * (75000 // (len(pcm)//2) + 2)
    def feed(end, enabled=True):
        nonlocal offset
        require(end > offset, "feed must advance raw PCM")
        reply = request("stream_feed",session_id=req_id,offset=offset,sample_rate=16000,audio_format="pcm_s16le",
                        audio_base64=pcm64(source[2*offset:2*end]),preview=enabled)
        stream_reply(reply,args.engine,args.backend,req_id,end,False)
        match(reply,text="")
        offset = end
        return reply

    def calls():
        return request("stats")["stats"]["vad_calls"]

    def wait_call(expected):
        deadline = time.monotonic()+30
        while time.monotonic() < deadline:
            seen = calls()
            if seen == expected:
                return
            require(seen < expected, f"extra VAD work: expected {expected}, got {seen}")
            time.sleep(0.05)
        raise AssertionError(f"preview never invoked VAD for expected call {expected} at {offset} samples")

    before = calls()
    match(feed(1919),tentative_text="")
    require(calls() == before,"early startup preview")
    feed(1920)
    wait_call(before+1)
    # Finish drains the exact covering preview, including an in-flight decode.
    short = request("stream_finish", session_id=req_id)
    stream_reply(short,args.engine,args.backend,req_id,1920,True)
    require(calls() == before+1,"covering final duplicated startup inference")
    req_id = 2
    offset = 0
    request("stream_start",session_id=req_id,language="zh")
    match(feed(61000,False),tentative_text="")
    require(calls() == before+1,"suppressed PCM invoked VAD")
    feed(61001)
    wait_call(before+2)
    # Poll via valid suppressed one-sample feeds so that waiting never queues another preview.
    deadline = time.monotonic()+30
    while time.monotonic() < deadline:
        result = feed(offset+1,False)
        if result["tentative_text"].strip():
            break
        time.sleep(0.05)
    else:
        raise AssertionError("real speech never produced a tentative transcript")
    require(offset < 64000, "preview polling unexpectedly exhausted the pre-cutoff interval")
    feed(64201)
    require(calls() == before+2,"3200 samples incorrectly triggered after 4 seconds")
    feed(66121)
    wait_call(before+3)
    final = request("stream_finish",session_id=req_id)
    stream_reply(final,args.engine,args.backend,req_id,66121,True)
    require(final["text"].strip(),"final speech transcript missing")
    require(calls() == before+3,"covering final repeated decode")
    report["transcripts"]["final"] = [final["text"],final["language"]]


if __name__ == "__main__":
    raise SystemExit(run_scenario("qwen-preview",preview))

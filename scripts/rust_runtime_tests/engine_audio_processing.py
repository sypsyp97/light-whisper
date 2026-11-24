"""Exercise the realtime processor along the actual file and streaming server paths."""
import time

from engine_protocol import match, pcm64, require, stream_reply
from engine_scenario import run_scenario


def audio_processing(wire,args,audio,pcm,report):
    request=wire.request
    n=len(pcm)//2
    def capture(label,session_id,enabled):
        request("stream_start",session_id=session_id,language="zh")
        match(request("set_audio_processing",enabled=enabled), audio_processing_enabled=enabled)
        request("set_audio_processing",False,enabled=not enabled)
        for offset in range(0,n,640):
            end=min(n,offset+640)
            payload=dict(session_id=session_id,offset=offset,sample_rate=16000,
                         audio_format="pcm_s16le",audio_base64=pcm64(pcm[offset*2:end*2]))
            if args.engine.startswith("qwen"):
                payload["preview"]=False
            reply=request("stream_feed",**payload)
            stream_reply(reply,args.engine,args.backend,session_id,end,False)
        result=request("stream_finish",session_id=session_id)
        stream_reply(result,args.engine,args.backend,session_id,n,True)
        require(result["text"].strip(),"empty realtime transcript")
        report["transcripts"][label]=[result["text"],result["language"]]
    request("set_audio_processing",False,enabled=1)
    match(request("set_audio_processing",enabled=True), audio_processing_enabled=True)
    def batch(label):
        result=request("transcribe",audio_path=str(audio),options={"language":"zh"})
        match(result,input_mode="path",engine=args.engine)
        require(result["text"].strip() and abs(result["duration"]-n/16000)<0.001,result)
        report["transcripts"][label]=[result["text"],result["language"]]
    batch("processed-file")
    # An aborted processed frame must be discarded before the following stream.
    request("stream_start",session_id=1)
    request("stream_feed",session_id=1,offset=0,sample_rate=16000,audio_format="pcm_s16le",
            audio_base64=pcm64(pcm[:6400]))
    cancelled=request("stream_cancel",session_id=1)
    stream_reply(cancelled,args.engine,args.backend,1,3200,True)
    capture("processed-stream",2,True)
    match(request("set_audio_processing",enabled=False),audio_processing_enabled=False)
    capture("unprocessed-stream",3,False)
    match(request("set_audio_processing",enabled=True),audio_processing_enabled=True)
    match(request("set_gpu_idle",seconds=1),enabled=True)
    deadline=time.monotonic()+15
    while time.monotonic()<deadline:
        if request("status").get("gpu_suspended") is True:
            break
        time.sleep(0.2)
    else:
        raise AssertionError("processed runtime did not suspend")
    match(request("set_gpu_idle",seconds=0),enabled=False)
    batch("processed-file-after-resume")
    require(report["transcripts"]["processed-file"]==report["transcripts"]["processed-file-after-resume"],
            "resumed enhanced offline transcript differs")
    capture("processed-stream-after-resume",4,True)
    require(report["transcripts"]["processed-stream"]==report["transcripts"]["processed-stream-after-resume"],
            "resumed enhanced stream transcript differs")
    match(request("stats")["stats"],transcription_count=5)


if __name__=="__main__":
    raise SystemExit(run_scenario("engine-audio-processing",audio_processing))

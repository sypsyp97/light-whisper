"""Production initialization must route streamed silence through VAD."""
import contextlib
import unittest
from unittest.mock import Mock,patch
import base64
import numpy as np
from r2t2_asr_server import R2T2ASRServer

class StreamWiringTests(unittest.TestCase):
    def test_cuda_uses_measured_160ms_chunks_and_shared_dll_directory(self):
        server = R2T2ASRServer.__new__(R2T2ASRServer)
        with patch('r2t2_asr_server.NativeRuntime') as runtime:
            server._load_runtime('verified-model')
        self.assertEqual(runtime.call_args.kwargs.get('chunk_ms'), 160)
        from pathlib import Path
        self.assertIn(Path(__file__).resolve().parent, runtime.call_args.kwargs['dll_directories'])

    def test_cpu_fallback_keeps_320ms_chunks(self):
        server = R2T2ASRServer.__new__(R2T2ASRServer)
        with patch('r2t2_asr_server.NativeRuntime', side_effect=[RuntimeError('CUDA unavailable'), Mock()]) as runtime:
            server._load_runtime('verified-model')
        self.assertEqual(runtime.call_args.kwargs.get('chunk_ms'), 320)
        self.assertEqual(server.backend, 'cpu')

    def test_initialized_stream_uses_vad_before_native_inference(self):
        native=Mock(chunk_samples=5120)
        native.feed.return_value=('',None);native.finish.return_value=('',None)
        vad=Mock();vad.speech_timestamps.return_value=[]
        with patch('signal.signal'),patch.object(R2T2ASRServer,'_setup_runtime_environment'),patch.object(R2T2ASRServer,'_detect_device',return_value='cpu'):
            server=R2T2ASRServer()
        server.stdout_suppressor=Mock();server.stdout_suppressor.suppress.side_effect=contextlib.nullcontext
        def load(_path): server.native=native
        with patch.object(server,'_resolve_model_path',return_value='verified-model'),patch.object(server,'_load_runtime',side_effect=load),patch('r2t2_asr_server.FireRedVad',return_value=vad):
            self.assertTrue(server.initialize()['success'])
        server.handle_stream_command({'action':'stream_start','session_id':1})
        result=server.handle_stream_command({'action':'stream_feed','session_id':1,'offset':0,'sample_rate':16000,'audio_format':'pcm_s16le','audio_base64':base64.b64encode(np.zeros(5120,dtype='<i2').tobytes()).decode()})
        self.assertTrue(result['success']);self.assertEqual(result['sample_count'],5120)
        final=server.handle_stream_command({'action':'stream_finish','session_id':1})
        self.assertTrue(final['success']);self.assertEqual(final['text'],'')
        vad.speech_timestamps.assert_called_once()
        native.start.assert_not_called();native.feed.assert_not_called();native.finish.assert_not_called()
        self.assertGreaterEqual(server.stdout_suppressor.suppress.call_count,4)

if __name__=='__main__': unittest.main()

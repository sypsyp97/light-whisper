#!/usr/bin/env python3
# -*- coding: utf-8 -*-

# Adapted from FireRedTeam/FireRedVAD, licensed under Apache-2.0.
# Modified by Light-Whisper contributors; see THIRD_PARTY_NOTICES.md.



from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path

import numpy as np


SAMPLE_RATE = 16_000
FRAME_SHIFT_MS = 10
FRAME_LENGTH_MS = 25
FRAME_SHIFT_SAMPLES = SAMPLE_RATE * FRAME_SHIFT_MS // 1000
FRAME_LENGTH_SAMPLES = SAMPLE_RATE * FRAME_LENGTH_MS // 1000
MODEL_FILENAME = "fireredvad_vad.onnx"
CMVN_FILENAME = "fireredvad_cmvn.json"


@dataclass(frozen=True)
class FireRedVadOptions:
    threshold: float = 0.5
    smooth_window_frames: int = 5
    min_speech_duration_ms: int = 150
    min_silence_duration_ms: int = 300
    speech_pad_ms: int = 120


def _resource_path(filename: str) -> Path:
    return Path(__file__).resolve().with_name(filename)


class FireRedVad:


    def __init__(
        self,
        model_path: str | Path | None = None,
        cmvn_path: str | Path | None = None,
        options: FireRedVadOptions | None = None,
    ):
        import kaldi_native_fbank as knf
        import onnxruntime as ort

        self.options = options or FireRedVadOptions()
        self.model_path = Path(model_path) if model_path else _resource_path(MODEL_FILENAME)
        self.cmvn_path = Path(cmvn_path) if cmvn_path else _resource_path(CMVN_FILENAME)

        if not self.model_path.is_file():
            raise FileNotFoundError(f"FireRedVAD 模型不存在: {self.model_path}")
        if not self.cmvn_path.is_file():
            raise FileNotFoundError(f"FireRedVAD CMVN 不存在: {self.cmvn_path}")

        cmvn = json.loads(self.cmvn_path.read_text(encoding="utf-8"))
        self._mean = np.asarray(cmvn["mean"], dtype=np.float32)
        self._inverse_std = np.asarray(cmvn["inverse_std"], dtype=np.float32)
        if self._mean.shape != (80,) or self._inverse_std.shape != (80,):
            raise ValueError("FireRedVAD CMVN 必须包含 80 维 mean 和 inverse_std")

        fbank_options = knf.FbankOptions()
        fbank_options.frame_opts.samp_freq = SAMPLE_RATE
        fbank_options.frame_opts.frame_length_ms = FRAME_LENGTH_MS
        fbank_options.frame_opts.frame_shift_ms = FRAME_SHIFT_MS
        fbank_options.frame_opts.dither = 0
        fbank_options.frame_opts.snip_edges = True
        fbank_options.mel_opts.num_bins = 80
        fbank_options.mel_opts.debug_mel = False
        self._knf = knf
        self._fbank_options = fbank_options
        self._feature_cache = None

        session_options = ort.SessionOptions()
        session_options.inter_op_num_threads = 1
        session_options.intra_op_num_threads = 1
        session_options.enable_cpu_mem_arena = False
        session_options.log_severity_level = 4
        self._session = ort.InferenceSession(
            str(self.model_path),
            providers=["CPUExecutionProvider"],
            sess_options=session_options,
        )

    def _extract_features(self, audio: np.ndarray) -> np.ndarray:
        cache = self._feature_cache

        self._feature_cache = None
        samples = np.asarray(audio, dtype=np.float32).reshape(-1)
        pcm = np.clip(samples * 32768.0, -32768.0, 32767.0)
        cacheable = pcm.size <= 12 * SAMPLE_RATE

        if (
            cacheable
            and cache is not None
            and pcm.size >= cache[0].size
            and np.array_equal(pcm[: cache[0].size], cache[0])
        ):
            previous_pcm, fbank, features = cache
            offset = previous_pcm.size
        else:
            fbank = self._knf.OnlineFbank(self._fbank_options)
            features = np.empty((0, 80), dtype=np.float32)
            offset = 0

        if pcm.size > offset:
            fbank.accept_waveform(SAMPLE_RATE, pcm[offset:].tolist())
        frame_count = fbank.num_frames_ready
        if frame_count > features.shape[0]:
            new_features = np.asarray(
                [fbank.get_frame(index) for index in range(features.shape[0], frame_count)],
                dtype=np.float32,
            )
            new_features = np.ascontiguousarray(
                (new_features - self._mean) * self._inverse_std,
                dtype=np.float32,
            )
            features = (
                np.concatenate((features, new_features)) if features.size else new_features
            )

        result = features.copy() if cacheable else features
        if cacheable:
            self._feature_cache = (pcm, fbank, features)
        return result

    def probabilities(self, audio: np.ndarray) -> np.ndarray:
        features = self._extract_features(audio)
        if features.shape[0] == 0:
            return np.empty(0, dtype=np.float32)
        output = self._session.run(None, {"feat": features[np.newaxis, :, :]})[0]
        return np.asarray(output, dtype=np.float32).reshape(-1)

    def warmup(self) -> None:
        self.probabilities(np.zeros(SAMPLE_RATE, dtype=np.float32))

    @property
    def _min_speech_frames(self):
        return max(1, self.options.min_speech_duration_ms // FRAME_SHIFT_MS)

    def speech_timestamps(self, audio: np.ndarray) -> list[dict[str, int]]:
        samples = np.asarray(audio, dtype=np.float32).reshape(-1)
        # With snip_edges, even continuous speech needs this many real samples.
        if samples.size < FRAME_LENGTH_SAMPLES + (self._min_speech_frames - 1) * FRAME_SHIFT_SAMPLES:
            self._feature_cache = None
            return []
        probabilities = self.probabilities(samples)
        return self._timestamps_from_probabilities(probabilities, len(samples))

    def _timestamps_from_probabilities(
        self,
        raw_probabilities: np.ndarray,
        audio_length_samples: int,
    ) -> list[dict[str, int]]:
        probabilities = np.asarray(raw_probabilities, dtype=np.float32).reshape(-1)
        if probabilities.size == 0:
            return []

        window = max(1, int(self.options.smooth_window_frames))
        if window > 1:
            kernel = np.ones(window, dtype=np.float32) / window
            smoothed = np.convolve(probabilities, kernel, mode="full")[: probabilities.size]
            for index in range(min(window - 1, probabilities.size)):
                smoothed[index] = probabilities[: index + 1].mean()
        else:
            smoothed = probabilities

        speech_flags = smoothed >= self.options.threshold
        min_speech_frames = self._min_speech_frames
        min_silence_frames = max(1, self.options.min_silence_duration_ms // 10)
        pad_samples = max(0, self.options.speech_pad_ms * SAMPLE_RATE // 1000)

        segments: list[tuple[int, int]] = []
        candidate_start: int | None = None
        speech_start: int | None = None
        silence_start: int | None = None

        for index, is_speech in enumerate(speech_flags):
            if speech_start is None:
                if is_speech:
                    if candidate_start is None:
                        candidate_start = index
                    if index - candidate_start + 1 >= min_speech_frames:
                        speech_start = candidate_start
                        silence_start = None
                else:
                    candidate_start = None
                continue

            if is_speech:
                silence_start = None
                continue

            if silence_start is None:
                silence_start = index
                continue

            if index - silence_start + 1 >= min_silence_frames:
                segments.append((speech_start, silence_start))
                speech_start = None
                candidate_start = None
                silence_start = None

        if speech_start is not None:
            segments.append((speech_start, probabilities.size))

        padded: list[dict[str, int]] = []
        for start_frame, end_frame in segments:
            start = max(0, start_frame * FRAME_SHIFT_SAMPLES - pad_samples)
            end = min(
                audio_length_samples,
                end_frame * FRAME_SHIFT_SAMPLES + pad_samples,
            )
            if end <= start:
                continue
            if padded and start <= padded[-1]["end"]:
                padded[-1]["end"] = max(padded[-1]["end"], end)
            else:
                padded.append({"start": start, "end": end})
        return padded

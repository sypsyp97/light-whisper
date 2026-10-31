"""Independent, test-only NumPy equations for the pinned Real-time RE-USE model.

Research/noncommercial validation only. No NVIDIA implementation is imported,
executed, or reproduced. Layout is [time, frequency, channels]; convolutions run
over a complete sequence, independently of native streaming caches.

Architecture/weights: https://huggingface.co/nvidia/Real-time_RE-USE/tree/
4c0c186272ca853cd18d832fc5c28777b62f32d1, specifically config.json,
models/streaming_codec_module_time_d1_input_ahead_sep_conv.py,
models/streaming_mamba_block2_SEMamba.py, and online_inference.py. The latter
specifies K=lookahead+1 for priming and lookahead zero frames at EOF.
Selective state-space equations: https://arxiv.org/abs/2312.00752, section 3.
"""

import hashlib
import json
from pathlib import Path
import struct

import numpy as np


MODEL_SHA256 = "6eac4e8cc721f99a22cde7eac3c9f8e2826ced5dc8a4234fd1977de1ec0d3ade"
BINS, FFT_SIZE, HOP_SIZE = 321, 640, 320


def load_weights(path, dtype=np.float32):
    """Read the original F32 safetensors payload without another dependency."""
    blob = Path(path).read_bytes()
    digest = hashlib.sha256(blob).hexdigest()
    if digest != MODEL_SHA256:
        raise ValueError(f"Real-time checkpoint SHA-256 mismatch: {digest}")
    header_bytes = struct.unpack_from("<Q", blob)[0]
    header = json.loads(blob[8:8 + header_bytes])
    weights = {}
    for name, entry in header.items():
        if name == "__metadata__":
            continue
        if entry["dtype"] != "F32":
            raise ValueError(f"Expected original F32 tensor: {name}")
        start, end = entry["data_offsets"]
        values = np.frombuffer(blob, dtype="<f4", count=(end - start) // 4,
                               offset=8 + header_bytes + start)
        weights[name] = values.reshape(entry["shape"]).astype(dtype, copy=False)
    return weights


def channel_norm(x, scale, bias):
    mean = x.mean(axis=-1, keepdims=True)
    variance = ((x - mean) ** 2).mean(axis=-1, keepdims=True)
    return (x - mean) / np.sqrt(variance + 1e-5) * scale + bias


def frequency_shuffle(x):
    """y[t, 2*f+r, c] = x[t, f, r*C+c], for r in {0, 1}."""
    t, f, channels = x.shape
    return x.reshape(t, 2 * f, channels // 2)


def convolution(x, weight, bias, *, time_padding=(0, 0), freq_padding=0,
                time_dilation=1, freq_stride=1):
    """Cross-correlation; materialize patches then one ordinary matrix product."""
    kt, kf = weight.shape[2:]
    span = (kt - 1) * time_dilation + 1
    padded = np.pad(x, (time_padding, (freq_padding, freq_padding), (0, 0)))
    patches = np.lib.stride_tricks.sliding_window_view(padded, (span, kf), axis=(0, 1))
    patches = patches[:, ::freq_stride, :, ::time_dilation, :]
    shape = patches.shape[:2] + (weight.shape[0],)
    return (patches.reshape(-1, weight.shape[1] * kt * kf)
            @ weight.reshape(weight.shape[0], -1).T + bias).reshape(shape)


class Reference:
    """Batch reference; no native code and no streaming-state representation."""

    def __init__(self, weights):
        self.w = weights

    def linear(self, x, name):
        y = x @ self.w[name + ".weight"].T
        return y + self.w[name + ".bias"] if name + ".bias" in self.w else y

    def activate(self, x, norm, prelu):
        x = channel_norm(x, self.w[norm + ".weight"], self.w[norm + ".bias"])
        return np.maximum(x, 0) + np.minimum(x, 0) * self.w[prelu + ".weight"]

    def conv(self, x, name, **kwargs):
        return convolution(x, self.w[name + ".weight"], self.w[name + ".bias"], **kwargs)

    def dense(self, x, name):
        features = x
        for depth in range(4):
            p = f"{name}.dense_block.{depth}"
            y = self.conv(features, p + ".1", time_padding=(2 * 2 ** depth, 0),
                          time_dilation=2 ** depth, freq_padding=1)
            y = self.activate(y, p + ".2.norm", p + ".3")
            features = np.concatenate((y, features), axis=-1)
        return y

    def selective(self, x, name):
        """x[B,L,C]; h <- exp(delta*A)h + delta*B*u; y <- C*h+D*u."""
        projected = self.linear(x, name + ".in_proj")
        u, gate = np.split(projected, 2, axis=-1)
        kernel = self.w[name + ".conv1d.weight"][:, 0]
        padded = np.pad(u, ((0, 0), (kernel.shape[1] - 1, 0), (0, 0)))
        windows = np.lib.stride_tricks.sliding_window_view(padded, kernel.shape[1], axis=1)
        u = (windows * kernel).sum(axis=-1) + self.w[name + ".conv1d.bias"]
        u = u / (1 + np.exp(-u))
        parameters = self.linear(u, name + ".x_proj")
        delta = self.linear(parameters[..., :4], name + ".dt_proj")
        delta = np.logaddexp(0, delta)
        b, c = parameters[..., 4:20], parameters[..., 20:36]
        a = -np.exp(self.w[name + ".A_log"])
        state = np.zeros((x.shape[0], u.shape[-1], a.shape[-1]), dtype=x.dtype)
        output = np.empty_like(u)
        for index in range(x.shape[1]):
            dt = delta[:, index, :, None]
            state = (np.exp(dt * a) * state
                     + dt * b[:, index, None, :] * u[:, index, :, None])
            output[:, index] = ((state * c[:, index, None, :]).sum(axis=-1)
                                + self.w[name + ".D"] * u[:, index])
        output *= gate / (1 + np.exp(-gate))
        return self.linear(output, name + ".out_proj")

    def axis_block(self, x, name, bidirectional):
        forward = self.selective(x, name + ".forward_blocks") + x
        if bidirectional:
            reverse = self.selective(x[:, ::-1], name + ".backward_blocks") + x[:, ::-1]
            forward = np.concatenate((forward, reverse[:, ::-1]), axis=-1)
        projected = self.linear(forward, name + ".output_proj")
        return x + channel_norm(projected, self.w[name + ".norm.weight"],
                                self.w[name + ".norm.bias"])

    def decode(self, x, name, phase=False):
        x = self.dense(x, name + ".dense_block")
        x = self.conv(x, name + ".up_conv1.0.conv", freq_padding=1)
        x = frequency_shuffle(x)
        x = self.activate(x, name + ".up_conv1.1.norm", name + ".up_conv1.2")
        # The second decoder kernel acts on time, with temporal ratio exactly one.
        kernel = self.w[name + ".up_conv2.0.conv.weight"].transpose(0, 1, 3, 2)
        x = convolution(x, kernel, self.w[name + ".up_conv2.0.conv.bias"], time_padding=(2, 0))
        x = self.activate(x, name + ".up_conv2.1.norm", name + ".up_conv2.2")
        heads = ("phase_conv_r", "phase_conv_i") if phase else ("final_conv",)
        return tuple(self.conv(x, name + "." + head)[:, :BINS, 0] for head in heads)

    def enhance(self, logmag, phase, lookahead, exits=(3, 12)):
        """Return frame-major (logmag, phase-head real, phase-head imag) by exit."""
        x = np.stack((logmag, phase), axis=-1).astype(next(iter(self.w.values())).dtype)
        x = np.pad(x, ((0, 0), (0, 2), (0, 0)))
        p = f"dense_encoder.dense_conv_1_{lookahead + 1}"
        x = self.conv(x, p + ".0", time_padding=(2 - lookahead, lookahead), freq_padding=1)
        x = self.activate(x, p + ".1.norm", p + ".2")
        x = self.dense(x, "dense_encoder.dense_block")
        p = "dense_encoder.dense_conv_2"
        x = self.conv(x, p + ".0", freq_stride=2)
        x = self.activate(x, p + ".1.norm", p + ".2")
        result = {}
        for layer in range(max(exits)):
            p = f"TSMamba.{layer}"
            x = self.axis_block(x.transpose(1, 0, 2), p + ".time_mamba", False).transpose(1, 0, 2)
            x = self.axis_block(x, p + ".freq_mamba", True)
            if layer + 1 in exits:
                magnitude, = self.decode(x, f"mask_decoder_list.{layer}")
                real, imag = self.decode(x, f"phase_decoder_list.{layer}", phase=True)
                result[layer + 1] = np.stack((magnitude, real, imag))
        return result


def spectral_inputs(audio_fixture):
    """19 analytic history frames followed by real audio STFT frames (16 kHz)."""
    t, f = np.arange(19)[:, None], np.arange(BINS)[None, :]
    magnitude = np.log1p(0.2 + (1 + np.sin(0.37 * t + 0.071 * f)) ** 2)
    phase = 2.7 * np.sin(0.41 * t - 0.017 * f)
    magnitude[8, ::17] += 0.8  # Nonzero taps beyond the longest dense dilation.
    with np.load(audio_fixture, allow_pickle=False) as fixture:
        audio = fixture["audio"].astype(np.float64)
    if audio.ndim != 1 or len(audio) < FFT_SIZE:
        raise ValueError("Expected real mono audio with at least 640 samples")
    audio = np.pad(audio, (FFT_SIZE // 2, FFT_SIZE // 2), mode="reflect")
    windows = np.lib.stride_tricks.sliding_window_view(audio, FFT_SIZE)[::HOP_SIZE]
    spectrum = np.fft.rfft(windows[:4] * np.hanning(FFT_SIZE + 1)[:-1])
    magnitude = np.concatenate((magnitude, np.log1p(np.abs(spectrum))))
    phase = np.concatenate((phase, np.angle(spectrum)))
    return np.ascontiguousarray(magnitude, dtype=np.float32), np.ascontiguousarray(phase, dtype=np.float32)

"""Adaptive Qwen3-ASR preview cadence.

Frozen contract (lead, 2026-11-20):

* Until the stream has captured ``64000`` samples (4 s at 16 kHz) every preview
  refresh keeps the historical ``preview_samples`` (``3200`` = 200 ms) cadence.
* From ``64000`` captured samples on, the interval relaxes to ``5120`` samples
  (320 ms).  The first preview of a stream still uses
  ``min(preview_samples, sample_rate * 120 // 1000) == 1920`` samples.
* The adaptive gate lives in ``QwenPreviewRuntime._accept_pcm``; the async
  production pipeline (``QwenPipelineRuntime``) is exercised through its own
  public ``feed``/``preview_text``/``finish`` surface and must show the same
  trigger positions, and the window / finish / reset semantics stay exactly as
  they were.  Trigger positions are asserted behaviourally, never by comparing
  method objects.

Cadence semantics pinned by these tests
---------------------------------------
The gate threshold is recomputed at the end of every ``_accept_pcm`` call from
the ``_sample_count`` reached there (the count *after* the chunk), and that
value judges the same accept.  ``formal/QwenPreviewCadence.tla`` models the same
rule: ``AcceptPcm`` fixes ``gate.value`` from the post-accept ``captured`` count
and the pending counter is compared against that value.  Consequences, asserted
below with both 1600-sample feeds and the 640-sample production grain:

* the accept landing exactly on ``64000`` samples already reads ``5120`` while
  only ``3200`` samples are pending, so it does **not** fire; the last preview
  below 4 s is the one at ``60800`` (1600 grain) / ``62720`` (640 grain);
* from ``64000`` on every trigger needs ``5120`` pending samples.  At the
  1600-sample grain that means one trigger every ``6400`` samples; at the
  production 640-sample grain ``5120`` is an exact multiple, so it is one
  trigger every 8 blocks (5120 samples).

Everything is driven through ``transcribe`` stubs: no model, no GPU, no native
library.
"""

import os
import sys
import threading
import time
import unittest
from types import SimpleNamespace

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from qwen3_native import QwenPipelineRuntime, QwenPreviewRuntime

SAMPLE_RATE = 16000
CHUNK = 1600                    # 100 ms of audio per feed
PRODUCTION_CHUNK = 640          # native_recording.rs feeds 40 ms blocks
STARTUP_SAMPLES = 1920          # min(preview_samples, sample_rate * 120 // 1000)
FAST_INTERVAL = 3200            # 200 ms, historical cadence
RELAXED_INTERVAL = 5120         # 320 ms, after 4 s of capture
RELAX_AFTER_SAMPLES = 64000     # 4 s of capture
WINDOW_SAMPLES = 192000         # 12 s preview window

# Trigger positions (cumulative accepted samples) for 1600-sample feeds.
# 3200 = FAST_INTERVAL: 3200 .. 60800 (19 triggers, the last one below 4 s).
FAST_TRIGGERS = [
    3200, 6400, 9600, 12800, 16000, 19200, 22400, 25600, 28800, 32000,
    35200, 38400, 41600, 44800, 48000, 51200, 54400, 57600, 60800,
]
# The accept that lands on exactly 64000 already reads the 5120 interval, so
# the last preview below 4 s is 60800 and the first relaxed one is 67200
# (3200 pending at 64000 and 4800 at 65600 stay below 5120).
EXPECTED_TRIGGERS_TO_64000 = FAST_TRIGGERS
# After the crossing the 5120 threshold plus 1600-sample granularity means one
# trigger every 6400 samples (four chunks).
EXPECTED_TRIGGERS_TO_160000 = FAST_TRIGGERS + [
    67200, 73600, 80000, 86400, 92800, 99200, 105600, 112000, 118400,
    124800, 131200, 137600, 144000, 150400, 156800,
]
EXPECTED_TRIGGERS_TO_195200 = EXPECTED_TRIGGERS_TO_160000 + [
    163200, 169600, 176000, 182400, 188800, 195200,
]
# Production feeds 640-sample blocks (native_recording.rs), where 5120 is an
# exact multiple: 5 blocks below 4 s, 8 blocks after it.  10 s of speech:
EXPECTED_640_TRIGGERS_TO_160000 = [
    1920, 5120, 8320, 11520, 14720, 17920, 21120, 24320, 27520, 30720,
    33920, 37120, 40320, 43520, 46720, 49920, 53120, 56320, 59520, 62720,
    67840, 72960, 78080, 83200, 88320, 93440, 98560, 103680, 108800,
    113920, 119040, 124160, 129280, 134400, 139520, 144640, 149760,
    154880, 160000,
]


def recorded(total, *, value=0.25):
    """Constant PCM of ``total`` samples."""
    return np.full(total, value, dtype=np.float32)


def ramp(total):
    """Distinct, finite PCM so that slices can be compared exactly."""
    return np.linspace(-0.5, 0.5, total, dtype=np.float32)


def feed_in_chunks(runtime, call_count, samples, *, chunk_samples=CHUNK,
                   preview=True, accepted=0):
    """Feed ``samples`` in chunks and return the trigger sample counts.

    A trigger is detected by the number of completed decodes growing across a
    single ``feed`` call.  Stubs decode synchronously, so this is exact.
    ``accepted`` is the sample count already captured before this call.
    """
    triggers = []
    fed = 0
    while fed < len(samples):
        step = samples[fed:fed + chunk_samples]
        before = call_count()
        runtime.feed(step, preview=preview)
        fed += len(step)
        if call_count() != before:
            triggers.append(accepted + fed)
    return triggers


class RecordingTranscribe:
    """Synchronous decode stub that records every PCM it receives."""

    def __init__(self, prefix='preview'):
        self.prefix = prefix
        self.calls = []

    def __call__(self, pcm, context='', language=None):
        text = f'{self.prefix}-{len(self.calls) + 1}'
        self.calls.append((pcm.copy(), context, language))
        return SimpleNamespace(text=text, language=None)


class PipelineHarness:
    """CPU/native stage recorder for the async pipeline; still no model."""

    def __init__(self):
        self.prepared = []
        self.calls = []
        self._condition = threading.Condition()

    def prepare_audio(self, pcm):
        with self._condition:
            self.prepared.append(pcm.copy())
            self._condition.notify_all()
        return pcm

    def transcribe(self, pcm, context='', language=None):
        with self._condition:
            text = f'pipeline-{len(self.calls) + 1}'
            self.calls.append((pcm.copy(), context, language))
            self._condition.notify_all()
        return SimpleNamespace(text=text, language=None)

    def wait_for_calls(self, count, timeout=15.0):
        deadline = time.monotonic() + timeout
        with self._condition:
            while len(self.calls) < count:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    return False
                self._condition.wait(remaining)
        return True

    @property
    def prepared_lengths(self):
        with self._condition:
            return [len(audio) for audio in self.prepared]


class QwenPreviewCadenceTests(unittest.TestCase):
    def make_preview_runtime(self, prefix='preview'):
        transcribe = RecordingTranscribe(prefix)
        # Contract: the *default* construction is 16000 Hz / 3200 / 192000.
        runtime = QwenPreviewRuntime(transcribe)
        runtime.start()
        return runtime, transcribe

    def assert_padded_decode(self, call, captured):
        audio, context, language = call
        expected = np.pad(captured, (0, max(0, 8000 - len(captured))))
        np.testing.assert_array_equal(audio, expected)
        self.assertEqual(audio.dtype, np.float32)
        self.assertEqual((context, language), ('', None))

    # ---------------------------------------------------------------- startup

    def test_startup_preview_still_fires_at_1920_samples(self):
        runtime, transcribe = self.make_preview_runtime()
        self.assertEqual((runtime._preview_samples, runtime._window_samples),
                         (FAST_INTERVAL, WINDOW_SAMPLES),
                         'the defaults this suite pins are 3200 / 192000')
        self.assertEqual(STARTUP_SAMPLES,
                         min(FAST_INTERVAL, SAMPLE_RATE * 120 // 1000))

        runtime.feed(recorded(STARTUP_SAMPLES - 20))
        self.assertEqual(len(transcribe.calls), 0,
                         'startup threshold must stay above 1900 samples')
        runtime.feed(recorded(20))
        self.assertEqual(len(transcribe.calls), 1,
                         'first preview must fire at min(3200, 1920) == 1920')

        self.assert_padded_decode(transcribe.calls[0], recorded(STARTUP_SAMPLES))

    # --------------------------------------------------------------- cadence

    def test_first_four_seconds_keep_the_3200_sample_cadence(self):
        runtime, transcribe = self.make_preview_runtime()
        samples = recorded(60800)

        triggers = feed_in_chunks(runtime, lambda: len(transcribe.calls), samples)

        self.assertEqual(triggers, FAST_TRIGGERS)
        self.assertEqual(len(transcribe.calls), len(FAST_TRIGGERS))
        self.assertEqual({later - earlier for earlier, later in
                          zip(triggers, triggers[1:])}, {FAST_INTERVAL})

    def test_cadence_relaxes_to_5120_samples_after_four_seconds(self):
        runtime, transcribe = self.make_preview_runtime()
        samples = recorded(160000)

        triggers = feed_in_chunks(runtime, lambda: len(transcribe.calls), samples)

        self.assertEqual(triggers, EXPECTED_TRIGGERS_TO_160000)
        self.assertEqual(len(transcribe.calls), len(EXPECTED_TRIGGERS_TO_160000))
        # 6400 is an artifact of the 1600-sample chunk size: the first pending
        # budget that can fire after the crossing is the 1600 multiple at or
        # above 5120.  The production 640-sample grain fires every 5120
        # samples (see test_production_640_sample_chunks_...).
        relaxed_gap = -(-RELAXED_INTERVAL // CHUNK) * CHUNK
        self.assertEqual(relaxed_gap, 6400)
        fast_count = len(FAST_TRIGGERS)
        self.assertEqual(triggers[:fast_count], FAST_TRIGGERS)
        self.assertNotIn(RELAX_AFTER_SAMPLES, triggers)
        self.assertEqual(triggers[fast_count], FAST_TRIGGERS[-1] + relaxed_gap)
        self.assertEqual({later - earlier for earlier, later in
                          zip(triggers[fast_count:], triggers[fast_count + 1:])},
                         {relaxed_gap})

    def test_production_640_sample_chunks_fire_every_5_then_every_8_blocks(self):
        """Same contract at the production capture grain (640 samples/block)."""
        runtime, transcribe = self.make_preview_runtime()
        samples = recorded(160000)  # 10 s of speech

        triggers = feed_in_chunks(runtime, lambda: len(transcribe.calls),
                                  samples, chunk_samples=PRODUCTION_CHUNK)

        self.assertEqual(triggers, EXPECTED_640_TRIGGERS_TO_160000)
        self.assertEqual(len(triggers), 39)
        blocks = [count // PRODUCTION_CHUNK for count in triggers]
        self.assertEqual(blocks[:20], list(range(3, 99, 5)),
                         'before 4 s: first preview on block 3 (1920 samples), '
                         'then one every 5 blocks (3200 samples)')
        self.assertEqual(triggers[19], 62720,
                         'last fast trigger: 1920 + 19 * 3200')
        self.assertEqual(blocks[20:], list(range(106, 251, 8)),
                         'after 4 s: one preview every 8 blocks (5120 samples)')
        self.assertEqual(triggers[20], 67840,
                         'first relaxed trigger: 62720 + 5120, because the '
                         'accept landing on 64000 already reads 5120')
        self.assertNotIn(RELAX_AFTER_SAMPLES, triggers)

    def test_boundary_at_64000_switches_the_interval_inside_that_accept(self):
        """The accept landing on 64000 already reads the relaxed interval.

        ``_accept_pcm`` recomputes the threshold at its end from the sample
        count reached there, so at exactly 64000 samples the interval is 5120
        while only 3200 samples are pending: the accept must not fire.  The
        next trigger needs 5120 pending samples (6400 with 1600-sample
        granularity).  ``formal/QwenPreviewCadence.tla`` encodes the same rule:
        ``AcceptPcm`` fixes ``gate.value`` from the post-accept count.

        A latching implementation (decide 3200 at 60800, use it at 64000)
        would fire at 64000 and fail this test.
        """
        runtime, transcribe = self.make_preview_runtime()

        triggers = feed_in_chunks(runtime, lambda: len(transcribe.calls),
                                  recorded(64000))

        self.assertEqual(triggers, EXPECTED_TRIGGERS_TO_64000)
        self.assertEqual(triggers[-1], 60800,
                         'the last preview below 4 s is the one at 60800')
        self.assertNotIn(RELAX_AFTER_SAMPLES, triggers,
                         'the accept landing on exactly 64000 must already '
                         'compare against the relaxed 5120 interval')

        # 1600, 3200 and 4800 pending samples stay silent after the crossing;
        # 6400 fires and the relaxed cadence then continues every 6400 samples.
        rest = feed_in_chunks(runtime, lambda: len(transcribe.calls),
                              recorded(25600), accepted=RELAX_AFTER_SAMPLES)
        self.assertEqual(rest, [67200, 73600, 80000, 86400],
                         '5120 pending samples (rounded to 6400 by the chunk '
                         'size) is what fires after the 4 s cutoff')

    # ----------------------------------------------------------------- reset

    def test_reset_restores_startup_then_the_adaptive_cadence(self):
        runtime, transcribe = self.make_preview_runtime()

        first = feed_in_chunks(runtime, lambda: len(transcribe.calls),
                               recorded(160000))
        self.assertEqual(first, EXPECTED_TRIGGERS_TO_160000)
        self.assertEqual(len(transcribe.calls), len(EXPECTED_TRIGGERS_TO_160000))
        # 160000 is past the last preview at 156800, so finish() decodes the
        # complete capture once instead of reusing a stale preview.
        runtime.finish()
        calls_after_first_session = len(transcribe.calls)
        self.assertEqual(calls_after_first_session,
                         len(EXPECTED_TRIGGERS_TO_160000) + 1)

        # A fresh session starts at the 1920 startup threshold again.
        runtime.start()
        runtime.feed(recorded(1920))
        self.assertEqual(len(transcribe.calls), calls_after_first_session + 1,
                         'reset() must restore the 1920 startup threshold')
        runtime.reset()

        # ... and then walks the very same fast -> relaxed cadence.
        runtime.start()
        second = feed_in_chunks(runtime, lambda: len(transcribe.calls),
                                recorded(160000))
        self.assertEqual(second, EXPECTED_TRIGGERS_TO_160000)
        self.assertEqual(len(transcribe.calls) - calls_after_first_session,
                         1 + len(EXPECTED_TRIGGERS_TO_160000))

    # ---------------------------------------------------------------- finish

    def test_finish_reuses_a_covering_preview_and_still_decodes_stale_tails(self):
        runtime, transcribe = self.make_preview_runtime()
        runtime.feed(recorded(3200))
        self.assertEqual(len(transcribe.calls), 1)
        self.assertEqual(runtime.finish(), ('preview-1', None))
        self.assertEqual(len(transcribe.calls), 1,
                         'a preview covering every sample must be reused')

        runtime, transcribe = self.make_preview_runtime()
        runtime.feed(recorded(3200))
        runtime.feed(recorded(13))
        self.assertEqual(len(transcribe.calls), 1)
        self.assertEqual(runtime.finish(), ('preview-2', None))
        self.assertEqual(len(transcribe.calls), 2,
                         'a 13 sample stale tail must trigger one full decode')
        self.assertEqual(len(transcribe.calls[1][0]), 8000)

    # ---------------------------------------------------------------- window

    def test_preview_window_is_still_the_last_192000_samples(self):
        runtime, transcribe = self.make_preview_runtime()
        samples = ramp(195200)

        triggers = feed_in_chunks(runtime, lambda: len(transcribe.calls), samples)

        self.assertEqual(triggers[-1], 195200,
                         'the last preview must cover the newest samples')
        last_audio, _context, _language = transcribe.calls[-1]
        self.assertEqual(len(last_audio), WINDOW_SAMPLES)
        np.testing.assert_array_equal(last_audio, samples[-WINDOW_SAMPLES:])
        self.assertLess(len(last_audio), len(samples),
                        'long captures must still be trimmed to 12 s')
        # ... and the first preview of the stream still sees the whole capture.
        np.testing.assert_array_equal(
            transcribe.calls[0][0], np.pad(samples[:3200], (0, 4800)))

    def test_final_decode_keeps_the_full_capture_and_sample_count(self):
        runtime, transcribe = self.make_preview_runtime()
        samples = ramp(195213)

        triggers = feed_in_chunks(runtime, lambda: len(transcribe.calls),
                                  samples[:195200])
        self.assertEqual(triggers, EXPECTED_TRIGGERS_TO_195200)
        runtime.feed(samples[195200:])
        self.assertEqual(len(transcribe.calls), len(EXPECTED_TRIGGERS_TO_195200),
                         'a 13 sample tail must not trigger a preview decode')

        expected_calls = len(EXPECTED_TRIGGERS_TO_195200) + 1
        self.assertEqual(runtime.finish(), ('preview-%d' % expected_calls, None))
        self.assertEqual(len(transcribe.calls), expected_calls)
        final_audio, _context, _language = transcribe.calls[-1]
        self.assertEqual(len(final_audio), 195213)
        np.testing.assert_array_equal(final_audio, samples)


class QwenPipelineCadenceTests(unittest.TestCase):
    def make_pipeline(self):
        harness = PipelineHarness()
        runtime = QwenPipelineRuntime(harness.transcribe,
                                      prepare_audio=harness.prepare_audio)
        runtime.start()
        self.addCleanup(runtime.close)
        return runtime, harness

    def prime_suppressed(self, runtime, harness, total=RELAX_AFTER_SAMPLES):
        """Accept audio without previews so a test can start past the cutoff."""
        samples = recorded(total)
        fed = 0
        while fed < len(samples):
            runtime.feed(samples[fed:fed + CHUNK], preview=False)
            fed += CHUNK
        self.assertEqual(harness.calls, [])

    def wait_for_preview_text(self, runtime, expected, timeout=15.0):
        """Poll the public preview surface until the async stage publishes."""
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if runtime.preview_text == expected:
                return True
            time.sleep(0.002)
        return False

    def test_pipeline_cadence_after_four_seconds_matches_the_preview_runtime(self):
        runtime, harness = self.make_pipeline()
        self.prime_suppressed(runtime, harness)

        # First live preview: the startup 1920 budget is already exceeded.
        runtime.feed(recorded(5120))
        self.assertTrue(self.wait_for_preview_text(runtime, 'pipeline-1'),
                        'the primed stream must publish its first preview')
        # 3200 pending samples must stay silent past the cutoff.  The pending
        # counter is read synchronously after feed(): a trigger re-arms it to 0
        # inside _submit, in the caller thread, so no thread hand-off and no
        # wall-clock window is involved.
        runtime.feed(recorded(3200))
        self.assertEqual(runtime._new_samples, 3200,
                         'the pending budget must not be re-armed at 3200 '
                         'samples past the 4 s cutoff')
        self.assertEqual(harness.prepared_lengths, [69120],
                         'no CPU preparation may be queued for 3200 pending '
                         'samples past the 4 s cutoff')
        # Filling the relaxed budget to 6400 decodes exactly once.
        runtime.feed(recorded(3200))
        self.assertTrue(harness.wait_for_calls(2),
                        'pending 6400 must fire with the relaxed interval; a '
                        'trigger at 72320 would have re-armed the budget')
        self.assertEqual(harness.prepared_lengths, [69120, 75520])
        self.assertEqual([len(audio) for audio, _c, _l in harness.calls],
                         [69120, 75520])

    def test_pipeline_finish_reuses_the_covering_preview(self):
        runtime, harness = self.make_pipeline()
        self.prime_suppressed(runtime, harness)

        runtime.feed(recorded(5120))
        self.assertTrue(harness.wait_for_calls(1))
        self.assertTrue(self.wait_for_preview_text(runtime, 'pipeline-1'),
                        'the covering preview must be published before finish')

        self.assertEqual(runtime.finish(), ('pipeline-1', None))
        self.assertEqual(len(harness.calls), 1,
                         'finish() must reuse a preview covering every sample')
        self.assertEqual(harness.prepared_lengths, [69120])

    def test_pipeline_finish_decodes_the_full_capture_for_a_stale_tail(self):
        runtime, harness = self.make_pipeline()
        self.prime_suppressed(runtime, harness)
        runtime.feed(recorded(5120))
        self.assertTrue(harness.wait_for_calls(1))
        self.assertTrue(self.wait_for_preview_text(runtime, 'pipeline-1'))

        runtime.feed(recorded(13))
        self.assertEqual(runtime.finish(), ('pipeline-2', None))
        self.assertEqual(len(harness.calls), 2,
                         'a stale 13 sample tail must be decoded in full')
        self.assertEqual(harness.prepared_lengths, [69120, 69133])
        final_audio, _context, _language = harness.calls[-1]
        self.assertEqual(len(final_audio), 69133)
        np.testing.assert_array_equal(final_audio,
                                      np.full(69133, 0.25, dtype=np.float32))


if __name__ == '__main__':
    unittest.main()

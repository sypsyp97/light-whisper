"""Regression for the real English clip's sentence boundary after VAD splitting."""
import unittest
from unittest.mock import Mock
import numpy as np
from r2t2_segmented import SegmentedR2T2

class SegmentSpacingTests(unittest.TestCase):
    def test_english_sentence_boundary_keeps_word_separation(self):
        native=Mock()
        native.feed.side_effect=[('years.',None),('22,500 times',None)]
        native.finish.side_effect=[('years.',None),('22,500 times',None)]
        vad=Mock();vad.speech_timestamps.return_value=[{'start':0,'end':4}]
        stream=SegmentedR2T2(native,vad,chunk_samples=4,window_samples=4,min_segment_samples=4,max_segment_samples=4)
        stream.start()
        first,_=stream.feed(np.ones(4,dtype=np.float32))
        second,_=stream.feed(np.ones(4,dtype=np.float32))
        final,_=stream.finish()
        self.assertEqual(first,'years.')
        self.assertEqual(second,'years. 22,500 times')
        self.assertEqual(final,second)
        self.assertEqual(native.finish.call_count,2)

if __name__=='__main__': unittest.main()

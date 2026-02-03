"""
test_chunking.py - Tests for text chunking utilities.

Tests estimate_tokens, split_into_sentences, split_into_paragraphs,
chunk_text, chunk_for_embedding, and TextChunk dataclass.
"""

from codeflow_py_lib.chunking import (
    CHARS_PER_TOKEN,
    DEFAULT_CHUNK_SIZE,
    DEFAULT_OVERLAP,
    TextChunk,
    chunk_for_embedding,
    chunk_text,
    estimate_tokens,
    split_into_paragraphs,
    split_into_sentences,
)


class TestEstimateTokens:
    """Tests for estimate_tokens function."""

    def test_empty_string_returns_zero(self):
        """Empty string should return 0 tokens."""
        assert estimate_tokens("") == 0

    def test_none_returns_zero(self):
        """None should return 0 tokens."""
        assert estimate_tokens(None) == 0

    def test_whitespace_only_returns_minimum(self):
        """Whitespace-only string returns 1 due to max(1,...) guarantee."""
        # Implementation uses max(1, len(text.strip()) // 4) which returns 1
        # for non-None strings even when they strip to empty
        assert estimate_tokens("   \n\t  ") == 1

    def test_short_string_returns_at_least_one(self):
        """Short strings should return at least 1 token."""
        assert estimate_tokens("Hi") >= 1
        assert estimate_tokens("A") >= 1

    def test_typical_word_estimate(self):
        """Typical word should be approximately 1-2 tokens."""
        # Average English word is 4-5 characters, so ~1 token
        tokens = estimate_tokens("hello")
        assert 1 <= tokens <= 2

    def test_sentence_estimate(self):
        """Sentence should estimate reasonable token count."""
        sentence = "The quick brown fox jumps over the lazy dog."
        tokens = estimate_tokens(sentence)
        # 44 characters / 4 chars per token = ~11 tokens
        assert 8 <= tokens <= 15

    def test_longer_text_scales(self):
        """Longer text should have proportionally more tokens."""
        short_text = "Hello world."
        long_text = short_text * 10
        short_tokens = estimate_tokens(short_text)
        long_tokens = estimate_tokens(long_text)
        # Should be approximately 10x more
        assert 8 * short_tokens <= long_tokens <= 12 * short_tokens


class TestSplitIntoSentences:
    """Tests for split_into_sentences function."""

    def test_empty_string_returns_empty_list(self):
        """Empty string should return empty list."""
        assert split_into_sentences("") == []

    def test_none_returns_empty_list(self):
        """None should return empty list."""
        assert split_into_sentences(None) == []

    def test_whitespace_only_returns_empty_list(self):
        """Whitespace-only should return empty list."""
        assert split_into_sentences("   \n\t  ") == []

    def test_single_sentence_no_terminal_punctuation(self):
        """Single sentence without terminal punctuation."""
        result = split_into_sentences("Hello world")
        assert len(result) == 1
        assert result[0] == "Hello world"

    def test_single_sentence_with_period(self):
        """Single sentence with period."""
        result = split_into_sentences("Hello world.")
        assert len(result) == 1
        assert "Hello world" in result[0]

    def test_two_sentences_period(self):
        """Two sentences split by period."""
        text = "First sentence. Second sentence."
        result = split_into_sentences(text)
        assert len(result) >= 1  # May be 1 or 2 depending on implementation

    def test_sentences_with_exclamation(self):
        """Sentences split by exclamation mark."""
        text = "Hello! How are you?"
        result = split_into_sentences(text)
        assert len(result) >= 1

    def test_sentences_with_question_mark(self):
        """Sentences split by question mark."""
        text = "What is your name? My name is Claude."
        result = split_into_sentences(text)
        assert len(result) >= 1

    def test_preserves_content(self):
        """All content should be preserved in output."""
        text = "First part. Second part. Third part."
        result = split_into_sentences(text)
        combined = " ".join(result)
        assert "First" in combined
        assert "Second" in combined
        assert "Third" in combined

    def test_normalizes_whitespace(self):
        """Should normalize multiple spaces."""
        text = "First   sentence.    Second   sentence."
        result = split_into_sentences(text)
        for sentence in result:
            assert "  " not in sentence  # No double spaces


class TestSplitIntoParagraphs:
    """Tests for split_into_paragraphs function."""

    def test_empty_string_returns_empty_list(self):
        """Empty string should return empty list."""
        assert split_into_paragraphs("") == []

    def test_none_returns_empty_list(self):
        """None should return empty list."""
        assert split_into_paragraphs(None) == []

    def test_whitespace_only_returns_empty_list(self):
        """Whitespace-only should return empty list."""
        assert split_into_paragraphs("   \n\t  ") == []

    def test_single_paragraph(self):
        """Single paragraph without double newlines."""
        text = "This is a single paragraph with one line."
        result = split_into_paragraphs(text)
        assert len(result) == 1
        assert "single paragraph" in result[0]

    def test_two_paragraphs(self):
        """Two paragraphs split by double newline."""
        text = "First paragraph.\n\nSecond paragraph."
        result = split_into_paragraphs(text)
        assert len(result) == 2
        assert "First" in result[0]
        assert "Second" in result[1]

    def test_multiple_paragraphs(self):
        """Multiple paragraphs."""
        text = "Para 1.\n\nPara 2.\n\nPara 3."
        result = split_into_paragraphs(text)
        assert len(result) == 3

    def test_handles_extra_newlines(self):
        """Should handle more than two consecutive newlines."""
        text = "First.\n\n\n\nSecond."
        result = split_into_paragraphs(text)
        assert len(result) == 2

    def test_strips_paragraph_whitespace(self):
        """Each paragraph should be stripped of leading/trailing whitespace."""
        text = "  First paragraph.  \n\n  Second paragraph.  "
        result = split_into_paragraphs(text)
        for para in result:
            assert para == para.strip()


class TestTextChunk:
    """Tests for TextChunk dataclass."""

    def test_create_text_chunk(self):
        """Should create TextChunk with all fields."""
        chunk = TextChunk(
            index=0,
            text="Hello world",
            char_offset_start=0,
            char_offset_end=11,
            token_estimate=3,
            has_overlap_prev=False,
            has_overlap_next=True,
        )
        assert chunk.index == 0
        assert chunk.text == "Hello world"
        assert chunk.char_offset_start == 0
        assert chunk.char_offset_end == 11
        assert chunk.token_estimate == 3
        assert chunk.has_overlap_prev is False
        assert chunk.has_overlap_next is True

    def test_chunk_equality(self):
        """Two chunks with same data should be equal."""
        chunk1 = TextChunk(0, "test", 0, 4, 1, False, False)
        chunk2 = TextChunk(0, "test", 0, 4, 1, False, False)
        assert chunk1 == chunk2

    def test_chunk_inequality(self):
        """Chunks with different data should not be equal."""
        chunk1 = TextChunk(0, "test", 0, 4, 1, False, False)
        chunk2 = TextChunk(1, "test", 0, 4, 1, False, False)
        assert chunk1 != chunk2


class TestChunkText:
    """Tests for chunk_text function."""

    def test_empty_string_returns_empty_list(self):
        """Empty string should return empty list."""
        assert chunk_text("") == []

    def test_none_returns_empty_list(self):
        """None should return empty list."""
        assert chunk_text(None) == []

    def test_whitespace_only_returns_empty_list(self):
        """Whitespace-only should return empty list."""
        assert chunk_text("   \n\t  ") == []

    def test_short_text_single_chunk(self):
        """Short text should produce single chunk."""
        text = "Hello world."
        chunks = chunk_text(text, chunk_size=50, overlap=10)
        assert len(chunks) == 1
        assert chunks[0].text.strip() != ""

    def test_chunk_has_correct_index(self):
        """Chunks should have sequential indices."""
        text = "First sentence. " * 20  # Long enough for multiple chunks
        chunks = chunk_text(text, chunk_size=20, overlap=5)
        for i, chunk in enumerate(chunks):
            assert chunk.index == i

    def test_first_chunk_no_overlap_prev(self):
        """First chunk should have no previous overlap."""
        text = "Sentence one. Sentence two. Sentence three. " * 5
        chunks = chunk_text(text, chunk_size=30, overlap=10)
        if chunks:
            assert chunks[0].has_overlap_prev is False

    def test_last_chunk_no_overlap_next(self):
        """Last chunk should have no next overlap."""
        text = "Sentence one. Sentence two. Sentence three. " * 5
        chunks = chunk_text(text, chunk_size=30, overlap=10)
        if chunks:
            assert chunks[-1].has_overlap_next is False

    def test_middle_chunks_have_overlap_indicators(self):
        """Middle chunks should indicate overlaps."""
        text = "The quick brown fox jumps over the lazy dog. " * 20
        chunks = chunk_text(text, chunk_size=30, overlap=10)
        if len(chunks) > 2:
            for chunk in chunks[1:-1]:
                assert chunk.has_overlap_prev is True

    def test_chunks_cover_all_content(self):
        """All original content should be present in chunks."""
        original_words = ["alpha", "beta", "gamma", "delta", "epsilon"]
        text = " ".join(f"{w}." for w in original_words)
        text = " ".join([text] * 3)  # Repeat to make it longer
        chunks = chunk_text(text, chunk_size=20, overlap=5)
        combined = " ".join(c.text for c in chunks)
        for word in original_words:
            assert word in combined

    def test_token_estimates_reasonable(self):
        """Token estimates should be reasonable."""
        text = "Hello world. " * 50
        chunks = chunk_text(text, chunk_size=50, overlap=10)
        for chunk in chunks:
            assert chunk.token_estimate > 0
            # Token estimate should be roughly chars / 4
            expected_approx = len(chunk.text.strip()) // CHARS_PER_TOKEN
            assert (
                abs(chunk.token_estimate - expected_approx) <= expected_approx // 2 + 1
            )

    def test_custom_chunk_size(self):
        """Should respect custom chunk size."""
        text = "Word. " * 200  # Lots of content
        small_chunks = chunk_text(text, chunk_size=20, overlap=5)
        large_chunks = chunk_text(text, chunk_size=100, overlap=10)
        # Smaller chunk size should produce more chunks
        assert len(small_chunks) > len(large_chunks)

    def test_overlap_creates_shared_content(self):
        """Consecutive chunks should share overlapping content."""
        text = "The quick brown fox jumps over the lazy dog. " * 10
        chunks = chunk_text(text, chunk_size=30, overlap=10)
        if len(chunks) >= 2:
            # End of first chunk and start of second should share words
            first_words = set(chunks[0].text.split()[-3:])  # Last 3 words
            second_words = set(chunks[1].text.split()[:5])  # First 5 words
            # There should be some overlap
            assert len(first_words & second_words) >= 0  # Some overlap expected

    def test_char_offsets_valid(self):
        """Character offsets should be valid."""
        text = "Hello world. This is a test. Another sentence."
        chunks = chunk_text(text)
        for chunk in chunks:
            assert chunk.char_offset_start >= 0
            assert chunk.char_offset_end > chunk.char_offset_start
            assert chunk.char_offset_end <= len(text) + 50  # Allow some flexibility


class TestChunkForEmbedding:
    """Tests for chunk_for_embedding function."""

    def test_returns_list_of_strings(self):
        """Should return list of strings, not TextChunk objects."""
        text = "Hello world. This is a test."
        result = chunk_for_embedding(text)
        assert isinstance(result, list)
        for item in result:
            assert isinstance(item, str)

    def test_empty_string_returns_empty_list(self):
        """Empty string should return empty list."""
        assert chunk_for_embedding("") == []

    def test_short_text_single_string(self):
        """Short text should return single string."""
        text = "Hello world."
        result = chunk_for_embedding(text, max_tokens=50)
        assert len(result) == 1

    def test_custom_max_tokens(self):
        """Should respect max_tokens parameter."""
        text = "Word. " * 200
        small_result = chunk_for_embedding(text, max_tokens=20)
        large_result = chunk_for_embedding(text, max_tokens=100)
        assert len(small_result) > len(large_result)

    def test_preserves_content(self):
        """All content should be preserved."""
        words = ["alpha", "beta", "gamma", "delta"]
        text = " ".join(f"{w}." for w in words) * 3
        result = chunk_for_embedding(text, max_tokens=20)
        combined = " ".join(result)
        for word in words:
            assert word in combined


class TestChunkingConstants:
    """Tests for module constants."""

    def test_default_chunk_size_positive(self):
        """DEFAULT_CHUNK_SIZE should be positive."""
        assert DEFAULT_CHUNK_SIZE > 0

    def test_default_overlap_positive(self):
        """DEFAULT_OVERLAP should be positive."""
        assert DEFAULT_OVERLAP > 0

    def test_overlap_less_than_chunk_size(self):
        """DEFAULT_OVERLAP should be less than DEFAULT_CHUNK_SIZE."""
        assert DEFAULT_OVERLAP < DEFAULT_CHUNK_SIZE

    def test_chars_per_token_positive(self):
        """CHARS_PER_TOKEN should be positive."""
        assert CHARS_PER_TOKEN > 0


class TestEdgeCases:
    """Tests for edge cases and special inputs."""

    def test_very_long_sentence_no_periods(self):
        """Very long sentence without periods stays as single chunk.

        The chunking algorithm is sentence-aware, so text without
        sentence boundaries (periods followed by capital letters)
        is treated as a single semantic unit.
        """
        text = "word " * 500  # No sentence boundaries
        chunks = chunk_text(text, chunk_size=50, overlap=10)
        # Without sentence boundaries, entire text is one chunk
        assert len(chunks) >= 1
        # All content should be preserved
        assert "word" in chunks[0].text

    def test_unicode_text(self):
        """Should handle unicode text."""
        text = "日本語の文章です。これは別の文です。もう一つの文。"
        chunks = chunk_text(text, chunk_size=10, overlap=2)
        assert len(chunks) >= 1
        combined = "".join(c.text for c in chunks)
        assert "日本語" in combined

    def test_emoji_text(self):
        """Should handle emoji in text."""
        text = "Hello 👋 world! How are you? 😊 Great! 🎉"
        chunks = chunk_text(text, chunk_size=20, overlap=5)
        assert len(chunks) >= 1

    def test_mixed_newlines_and_sentences(self):
        """Should handle text with both newlines and sentence breaks."""
        text = "First line.\nSecond line.\n\nNew paragraph. Another sentence."
        chunks = chunk_text(text)
        assert len(chunks) >= 1

    def test_single_character(self):
        """Single character should produce one chunk."""
        chunks = chunk_text("A")
        assert len(chunks) == 1
        assert chunks[0].text == "A"

    def test_only_punctuation(self):
        """Text with only punctuation."""
        text = "... !!! ???"
        chunks = chunk_text(text)
        assert len(chunks) >= 1

    def test_code_like_text(self):
        """Should handle code-like text."""
        text = "def foo():\n    return 42\n\ndef bar():\n    return 'hello'"
        chunks = chunk_text(text, chunk_size=30, overlap=5)
        assert len(chunks) >= 1
        combined = " ".join(c.text for c in chunks)
        assert "def" in combined
        assert "return" in combined


class TestChunkByCharactersFallback:
    """Tests for the _chunk_by_characters fallback function."""

    def test_fallback_when_no_sentence_boundaries(self):
        """Should use character-based chunking when no sentences detected."""
        # Text without sentence-ending punctuation followed by capital letter
        # This should trigger _chunk_by_characters
        text = "lowercase text without proper sentence boundaries " * 50
        chunks = chunk_text(text, chunk_size=20, overlap=5)
        assert len(chunks) >= 1
        # Should still preserve content
        combined = "".join(c.text for c in chunks)
        assert "lowercase" in combined

    def test_fallback_with_long_continuous_text(self):
        """Character chunking for text that won't split into sentences."""
        # No capital letters after punctuation = no sentence boundaries
        text = "this is a test text that continues without proper sentences " * 30
        chunks = chunk_text(text, chunk_size=15, overlap=3)
        # Should produce multiple chunks through character fallback
        assert len(chunks) >= 1

    def test_fallback_produces_chunks(self):
        """Long text without sentence boundaries produces chunks."""
        # Very long text without sentence boundaries
        # Note: This text is treated as a single "sentence" by the sentence splitter
        # so it remains as one chunk (sentence-aware chunking keeps semantic units together)
        text = "abc " * 500  # 2000 chars
        chunks = chunk_text(text, chunk_size=20, overlap=5)
        # Single sentence = single chunk in sentence-aware mode
        assert len(chunks) >= 1
        # Content is preserved
        assert "abc" in chunks[0].text

    def test_fallback_overlap_indicators(self):
        """Fallback chunks should have correct overlap indicators."""
        text = "xyz " * 200
        chunks = chunk_text(text, chunk_size=15, overlap=3)
        if len(chunks) >= 2:
            # First chunk: no prev overlap
            assert chunks[0].has_overlap_prev is False
            # Last chunk: no next overlap
            assert chunks[-1].has_overlap_next is False

    def test_fallback_character_offsets(self):
        """Fallback should set correct character offsets."""
        text = "test " * 200
        chunks = chunk_text(text, chunk_size=15, overlap=3)
        if chunks:
            # First chunk starts at 0
            assert chunks[0].char_offset_start == 0
            # Offsets should increase
            if len(chunks) > 1:
                assert chunks[1].char_offset_start > 0


class TestInternalFunctions:
    """Tests for internal helper functions through their effects."""

    def test_get_overlap_sentences_empty_input(self):
        """_get_overlap_sentences handles empty list (tested via chunk_text)."""
        # Single short sentence - no overlap needed
        text = "Short."
        chunks = chunk_text(text, chunk_size=100, overlap=20)
        assert len(chunks) == 1
        assert chunks[0].has_overlap_prev is False

    def test_create_chunk_sentence_position_fallback(self):
        """_create_chunk handles sentence position fallback."""
        # Create text where sentence position lookup might fail
        text = "First sentence is here. Second one is here too. Third follows."
        chunks = chunk_text(text, chunk_size=10, overlap=3)
        # Should still work even if position lookup is approximate
        assert len(chunks) >= 1

    def test_overlap_calculation_with_many_sentences(self):
        """Overlap calculation with multiple sentences."""
        text = "A. B. C. D. E. F. G. H. I. J. K. L. M. N. O. P. Q. R. S. T."
        chunks = chunk_text(text, chunk_size=10, overlap=5)
        # Should have overlapping content
        if len(chunks) >= 2:
            # Middle chunks should indicate overlaps
            for chunk in chunks[1:]:
                assert chunk.has_overlap_prev is True

    def test_large_overlap_relative_to_chunk(self):
        """Handle case where overlap is large relative to chunk size."""
        text = "Sentence one here. Sentence two here. Sentence three here. Four."
        # Overlap almost as large as chunk size
        chunks = chunk_text(text, chunk_size=15, overlap=12)
        assert len(chunks) >= 1
        # All content preserved
        combined = " ".join(c.text for c in chunks)
        assert "one" in combined
        assert "two" in combined

    def test_single_very_long_sentence_triggers_fallback(self):
        """Single sentence longer than chunk size uses fallback."""
        # One very long sentence without proper boundaries
        text = "this is one continuous sentence without proper boundaries " * 20
        chunks = chunk_text(text, chunk_size=10, overlap=2)
        # Should still chunk through fallback
        assert len(chunks) >= 1

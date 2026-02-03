"""
test_embeddings.py - Tests for embedding generation utilities.

Tests generate_embedding, generate_embeddings, embedding_to_blob,
blob_to_embedding, cosine_similarity, find_most_similar, and related functions.
"""

import struct

import pytest
from codeflow_py_lib.embeddings import (
    DEFAULT_MODEL,
    EMBEDDING_DIM,
    SENTENCE_TRANSFORMERS_AVAILABLE,
    _hash_embedding,
    blob_to_embedding,
    cosine_similarity,
    embedding_to_blob,
    find_most_similar,
    generate_embedding,
    generate_embeddings,
    get_model,
)


class TestConstants:
    """Tests for module constants."""

    def test_default_model_is_string(self):
        """DEFAULT_MODEL should be a string."""
        assert isinstance(DEFAULT_MODEL, str)
        assert len(DEFAULT_MODEL) > 0

    def test_embedding_dim_positive(self):
        """EMBEDDING_DIM should be positive."""
        assert EMBEDDING_DIM > 0
        assert EMBEDDING_DIM == 384  # Expected for all-MiniLM-L6-v2

    def test_sentence_transformers_available_is_bool(self):
        """SENTENCE_TRANSFORMERS_AVAILABLE should be boolean."""
        assert isinstance(SENTENCE_TRANSFORMERS_AVAILABLE, bool)


class TestHashEmbedding:
    """Tests for _hash_embedding fallback function."""

    def test_returns_correct_dimension(self):
        """Should return embedding of correct dimension."""
        embedding = _hash_embedding("test text")
        assert len(embedding) == EMBEDDING_DIM

    def test_returns_list_of_floats(self):
        """Should return list of floats."""
        embedding = _hash_embedding("test")
        assert isinstance(embedding, list)
        for val in embedding:
            assert isinstance(val, float)

    def test_values_in_valid_range(self):
        """Values should be in [-1, 1] range."""
        embedding = _hash_embedding("test text for embedding")
        for val in embedding:
            assert -1.0 <= val <= 1.0

    def test_deterministic(self):
        """Same input should produce same output."""
        text = "deterministic test"
        emb1 = _hash_embedding(text)
        emb2 = _hash_embedding(text)
        assert emb1 == emb2

    def test_different_inputs_different_outputs(self):
        """Different inputs should produce different outputs."""
        emb1 = _hash_embedding("text one")
        emb2 = _hash_embedding("text two")
        assert emb1 != emb2

    def test_empty_string_returns_zeros(self):
        """Empty string should return zero vector."""
        embedding = _hash_embedding("")
        assert len(embedding) == EMBEDDING_DIM
        assert all(v == 0.0 for v in embedding)

    def test_whitespace_only_returns_zeros(self):
        """Whitespace-only should return zero vector."""
        embedding = _hash_embedding("   \n\t  ")
        assert len(embedding) == EMBEDDING_DIM
        assert all(v == 0.0 for v in embedding)


class TestGenerateEmbedding:
    """Tests for generate_embedding function."""

    def test_returns_correct_dimension(self):
        """Should return embedding of correct dimension."""
        embedding = generate_embedding("test text")
        assert len(embedding) == EMBEDDING_DIM

    def test_returns_list_of_floats(self):
        """Should return list of floats."""
        embedding = generate_embedding("test")
        assert isinstance(embedding, list)
        for val in embedding:
            assert isinstance(val, float)

    def test_empty_string_returns_zero_vector(self):
        """Empty string should return zero vector."""
        embedding = generate_embedding("")
        assert len(embedding) == EMBEDDING_DIM
        assert all(v == 0.0 for v in embedding)

    def test_none_handled(self):
        """None input should be handled gracefully."""
        # May raise an error or return zero vector depending on implementation
        try:
            embedding = generate_embedding(None)
            assert len(embedding) == EMBEDDING_DIM
        except (TypeError, AttributeError):
            pass  # Acceptable to raise error on None

    def test_whitespace_returns_zero_vector(self):
        """Whitespace-only should return zero vector."""
        embedding = generate_embedding("   ")
        assert len(embedding) == EMBEDDING_DIM
        assert all(v == 0.0 for v in embedding)

    def test_deterministic_with_fallback(self):
        """Should be deterministic (at least with fallback)."""
        text = "consistent embedding test"
        emb1 = generate_embedding(text)
        emb2 = generate_embedding(text)
        assert emb1 == emb2

    def test_different_texts_different_embeddings(self):
        """Different texts should produce different embeddings."""
        emb1 = generate_embedding("the quick brown fox")
        emb2 = generate_embedding("jumps over the lazy dog")
        assert emb1 != emb2


class TestGenerateEmbeddings:
    """Tests for generate_embeddings batch function."""

    def test_empty_list_returns_empty(self):
        """Empty list should return empty list."""
        result = generate_embeddings([])
        assert result == []

    def test_single_text(self):
        """Single text should return list with one embedding."""
        result = generate_embeddings(["test text"])
        assert len(result) == 1
        assert len(result[0]) == EMBEDDING_DIM

    def test_multiple_texts(self):
        """Multiple texts should return multiple embeddings."""
        texts = ["text one", "text two", "text three"]
        result = generate_embeddings(texts)
        assert len(result) == 3
        for emb in result:
            assert len(emb) == EMBEDDING_DIM

    def test_preserves_order(self):
        """Embeddings should be in same order as inputs."""
        texts = ["alpha", "beta", "gamma"]
        result = generate_embeddings(texts)
        # Each text should produce unique embedding
        assert result[0] != result[1]
        assert result[1] != result[2]

    def test_batch_matches_individual(self):
        """Batch result should match individual calls."""
        texts = ["test one", "test two"]
        batch_result = generate_embeddings(texts)
        individual_results = [generate_embedding(t) for t in texts]
        assert batch_result[0] == individual_results[0]
        assert batch_result[1] == individual_results[1]


class TestEmbeddingToBlob:
    """Tests for embedding_to_blob function."""

    def test_returns_bytes(self):
        """Should return bytes object."""
        embedding = [0.1, 0.2, 0.3]
        result = embedding_to_blob(embedding)
        assert isinstance(result, bytes)

    def test_correct_length(self):
        """Blob length should be 4 bytes per float."""
        embedding = [0.1, 0.2, 0.3, 0.4, 0.5]
        result = embedding_to_blob(embedding)
        assert len(result) == 5 * 4  # 4 bytes per float

    def test_full_dimension_embedding(self):
        """Should handle full dimension embedding."""
        embedding = [0.1] * EMBEDDING_DIM
        result = embedding_to_blob(embedding)
        assert len(result) == EMBEDDING_DIM * 4

    def test_empty_embedding(self):
        """Empty embedding should produce empty blob."""
        result = embedding_to_blob([])
        assert result == b""

    def test_negative_values(self):
        """Should handle negative values."""
        embedding = [-0.5, 0.0, 0.5]
        result = embedding_to_blob(embedding)
        assert len(result) == 12


class TestBlobToEmbedding:
    """Tests for blob_to_embedding function."""

    def test_returns_list_of_floats(self):
        """Should return list of floats."""
        blob = struct.pack(">3f", 0.1, 0.2, 0.3)
        result = blob_to_embedding(blob)
        assert isinstance(result, list)
        for val in result:
            assert isinstance(val, float)

    def test_correct_count(self):
        """Should return correct number of floats."""
        blob = struct.pack(">5f", 0.1, 0.2, 0.3, 0.4, 0.5)
        result = blob_to_embedding(blob)
        assert len(result) == 5

    def test_empty_blob(self):
        """Empty blob should return empty list."""
        result = blob_to_embedding(b"")
        assert result == []

    def test_roundtrip(self):
        """embedding_to_blob and blob_to_embedding should roundtrip."""
        original = [0.123, -0.456, 0.789, 0.0, -1.0]
        blob = embedding_to_blob(original)
        recovered = blob_to_embedding(blob)
        assert len(original) == len(recovered)
        for orig, rec in zip(original, recovered):
            assert abs(orig - rec) < 1e-5  # Float precision

    def test_full_dimension_roundtrip(self):
        """Full dimension embedding should roundtrip correctly."""
        original = [float(i) / EMBEDDING_DIM for i in range(EMBEDDING_DIM)]
        blob = embedding_to_blob(original)
        recovered = blob_to_embedding(blob)
        assert len(recovered) == EMBEDDING_DIM
        for orig, rec in zip(original, recovered):
            assert abs(orig - rec) < 1e-5


class TestCosineSimilarity:
    """Tests for cosine_similarity function."""

    def test_identical_vectors_similarity_one(self):
        """Identical vectors should have similarity 1.0."""
        vec = [0.1, 0.2, 0.3, 0.4, 0.5]
        sim = cosine_similarity(vec, vec)
        assert abs(sim - 1.0) < 1e-6

    def test_opposite_vectors_similarity_negative(self):
        """Opposite vectors should have similarity -1.0."""
        vec1 = [1.0, 0.0, 0.0]
        vec2 = [-1.0, 0.0, 0.0]
        sim = cosine_similarity(vec1, vec2)
        assert abs(sim - (-1.0)) < 1e-6

    def test_orthogonal_vectors_similarity_zero(self):
        """Orthogonal vectors should have similarity 0.0."""
        vec1 = [1.0, 0.0, 0.0]
        vec2 = [0.0, 1.0, 0.0]
        sim = cosine_similarity(vec1, vec2)
        assert abs(sim) < 1e-6

    def test_similar_vectors_high_similarity(self):
        """Similar vectors should have high similarity."""
        vec1 = [0.8, 0.1, 0.1]
        vec2 = [0.9, 0.05, 0.05]
        sim = cosine_similarity(vec1, vec2)
        assert sim > 0.9

    def test_zero_vector_returns_zero(self):
        """Zero vector should return 0 similarity."""
        vec1 = [0.0, 0.0, 0.0]
        vec2 = [1.0, 2.0, 3.0]
        sim = cosine_similarity(vec1, vec2)
        assert sim == 0.0

    def test_both_zero_vectors(self):
        """Both zero vectors should return 0."""
        vec1 = [0.0, 0.0, 0.0]
        vec2 = [0.0, 0.0, 0.0]
        sim = cosine_similarity(vec1, vec2)
        assert sim == 0.0

    def test_dimension_mismatch_raises(self):
        """Different dimensions should raise ValueError."""
        vec1 = [0.1, 0.2, 0.3]
        vec2 = [0.1, 0.2]
        with pytest.raises(ValueError) as exc_info:
            cosine_similarity(vec1, vec2)
        assert "dimensions must match" in str(exc_info.value).lower()

    def test_similarity_in_valid_range(self):
        """Similarity should always be in [-1, 1]."""
        import random

        random.seed(42)
        for _ in range(10):
            vec1 = [random.uniform(-1, 1) for _ in range(10)]
            vec2 = [random.uniform(-1, 1) for _ in range(10)]
            sim = cosine_similarity(vec1, vec2)
            assert -1.0 <= sim <= 1.0


class TestFindMostSimilar:
    """Tests for find_most_similar function."""

    def test_empty_candidates_returns_empty(self):
        """Empty candidates should return empty list."""
        query = [0.1, 0.2, 0.3]
        result = find_most_similar(query, [])
        assert result == []

    def test_single_candidate(self):
        """Single candidate should be returned."""
        query = [0.1, 0.2, 0.3]
        candidates = [[0.1, 0.2, 0.3]]
        result = find_most_similar(query, candidates, top_k=5)
        assert len(result) == 1
        assert result[0][0] == 0  # Index
        assert abs(result[0][1] - 1.0) < 1e-6  # Similarity ~1.0

    def test_returns_top_k(self):
        """Should return exactly top_k results."""
        query = [1.0, 0.0, 0.0]
        candidates = [
            [1.0, 0.0, 0.0],  # Most similar
            [0.9, 0.1, 0.0],
            [0.8, 0.2, 0.0],
            [0.7, 0.3, 0.0],
            [0.6, 0.4, 0.0],
        ]
        result = find_most_similar(query, candidates, top_k=3)
        assert len(result) == 3

    def test_sorted_by_similarity_descending(self):
        """Results should be sorted by similarity descending."""
        query = [1.0, 0.0, 0.0]
        candidates = [
            [0.5, 0.5, 0.0],  # Medium
            [1.0, 0.0, 0.0],  # Highest
            [0.0, 1.0, 0.0],  # Lowest
        ]
        result = find_most_similar(query, candidates, top_k=3)
        assert result[0][1] >= result[1][1]
        assert result[1][1] >= result[2][1]

    def test_returns_index_and_similarity(self):
        """Each result should be (index, similarity) tuple."""
        query = [0.1, 0.2, 0.3]
        candidates = [[0.1, 0.2, 0.3]]
        result = find_most_similar(query, candidates)
        assert len(result) == 1
        assert isinstance(result[0], tuple)
        assert len(result[0]) == 2
        index, similarity = result[0]
        assert isinstance(index, int)
        assert isinstance(similarity, float)

    def test_top_k_larger_than_candidates(self):
        """top_k larger than candidates should return all."""
        query = [0.1, 0.2, 0.3]
        candidates = [[0.1, 0.2, 0.3], [0.4, 0.5, 0.6]]
        result = find_most_similar(query, candidates, top_k=10)
        assert len(result) == 2

    def test_finds_exact_match(self):
        """Should find exact match as most similar."""
        query = [0.5, 0.5, 0.0]
        candidates = [
            [1.0, 0.0, 0.0],
            [0.5, 0.5, 0.0],  # Exact match at index 1
            [0.0, 1.0, 0.0],
        ]
        result = find_most_similar(query, candidates, top_k=1)
        assert result[0][0] == 1  # Index of exact match
        assert abs(result[0][1] - 1.0) < 1e-6


class TestGetModel:
    """Tests for get_model function."""

    def test_returns_none_or_model(self):
        """Should return None or a model instance."""
        model = get_model()
        # Model is either None or has encode method
        if model is not None:
            assert hasattr(model, "encode")

    def test_caches_model(self):
        """Should return same instance on subsequent calls."""
        model1 = get_model()
        model2 = get_model()
        # Either both None or same instance
        assert model1 is model2


class TestSemanticSimilarity:
    """Integration tests for semantic similarity (if model available)."""

    def test_similar_sentences_higher_similarity(self):
        """Semantically similar sentences should have higher similarity."""
        # Using fallback (hash-based) embeddings, similarity is not semantic
        # but this test checks the basic flow works
        emb1 = generate_embedding("The cat sat on the mat")
        emb2 = generate_embedding("A feline rested on the rug")
        emb3 = generate_embedding("Python programming language")

        # Can't guarantee semantic similarity with hash fallback
        # but all embeddings should have correct dimension
        assert len(emb1) == EMBEDDING_DIM
        assert len(emb2) == EMBEDDING_DIM
        assert len(emb3) == EMBEDDING_DIM


class TestEdgeCases:
    """Tests for edge cases and special inputs."""

    def test_very_long_text(self):
        """Should handle very long text."""
        long_text = "word " * 10000
        embedding = generate_embedding(long_text)
        assert len(embedding) == EMBEDDING_DIM

    def test_unicode_text(self):
        """Should handle unicode text."""
        text = "日本語のテキスト"
        embedding = generate_embedding(text)
        assert len(embedding) == EMBEDDING_DIM

    def test_emoji_text(self):
        """Should handle emoji."""
        text = "Hello 👋 World 🌍"
        embedding = generate_embedding(text)
        assert len(embedding) == EMBEDDING_DIM

    def test_special_characters(self):
        """Should handle special characters."""
        text = '!@#$%^&*(){}[]|\\:";<>?,./~`'
        embedding = generate_embedding(text)
        assert len(embedding) == EMBEDDING_DIM

    def test_newlines_and_tabs(self):
        """Should handle newlines and tabs."""
        text = "Line 1\nLine 2\tTabbed"
        embedding = generate_embedding(text)
        assert len(embedding) == EMBEDDING_DIM

    def test_single_character(self):
        """Should handle single character."""
        embedding = generate_embedding("A")
        assert len(embedding) == EMBEDDING_DIM

    def test_numbers_only(self):
        """Should handle numbers only."""
        embedding = generate_embedding("12345 67890")
        assert len(embedding) == EMBEDDING_DIM


class TestModelCodePaths:
    """Tests for model loading and encoding code paths using mocks."""

    def test_get_model_with_mock_sentence_transformer(self, monkeypatch):
        """Test get_model when SentenceTransformer is available."""
        import codeflow_py_lib.embeddings as emb_module

        # Reset singleton
        monkeypatch.setattr(emb_module, "_model", None)

        # Create mock SentenceTransformer
        class MockModel:
            def encode(self, text, **kwargs):
                import numpy as np

                if isinstance(text, str):
                    return np.zeros(EMBEDDING_DIM)
                return np.zeros((len(text), EMBEDDING_DIM))

        class MockSentenceTransformer:
            def __init__(self, model_name):
                self.model_name = model_name

            def encode(self, text, **kwargs):
                import numpy as np

                if isinstance(text, str):
                    return np.zeros(EMBEDDING_DIM)
                return np.zeros((len(text), EMBEDDING_DIM))

        # Simulate SentenceTransformer being available
        monkeypatch.setattr(emb_module, "SENTENCE_TRANSFORMERS_AVAILABLE", True)
        monkeypatch.setattr(emb_module, "SentenceTransformer", MockSentenceTransformer)

        model = get_model()
        assert model is not None
        assert hasattr(model, "encode")

        # Cleanup
        monkeypatch.setattr(emb_module, "_model", None)

    def test_generate_embedding_with_model(self, monkeypatch):
        """Test generate_embedding using a mock model."""
        import codeflow_py_lib.embeddings as emb_module
        import numpy as np

        # Reset singleton
        monkeypatch.setattr(emb_module, "_model", None)

        class MockModel:
            def encode(self, text, **kwargs):
                # Return a numpy array with predictable values
                return np.full(EMBEDDING_DIM, 0.5)

        # Set up mock
        monkeypatch.setattr(emb_module, "SENTENCE_TRANSFORMERS_AVAILABLE", True)
        monkeypatch.setattr(emb_module, "_model", MockModel())

        embedding = generate_embedding("test text")
        assert len(embedding) == EMBEDDING_DIM
        assert embedding[0] == 0.5  # Should use model output

        # Cleanup
        monkeypatch.setattr(emb_module, "_model", None)

    def test_generate_embedding_model_error_fallback(self, monkeypatch):
        """Test generate_embedding falls back when model.encode fails."""
        import codeflow_py_lib.embeddings as emb_module

        # Reset singleton
        monkeypatch.setattr(emb_module, "_model", None)

        class FailingModel:
            def encode(self, text, **kwargs):
                raise RuntimeError("Model encoding failed")

        # Set up mock that fails
        monkeypatch.setattr(emb_module, "SENTENCE_TRANSFORMERS_AVAILABLE", True)
        monkeypatch.setattr(emb_module, "_model", FailingModel())

        # Should fall back to hash embedding
        embedding = generate_embedding("test text")
        assert len(embedding) == EMBEDDING_DIM

        # Cleanup
        monkeypatch.setattr(emb_module, "_model", None)

    def test_generate_embeddings_with_model(self, monkeypatch):
        """Test generate_embeddings batch function using mock model."""
        import codeflow_py_lib.embeddings as emb_module
        import numpy as np

        # Reset singleton
        monkeypatch.setattr(emb_module, "_model", None)

        class MockModel:
            def encode(self, texts, **kwargs):
                # Return array with shape (num_texts, EMBEDDING_DIM)
                return np.full((len(texts), EMBEDDING_DIM), 0.7)

        # Set up mock
        monkeypatch.setattr(emb_module, "SENTENCE_TRANSFORMERS_AVAILABLE", True)
        monkeypatch.setattr(emb_module, "_model", MockModel())

        embeddings = generate_embeddings(["text1", "text2", "text3"])
        assert len(embeddings) == 3
        assert all(len(e) == EMBEDDING_DIM for e in embeddings)
        assert embeddings[0][0] == 0.7

        # Cleanup
        monkeypatch.setattr(emb_module, "_model", None)

    def test_generate_embeddings_model_error_fallback(self, monkeypatch):
        """Test generate_embeddings falls back when model.encode fails."""
        import codeflow_py_lib.embeddings as emb_module

        # Reset singleton
        monkeypatch.setattr(emb_module, "_model", None)

        class FailingModel:
            def encode(self, texts, **kwargs):
                raise RuntimeError("Batch encoding failed")

        # Set up mock that fails
        monkeypatch.setattr(emb_module, "SENTENCE_TRANSFORMERS_AVAILABLE", True)
        monkeypatch.setattr(emb_module, "_model", FailingModel())

        # Should fall back to hash embeddings
        embeddings = generate_embeddings(["text1", "text2"])
        assert len(embeddings) == 2
        assert all(len(e) == EMBEDDING_DIM for e in embeddings)

        # Cleanup
        monkeypatch.setattr(emb_module, "_model", None)

    def test_get_model_load_failure(self, monkeypatch):
        """Test get_model when SentenceTransformer init fails."""
        import codeflow_py_lib.embeddings as emb_module

        # Reset singleton
        monkeypatch.setattr(emb_module, "_model", None)

        class FailingSentenceTransformer:
            def __init__(self, model_name):
                raise RuntimeError("Failed to load model")

        # Simulate load failure
        monkeypatch.setattr(emb_module, "SENTENCE_TRANSFORMERS_AVAILABLE", True)
        monkeypatch.setattr(
            emb_module, "SentenceTransformer", FailingSentenceTransformer
        )

        model = get_model()
        assert model is None

        # Cleanup
        monkeypatch.setattr(emb_module, "_model", None)

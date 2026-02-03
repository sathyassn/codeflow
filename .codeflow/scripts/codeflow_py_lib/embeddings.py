"""
embeddings.py - Local embedding generation for semantic search.

Provides functions to generate vector embeddings using the all-MiniLM-L6-v2 model.
Falls back to a simple hash-based embedding if sentence-transformers is not available.
"""

import hashlib
import struct
from typing import List, Optional

import numpy as np

from .logging import get_logger

logger = get_logger(__name__)

# Model configuration
DEFAULT_MODEL = "all-MiniLM-L6-v2"
EMBEDDING_DIM = 384  # Dimension for all-MiniLM-L6-v2

# Try to import sentence-transformers
try:
    from sentence_transformers import SentenceTransformer

    SENTENCE_TRANSFORMERS_AVAILABLE = True
except ImportError:
    SentenceTransformer = None
    SENTENCE_TRANSFORMERS_AVAILABLE = False

# Singleton model instance
_model: Optional[object] = None


def get_model(model_name: str = DEFAULT_MODEL) -> Optional[object]:
    """Get or load the sentence transformer model.

    Args:
        model_name: Name of the model to load

    Returns:
        SentenceTransformer model instance, or None if not available
    """
    global _model
    if _model is None and SENTENCE_TRANSFORMERS_AVAILABLE:
        try:
            logger.info(f"Loading embedding model: {model_name}")
            _model = SentenceTransformer(model_name)
        except Exception as e:
            logger.warning(f"Failed to load model {model_name}: {e}")
            _model = None
    return _model


def generate_embedding(
    text: str,
    model_name: str = DEFAULT_MODEL,
) -> List[float]:
    """Generate embedding vector for a single text.

    Args:
        text: Text to embed
        model_name: Model to use for embedding

    Returns:
        List of floats representing the embedding vector (384 dimensions)
    """
    if not text or not text.strip():
        # Return zero vector for empty text
        return [0.0] * EMBEDDING_DIM

    model = get_model(model_name)

    if model is not None:
        try:
            embedding = model.encode(text, convert_to_numpy=True)
            return embedding.tolist()
        except Exception as e:
            logger.warning(f"Model encoding failed, using fallback: {e}")

    # Fallback: hash-based pseudo-embedding
    return _hash_embedding(text)


def generate_embeddings(
    texts: List[str],
    model_name: str = DEFAULT_MODEL,
    batch_size: int = 32,
) -> List[List[float]]:
    """Generate embedding vectors for multiple texts.

    Args:
        texts: List of texts to embed
        model_name: Model to use for embedding
        batch_size: Batch size for encoding

    Returns:
        List of embedding vectors
    """
    if not texts:
        return []

    model = get_model(model_name)

    if model is not None:
        try:
            embeddings = model.encode(
                texts,
                convert_to_numpy=True,
                batch_size=batch_size,
                show_progress_bar=False,
            )
            return embeddings.tolist()
        except Exception as e:
            logger.warning(f"Batch encoding failed, using fallback: {e}")

    # Fallback: hash-based pseudo-embeddings
    return [_hash_embedding(text) for text in texts]


def _hash_embedding(text: str) -> List[float]:
    """Generate a deterministic pseudo-embedding using SHA-256 hash.

    This is a fallback when sentence-transformers is not available.
    It's not semantically meaningful but provides consistent vectors.

    Args:
        text: Text to create pseudo-embedding for

    Returns:
        List of 384 floats derived from hash
    """
    if not text or not text.strip():
        return [0.0] * EMBEDDING_DIM

    # Generate multiple hashes to fill 384 dimensions
    embeddings = []
    current_text = text

    while len(embeddings) < EMBEDDING_DIM:
        # Hash the current text
        hash_bytes = hashlib.sha256(current_text.encode("utf-8")).digest()

        # Convert bytes to floats in range [-1, 1]
        for i in range(0, len(hash_bytes), 4):
            if len(embeddings) >= EMBEDDING_DIM:
                break
            # Unpack 4 bytes as a 32-bit integer
            value = struct.unpack(">I", hash_bytes[i : i + 4])[0]
            # Normalize to [-1, 1]
            normalized = (value / (2**32 - 1)) * 2 - 1
            embeddings.append(normalized)

        # Modify text for next iteration
        current_text = hash_bytes.hex() + current_text

    return embeddings[:EMBEDDING_DIM]


def embedding_to_blob(embedding: List[float]) -> bytes:
    """Convert embedding to bytes for SQLite BLOB storage.

    Args:
        embedding: List of floats

    Returns:
        Bytes representation of the embedding
    """
    return struct.pack(f">{len(embedding)}f", *embedding)


def blob_to_embedding(blob: bytes) -> List[float]:
    """Convert SQLite BLOB to embedding list.

    Args:
        blob: Bytes from SQLite BLOB column

    Returns:
        List of floats
    """
    count = len(blob) // 4  # 4 bytes per float
    return list(struct.unpack(f">{count}f", blob))


def cosine_similarity(vec1: List[float], vec2: List[float]) -> float:
    """Calculate cosine similarity between two vectors.

    Args:
        vec1: First embedding vector
        vec2: Second embedding vector

    Returns:
        Cosine similarity score in range [-1, 1]
    """
    if len(vec1) != len(vec2):
        raise ValueError(f"Vector dimensions must match: {len(vec1)} != {len(vec2)}")

    # Convert to numpy for efficient computation
    a = np.array(vec1)
    b = np.array(vec2)

    dot_product = np.dot(a, b)
    norm_a = np.linalg.norm(a)
    norm_b = np.linalg.norm(b)

    if norm_a == 0 or norm_b == 0:
        return 0.0

    return float(dot_product / (norm_a * norm_b))


def find_most_similar(
    query_embedding: List[float],
    candidate_embeddings: List[List[float]],
    top_k: int = 5,
) -> List[tuple]:
    """Find the most similar embeddings to a query.

    Args:
        query_embedding: The query embedding vector
        candidate_embeddings: List of candidate embeddings to search
        top_k: Number of top results to return

    Returns:
        List of (index, similarity_score) tuples, sorted by similarity descending
    """
    if not candidate_embeddings:
        return []

    similarities = [
        (i, cosine_similarity(query_embedding, emb))
        for i, emb in enumerate(candidate_embeddings)
    ]

    # Sort by similarity descending
    similarities.sort(key=lambda x: x[1], reverse=True)

    return similarities[:top_k]


__all__ = [
    "generate_embedding",
    "generate_embeddings",
    "embedding_to_blob",
    "blob_to_embedding",
    "cosine_similarity",
    "find_most_similar",
    "get_model",
    "DEFAULT_MODEL",
    "EMBEDDING_DIM",
    "SENTENCE_TRANSFORMERS_AVAILABLE",
]

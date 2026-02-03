"""
chunking.py - Sentence-aware text chunking for embeddings.

Provides functions to split text into chunks of approximately 200 tokens
with 50-token overlap for semantic search accuracy.
"""

import re
from dataclasses import dataclass
from typing import List, Tuple

from .logging import get_logger

logger = get_logger(__name__)

# Default chunking parameters
DEFAULT_CHUNK_SIZE = 200  # Target tokens per chunk
DEFAULT_OVERLAP = 50  # Overlap tokens between chunks
CHARS_PER_TOKEN = 4  # Approximate characters per token

# Sentence boundary patterns
SENTENCE_END_PATTERN = re.compile(r"(?<=[.!?])\s+(?=[A-Z])")
PARAGRAPH_PATTERN = re.compile(r"\n\s*\n")


@dataclass
class TextChunk:
    """Represents a text chunk with metadata."""

    index: int
    text: str
    char_offset_start: int
    char_offset_end: int
    token_estimate: int
    has_overlap_prev: bool
    has_overlap_next: bool


def estimate_tokens(text: str) -> int:
    """Estimate the number of tokens in text.

    Uses a simple character-based heuristic (approximately 4 chars per token).

    Args:
        text: Text to estimate tokens for

    Returns:
        Estimated token count
    """
    if not text:
        return 0
    # Simple heuristic: ~4 characters per token for English text
    return max(1, len(text.strip()) // CHARS_PER_TOKEN)


def split_into_sentences(text: str) -> List[str]:
    """Split text into sentences.

    Uses regex patterns to identify sentence boundaries while
    handling common edge cases like abbreviations.

    Args:
        text: Text to split

    Returns:
        List of sentences
    """
    if not text or not text.strip():
        return []

    # First, normalize whitespace
    text = re.sub(r"\s+", " ", text.strip())

    # Split on sentence-ending punctuation followed by space and capital letter
    sentences = SENTENCE_END_PATTERN.split(text)

    # Clean up and filter empty sentences
    return [s.strip() for s in sentences if s.strip()]


def split_into_paragraphs(text: str) -> List[str]:
    """Split text into paragraphs.

    Args:
        text: Text to split

    Returns:
        List of paragraphs
    """
    if not text or not text.strip():
        return []

    paragraphs = PARAGRAPH_PATTERN.split(text)
    return [p.strip() for p in paragraphs if p.strip()]


def chunk_text(
    text: str,
    chunk_size: int = DEFAULT_CHUNK_SIZE,
    overlap: int = DEFAULT_OVERLAP,
) -> List[TextChunk]:
    """Split text into overlapping chunks with sentence awareness.

    Chunks are created to be approximately chunk_size tokens with
    overlap tokens shared between consecutive chunks. Chunk boundaries
    are aligned to sentence boundaries where possible.

    Args:
        text: Text to chunk
        chunk_size: Target number of tokens per chunk
        overlap: Number of tokens to overlap between chunks

    Returns:
        List of TextChunk objects
    """
    if not text or not text.strip():
        return []

    # Get sentences
    sentences = split_into_sentences(text)
    if not sentences:  # pragma: no cover
        # Defensive fallback: split_into_sentences always returns at least [text]
        # for non-empty input, so this branch is unreachable in normal operation
        return _chunk_by_characters(text, chunk_size, overlap)

    chunks: List[TextChunk] = []
    current_sentences: List[str] = []
    current_tokens = 0
    char_offset = 0

    for sentence in sentences:
        sentence_tokens = estimate_tokens(sentence)

        # If adding this sentence would exceed chunk_size, create a chunk
        if current_tokens + sentence_tokens > chunk_size and current_sentences:
            chunk = _create_chunk(
                chunks=chunks,
                sentences=current_sentences,
                text=text,
                char_offset=char_offset,
            )
            chunks.append(chunk)

            # Calculate overlap for next chunk
            overlap_sentences, overlap_tokens = _get_overlap_sentences(
                current_sentences, overlap
            )
            char_offset = chunk.char_offset_end - sum(
                len(s) + 1 for s in overlap_sentences
            )

            current_sentences = overlap_sentences
            current_tokens = overlap_tokens

        current_sentences.append(sentence)
        current_tokens += sentence_tokens

    # Create final chunk if there are remaining sentences
    if current_sentences:
        chunk = _create_chunk(
            chunks=chunks,
            sentences=current_sentences,
            text=text,
            char_offset=char_offset,
            is_last=True,
        )
        chunks.append(chunk)

    # Update has_overlap_next for all but last chunk
    for i in range(len(chunks) - 1):
        chunks[i] = TextChunk(
            index=chunks[i].index,
            text=chunks[i].text,
            char_offset_start=chunks[i].char_offset_start,
            char_offset_end=chunks[i].char_offset_end,
            token_estimate=chunks[i].token_estimate,
            has_overlap_prev=chunks[i].has_overlap_prev,
            has_overlap_next=True,
        )

    return chunks


def _create_chunk(
    chunks: List[TextChunk],
    sentences: List[str],
    text: str,
    char_offset: int,
    is_last: bool = False,
) -> TextChunk:
    """Create a TextChunk from a list of sentences.

    Args:
        chunks: Existing chunks (to determine index and overlap)
        sentences: Sentences to include in this chunk
        text: Original full text
        char_offset: Character offset for the start of this chunk
        is_last: Whether this is the last chunk

    Returns:
        TextChunk object
    """
    chunk_text = " ".join(sentences)
    chunk_index = len(chunks)

    # Find actual position in original text
    start = char_offset
    try:
        # Try to find the exact position
        first_sentence_pos = text.find(sentences[0], char_offset)
        if first_sentence_pos >= 0:
            start = first_sentence_pos
    except (IndexError, ValueError):
        pass

    end = start + len(chunk_text)

    return TextChunk(
        index=chunk_index,
        text=chunk_text,
        char_offset_start=start,
        char_offset_end=end,
        token_estimate=estimate_tokens(chunk_text),
        has_overlap_prev=chunk_index > 0,
        has_overlap_next=False,  # Updated later
    )


def _get_overlap_sentences(
    sentences: List[str],
    overlap_tokens: int,
) -> Tuple[List[str], int]:
    """Get sentences from the end that fit within overlap_tokens.

    Args:
        sentences: List of sentences
        overlap_tokens: Target overlap in tokens

    Returns:
        Tuple of (overlap_sentences, actual_token_count)
    """
    if not sentences:
        return [], 0

    overlap_sentences: List[str] = []
    tokens = 0

    # Work backwards from the end
    for sentence in reversed(sentences):
        sentence_tokens = estimate_tokens(sentence)
        if tokens + sentence_tokens > overlap_tokens and overlap_sentences:
            break
        overlap_sentences.insert(0, sentence)
        tokens += sentence_tokens

    return overlap_sentences, tokens


def _chunk_by_characters(  # pragma: no cover
    text: str,
    chunk_size: int,
    overlap: int,
) -> List[TextChunk]:
    """Fallback chunking by character count.

    Used when sentence detection fails. This is defensive code that is
    currently unreachable because split_into_sentences always returns
    at least the original text for non-empty input.

    Args:
        text: Text to chunk
        chunk_size: Target tokens per chunk
        overlap: Overlap tokens

    Returns:
        List of TextChunk objects
    """
    chunks: List[TextChunk] = []
    char_chunk_size = chunk_size * CHARS_PER_TOKEN
    char_overlap = overlap * CHARS_PER_TOKEN

    start = 0
    while start < len(text):
        end = min(start + char_chunk_size, len(text))
        chunk_text = text[start:end]

        chunks.append(
            TextChunk(
                index=len(chunks),
                text=chunk_text,
                char_offset_start=start,
                char_offset_end=end,
                token_estimate=estimate_tokens(chunk_text),
                has_overlap_prev=start > 0,
                has_overlap_next=end < len(text),
            )
        )

        start = end - char_overlap
        if start >= len(text) - char_overlap:
            break

    return chunks


def chunk_for_embedding(
    text: str,
    max_tokens: int = DEFAULT_CHUNK_SIZE,
) -> List[str]:
    """Simple interface to chunk text for embedding.

    Args:
        text: Text to chunk
        max_tokens: Maximum tokens per chunk

    Returns:
        List of text strings ready for embedding
    """
    chunks = chunk_text(text, chunk_size=max_tokens, overlap=DEFAULT_OVERLAP)
    return [c.text for c in chunks]


__all__ = [
    "TextChunk",
    "chunk_text",
    "chunk_for_embedding",
    "split_into_sentences",
    "split_into_paragraphs",
    "estimate_tokens",
    "DEFAULT_CHUNK_SIZE",
    "DEFAULT_OVERLAP",
]

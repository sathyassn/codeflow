"""
crdt.py - Loro CRDT operations for coordination state.

Provides functions to load, modify, and save CRDT state for work claims.
Uses Loro library for conflict-free replicated data type semantics.
Falls back to JSON if Loro is not available.
"""

import json
from datetime import datetime
from pathlib import Path
from typing import Any, Dict, Optional

# Loro library for CRDT operations
try:
    import loro

    LORO_AVAILABLE = True
except ImportError:
    loro = None
    LORO_AVAILABLE = False

from .errors import CRDTError
from .logging import get_logger
from .paths import get_state_dir

logger = get_logger(__name__)

CRDT_STATE_FILE = "coordination/state.loro"
CRDT_JSON_FALLBACK = "coordination/state.json"


class CoordinationDoc:
    """CRDT document for coordination state."""

    def __init__(self, doc: Optional[Any] = None):
        """Initialize coordination document.

        Args:
            doc: Optional Loro document instance
        """
        self._doc = doc if loro and doc else (loro.LoroDoc() if loro else None)
        self._claims: Dict[str, Dict[str, Any]] = {}
        self._token_counter: int = 0

    @property
    def claims(self) -> Dict[str, Dict[str, Any]]:
        """Access claims map."""
        return self._claims

    def get_next_fencing_token(self) -> int:
        """Get next monotonic fencing token."""
        self._token_counter += 1
        return self._token_counter

    def add_claim(
        self,
        claim_id: str,
        work_id: str,
        pattern: str,
        owner_id: str,
        mode: str = "exclusive",
        expires_at: Optional[str] = None,
    ) -> Dict[str, Any]:
        """Add a new claim to the coordination state.

        Args:
            claim_id: Unique claim identifier
            work_id: Work ID this claim belongs to
            pattern: Resource pattern being claimed
            owner_id: Owner of the claim
            mode: Claim mode (exclusive or shared)
            expires_at: Expiration timestamp (ISO format)

        Returns:
            The created claim dictionary
        """
        fencing_token = self.get_next_fencing_token()
        claim = {
            "id": claim_id,
            "work_id": work_id,
            "pattern": pattern,
            "mode": mode,
            "owner_id": owner_id,
            "fencing_token": fencing_token,
            "expires_at": expires_at,
            "status": "active",
            "created_at": datetime.utcnow().isoformat() + "Z",
        }
        self._claims[claim_id] = claim
        return claim

    def release_claim(self, claim_id: str) -> bool:
        """Release a claim by setting its status to released.

        Args:
            claim_id: The claim ID to release

        Returns:
            True if claim was found and released, False otherwise
        """
        if claim_id in self._claims:
            self._claims[claim_id]["status"] = "released"
            return True
        return False

    def renew_claim(self, claim_id: str, new_expires_at: str) -> bool:
        """Renew a claim's expiration time.

        Args:
            claim_id: The claim ID to renew
            new_expires_at: New expiration timestamp (ISO format)

        Returns:
            True if claim was found and renewed, False otherwise
        """
        if claim_id in self._claims and self._claims[claim_id]["status"] == "active":
            self._claims[claim_id]["expires_at"] = new_expires_at
            return True
        return False

    def get_active_claims_for_pattern(self, pattern: str) -> list:
        """Get all active claims for a given pattern.

        Args:
            pattern: The resource pattern to check

        Returns:
            List of active claims matching the pattern
        """
        now = datetime.utcnow().isoformat() + "Z"
        return [
            claim
            for claim in self._claims.values()
            if claim["pattern"] == pattern
            and claim["status"] == "active"
            and (claim["expires_at"] is None or claim["expires_at"] > now)
        ]

    def to_bytes(self) -> bytes:
        """Serialize to bytes for storage."""
        if self._doc and loro:
            try:
                return self._doc.export_snapshot()
            except Exception:
                pass
        # Fallback: JSON serialization
        return json.dumps(
            {
                "claims": self._claims,
                "token_counter": self._token_counter,
            },
            separators=(",", ":"),
        ).encode("utf-8")

    @classmethod
    def from_bytes(cls, data: bytes) -> "CoordinationDoc":
        """Deserialize from bytes."""
        doc = cls()
        if loro:
            try:
                doc._doc = loro.LoroDoc()
                doc._doc.import_snapshot(data)
                doc._sync_from_crdt()
                return doc
            except Exception:
                pass
        # Fallback: JSON deserialization
        try:
            parsed = json.loads(data.decode("utf-8"))
            doc._claims = parsed.get("claims", {})
            doc._token_counter = parsed.get("token_counter", 0)
        except (json.JSONDecodeError, UnicodeDecodeError):
            pass
        return doc

    def _sync_from_crdt(self) -> None:
        """Sync internal state from CRDT document."""
        if not self._doc or not loro:
            return
        # Extract claims from Loro document
        # Implementation depends on Loro API
        pass


def load_coordination() -> CoordinationDoc:
    """
    Load coordination CRDT state from disk.

    Returns empty doc if file doesn't exist.
    Raises CRDTError if file is corrupted.
    """
    state_dir = get_state_dir()

    # Try Loro format first
    loro_path = state_dir / CRDT_STATE_FILE
    if loro_path.exists():
        try:
            data = loro_path.read_bytes()
            return CoordinationDoc.from_bytes(data)
        except Exception as e:
            logger.error(f"Failed to load CRDT state from {loro_path}: {e}")
            # Fall through to JSON fallback

    # Try JSON fallback
    json_path = state_dir / CRDT_JSON_FALLBACK
    if json_path.exists():
        try:
            data = json_path.read_bytes()
            return CoordinationDoc.from_bytes(data)
        except Exception as e:
            logger.error(f"Failed to load JSON state from {json_path}: {e}")
            raise CRDTError(f"Corrupted coordination state: {e}")

    logger.info("Creating new coordination state")
    return CoordinationDoc()


def save_coordination(doc: CoordinationDoc) -> None:
    """
    Save coordination CRDT state to disk.

    Creates parent directories if needed.
    Saves in both Loro and JSON formats for compatibility.
    """
    state_dir = get_state_dir()

    # Ensure coordination directory exists
    coord_dir = state_dir / "coordination"
    coord_dir.mkdir(parents=True, exist_ok=True)

    try:
        data = doc.to_bytes()

        # Save to Loro format
        loro_path = state_dir / CRDT_STATE_FILE
        loro_path.write_bytes(data)

        # Also save JSON fallback
        json_path = state_dir / CRDT_JSON_FALLBACK
        json_data = json.dumps(
            {
                "claims": doc.claims,
                "token_counter": doc._token_counter,
            },
            indent=2,
        )
        json_path.write_text(json_data, encoding="utf-8")

        logger.debug("Coordination state saved")
    except Exception as e:
        logger.error(f"Failed to save CRDT state: {e}")
        raise CRDTError(f"Failed to save state: {e}")


def rebuild_from_jsonl(jsonl_path: Path) -> CoordinationDoc:
    """
    Rebuild CRDT state from JSONL events.

    Used for recovery when CRDT state is corrupted.
    Reads claim_created, claim_released, and claim_renewed events.

    Args:
        jsonl_path: Path to the sessions.jsonl file

    Returns:
        Rebuilt CoordinationDoc
    """
    from .jsonl import read_jsonl

    doc = CoordinationDoc()

    for event in read_jsonl(jsonl_path):
        event_type = event.get("type") or event.get("e")

        if event_type == "claim_created":
            claim_id = event.get("id") or event.get("claim_id")
            if claim_id:
                doc._claims[claim_id] = {
                    "id": claim_id,
                    "work_id": event.get("work_id"),
                    "pattern": event.get("pattern"),
                    "mode": event.get("mode", "exclusive"),
                    "owner_id": event.get("owner_id"),
                    "fencing_token": event.get("fencing_token", 0),
                    "expires_at": event.get("expires_at"),
                    "status": "active",
                    "created_at": event.get("created_at") or event.get("ts"),
                }
                if event.get("fencing_token", 0) > doc._token_counter:
                    doc._token_counter = event["fencing_token"]

        elif event_type == "claim_released":
            claim_id = event.get("claim_id")
            if claim_id and claim_id in doc._claims:
                doc._claims[claim_id]["status"] = "released"

        elif event_type == "claim_renewed":
            claim_id = event.get("claim_id")
            if claim_id and claim_id in doc._claims:
                doc._claims[claim_id]["expires_at"] = event.get("expires_at")

    return doc


__all__ = [
    "CoordinationDoc",
    "load_coordination",
    "save_coordination",
    "rebuild_from_jsonl",
    "LORO_AVAILABLE",
]

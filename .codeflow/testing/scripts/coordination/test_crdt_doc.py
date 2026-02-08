"""
test_crdt_doc.py - Tests for CoordinationDoc CRDT class.
"""

from datetime import datetime, timedelta, timezone

from codeflow_py_lib.crdt import CoordinationDoc


class TestCoordinationDocBasic:
    """Tests for basic CoordinationDoc operations."""

    def test_creates_empty_doc(self):
        """Should create empty document."""
        doc = CoordinationDoc()
        assert len(doc.claims) == 0
        assert doc._token_counter == 0

    def test_claims_property(self):
        """Should return claims dict."""
        doc = CoordinationDoc()
        assert isinstance(doc.claims, dict)


class TestCoordinationDocFencingToken:
    """Tests for fencing token generation."""

    def test_get_next_fencing_token(self):
        """Should return monotonically increasing tokens."""
        doc = CoordinationDoc()

        token1 = doc.get_next_fencing_token()
        token2 = doc.get_next_fencing_token()
        token3 = doc.get_next_fencing_token()

        assert token1 == 1
        assert token2 == 2
        assert token3 == 3

    def test_fencing_token_never_resets(self):
        """Should maintain token counter across operations."""
        doc = CoordinationDoc()
        doc._token_counter = 100

        token = doc.get_next_fencing_token()
        assert token == 101


class TestCoordinationDocAddClaim:
    """Tests for adding claims."""

    def test_add_claim(self):
        """Should add claim to document."""
        doc = CoordinationDoc()

        claim = doc.add_claim(
            claim_id="claim-001",
            work_id="TSK-001",
            pattern="file:src/*.ts",
            owner_id="agent-001",
        )

        assert "claim-001" in doc.claims
        assert claim["id"] == "claim-001"
        assert claim["work_id"] == "TSK-001"
        assert claim["status"] == "active"

    def test_add_claim_with_mode(self):
        """Should support shared mode."""
        doc = CoordinationDoc()

        claim = doc.add_claim(
            claim_id="claim-001",
            work_id="TSK-001",
            pattern="file:src/*.ts",
            owner_id="agent-001",
            mode="shared",
        )

        assert claim["mode"] == "shared"

    def test_add_claim_with_expiration(self):
        """Should set expiration time."""
        doc = CoordinationDoc()
        expires = (datetime.now(timezone.utc) + timedelta(hours=1)).isoformat() + "Z"

        claim = doc.add_claim(
            claim_id="claim-001",
            work_id="TSK-001",
            pattern="file:src/*.ts",
            owner_id="agent-001",
            expires_at=expires,
        )

        assert claim["expires_at"] == expires

    def test_add_claim_assigns_fencing_token(self):
        """Should assign unique fencing token to each claim."""
        doc = CoordinationDoc()

        claim1 = doc.add_claim(
            claim_id="claim-001",
            work_id="TSK-001",
            pattern="file:src/*.ts",
            owner_id="agent-001",
        )
        claim2 = doc.add_claim(
            claim_id="claim-002",
            work_id="TSK-002",
            pattern="file:src/*.js",
            owner_id="agent-002",
        )

        assert claim1["fencing_token"] == 1
        assert claim2["fencing_token"] == 2


class TestCoordinationDocReleaseClaim:
    """Tests for releasing claims."""

    def test_release_claim(self):
        """Should release existing claim."""
        doc = CoordinationDoc()
        doc.add_claim(
            claim_id="claim-001",
            work_id="TSK-001",
            pattern="file:src/*.ts",
            owner_id="agent-001",
        )

        result = doc.release_claim("claim-001")

        assert result is True
        assert doc.claims["claim-001"]["status"] == "released"

    def test_release_nonexistent_claim(self):
        """Should return False for nonexistent claim."""
        doc = CoordinationDoc()

        result = doc.release_claim("nonexistent")

        assert result is False


class TestCoordinationDocRenewClaim:
    """Tests for renewing claims."""

    def test_renew_claim(self):
        """Should update expiration time."""
        doc = CoordinationDoc()
        doc.add_claim(
            claim_id="claim-001",
            work_id="TSK-001",
            pattern="file:src/*.ts",
            owner_id="agent-001",
        )
        new_expires = (datetime.now(timezone.utc) + timedelta(hours=2)).isoformat() + "Z"

        result = doc.renew_claim("claim-001", new_expires)

        assert result is True
        assert doc.claims["claim-001"]["expires_at"] == new_expires

    def test_renew_nonexistent_claim(self):
        """Should return False for nonexistent claim."""
        doc = CoordinationDoc()

        result = doc.renew_claim("nonexistent", "2025-01-01T00:00:00Z")

        assert result is False

    def test_renew_released_claim(self):
        """Should not renew released claim."""
        doc = CoordinationDoc()
        doc.add_claim(
            claim_id="claim-001",
            work_id="TSK-001",
            pattern="file:src/*.ts",
            owner_id="agent-001",
        )
        doc.release_claim("claim-001")

        result = doc.renew_claim("claim-001", "2025-01-01T00:00:00Z")

        assert result is False


class TestCoordinationDocGetActiveClaims:
    """Tests for getting active claims."""

    def test_get_active_claims_for_pattern(self):
        """Should return active claims matching pattern."""
        doc = CoordinationDoc()
        expires = (datetime.now(timezone.utc) + timedelta(hours=1)).isoformat() + "Z"

        doc.add_claim(
            claim_id="claim-001",
            work_id="TSK-001",
            pattern="file:src/*.ts",
            owner_id="agent-001",
            expires_at=expires,
        )
        doc.add_claim(
            claim_id="claim-002",
            work_id="TSK-002",
            pattern="file:src/*.ts",
            owner_id="agent-002",
            expires_at=expires,
        )

        active = doc.get_active_claims_for_pattern("file:src/*.ts")

        assert len(active) == 2

    def test_excludes_released_claims(self):
        """Should exclude released claims."""
        doc = CoordinationDoc()
        expires = (datetime.now(timezone.utc) + timedelta(hours=1)).isoformat() + "Z"

        doc.add_claim(
            claim_id="claim-001",
            work_id="TSK-001",
            pattern="file:src/*.ts",
            owner_id="agent-001",
            expires_at=expires,
        )
        doc.release_claim("claim-001")

        active = doc.get_active_claims_for_pattern("file:src/*.ts")

        assert len(active) == 0

    def test_excludes_expired_claims(self):
        """Should exclude expired claims."""
        doc = CoordinationDoc()
        # Expired timestamp
        expired = (datetime.now(timezone.utc) - timedelta(hours=1)).isoformat() + "Z"

        doc.add_claim(
            claim_id="claim-001",
            work_id="TSK-001",
            pattern="file:src/*.ts",
            owner_id="agent-001",
            expires_at=expired,
        )

        active = doc.get_active_claims_for_pattern("file:src/*.ts")

        assert len(active) == 0


class TestCoordinationDocSerialization:
    """Tests for serialization/deserialization."""

    def test_to_bytes(self):
        """Should serialize to bytes."""
        doc = CoordinationDoc()
        doc.add_claim(
            claim_id="claim-001",
            work_id="TSK-001",
            pattern="file:src/*.ts",
            owner_id="agent-001",
        )

        data = doc.to_bytes()

        assert isinstance(data, bytes)
        # Should contain claim data
        assert b"claim-001" in data

    def test_from_bytes(self):
        """Should deserialize from bytes."""
        doc1 = CoordinationDoc()
        doc1.add_claim(
            claim_id="claim-001",
            work_id="TSK-001",
            pattern="file:src/*.ts",
            owner_id="agent-001",
        )

        data = doc1.to_bytes()
        doc2 = CoordinationDoc.from_bytes(data)

        assert "claim-001" in doc2.claims
        assert doc2.claims["claim-001"]["work_id"] == "TSK-001"

    def test_serialization_preserves_token_counter(self):
        """Should preserve token counter."""
        doc1 = CoordinationDoc()
        doc1._token_counter = 42

        data = doc1.to_bytes()
        doc2 = CoordinationDoc.from_bytes(data)

        assert doc2._token_counter == 42

    def test_from_bytes_handles_invalid_data(self):
        """Should handle invalid data gracefully."""
        doc = CoordinationDoc.from_bytes(b"invalid json data")

        assert len(doc.claims) == 0

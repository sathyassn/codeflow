"""
test_crdt.py - Tests for CRDT coordination module.

Tests CoordinationDoc class and load/save/rebuild functions.
"""

import json
import shutil
import tempfile
from pathlib import Path

import pytest
from codeflow_py_lib.crdt import (
    LORO_AVAILABLE,
    CoordinationDoc,
    load_coordination,
    rebuild_from_jsonl,
    save_coordination,
)


@pytest.fixture
def coordination_doc():
    """Create a fresh CoordinationDoc for testing."""
    return CoordinationDoc()


@pytest.fixture
def temp_state_dir(monkeypatch):
    """Create temporary state directory and patch get_state_dir."""
    temp_dir = tempfile.mkdtemp(prefix="codeflow_crdt_test_")
    state_dir = Path(temp_dir)
    (state_dir / "coordination").mkdir(parents=True)

    # Patch get_state_dir to return our temp directory
    import codeflow_py_lib.crdt as crdt_module

    monkeypatch.setattr(crdt_module, "get_state_dir", lambda: state_dir)

    yield state_dir

    shutil.rmtree(temp_dir, ignore_errors=True)


@pytest.fixture
def sample_jsonl_file(tmp_path):
    """Create sample JSONL file with claim events."""
    jsonl_path = tmp_path / "sessions.jsonl"
    events = [
        {
            "type": "claim_created",
            "claim_id": "claim-001",
            "work_id": "TSK-001",
            "pattern": "src/auth/*.py",
            "mode": "exclusive",
            "owner_id": "agent-1",
            "fencing_token": 1,
            "expires_at": None,
            "created_at": "2026-01-25T10:00:00Z",
        },
        {
            "type": "claim_created",
            "claim_id": "claim-002",
            "work_id": "TSK-002",
            "pattern": "src/api/*.py",
            "mode": "shared",
            "owner_id": "agent-2",
            "fencing_token": 2,
            "expires_at": "2026-01-25T12:00:00Z",
            "created_at": "2026-01-25T10:01:00Z",
        },
        {
            "type": "claim_released",
            "claim_id": "claim-001",
        },
        {
            "type": "claim_renewed",
            "claim_id": "claim-002",
            "expires_at": "2026-01-25T14:00:00Z",
        },
    ]
    with open(jsonl_path, "w") as f:
        for event in events:
            f.write(json.dumps(event) + "\n")
    return jsonl_path


class TestCoordinationDoc:
    """Tests for CoordinationDoc class."""

    def test_init_creates_empty_doc(self, coordination_doc):
        """New doc should have empty claims."""
        assert coordination_doc.claims == {}
        assert coordination_doc._token_counter == 0

    def test_get_next_fencing_token_increments(self, coordination_doc):
        """Fencing token should increment monotonically."""
        token1 = coordination_doc.get_next_fencing_token()
        token2 = coordination_doc.get_next_fencing_token()
        token3 = coordination_doc.get_next_fencing_token()
        assert token1 == 1
        assert token2 == 2
        assert token3 == 3

    def test_add_claim_creates_claim(self, coordination_doc):
        """add_claim should create claim with all fields."""
        claim = coordination_doc.add_claim(
            claim_id="claim-001",
            work_id="TSK-001",
            pattern="src/*.py",
            owner_id="agent-1",
            mode="exclusive",
        )
        assert claim["id"] == "claim-001"
        assert claim["work_id"] == "TSK-001"
        assert claim["pattern"] == "src/*.py"
        assert claim["owner_id"] == "agent-1"
        assert claim["mode"] == "exclusive"
        assert claim["status"] == "active"
        assert claim["fencing_token"] == 1

    def test_add_claim_with_expiry(self, coordination_doc):
        """add_claim should accept expiration time."""
        expires = "2026-01-25T12:00:00Z"
        claim = coordination_doc.add_claim(
            claim_id="claim-001",
            work_id="TSK-001",
            pattern="src/*.py",
            owner_id="agent-1",
            expires_at=expires,
        )
        assert claim["expires_at"] == expires

    def test_release_claim_changes_status(self, coordination_doc):
        """release_claim should set status to released."""
        coordination_doc.add_claim(
            claim_id="claim-001",
            work_id="TSK-001",
            pattern="src/*.py",
            owner_id="agent-1",
        )
        result = coordination_doc.release_claim("claim-001")
        assert result is True
        assert coordination_doc.claims["claim-001"]["status"] == "released"

    def test_release_claim_nonexistent(self, coordination_doc):
        """release_claim should return False for nonexistent claim."""
        result = coordination_doc.release_claim("nonexistent")
        assert result is False

    def test_renew_claim_updates_expiry(self, coordination_doc):
        """renew_claim should update expiration time."""
        coordination_doc.add_claim(
            claim_id="claim-001",
            work_id="TSK-001",
            pattern="src/*.py",
            owner_id="agent-1",
            expires_at="2026-01-25T10:00:00Z",
        )
        new_expiry = "2026-01-25T12:00:00Z"
        result = coordination_doc.renew_claim("claim-001", new_expiry)
        assert result is True
        assert coordination_doc.claims["claim-001"]["expires_at"] == new_expiry

    def test_renew_claim_nonexistent(self, coordination_doc):
        """renew_claim should return False for nonexistent claim."""
        result = coordination_doc.renew_claim("nonexistent", "2026-01-25T12:00:00Z")
        assert result is False

    def test_renew_claim_released(self, coordination_doc):
        """renew_claim should return False for released claim."""
        coordination_doc.add_claim(
            claim_id="claim-001",
            work_id="TSK-001",
            pattern="src/*.py",
            owner_id="agent-1",
        )
        coordination_doc.release_claim("claim-001")
        result = coordination_doc.renew_claim("claim-001", "2026-01-25T12:00:00Z")
        assert result is False

    def test_get_active_claims_for_pattern(self, coordination_doc):
        """Should return only active non-expired claims for pattern."""
        coordination_doc.add_claim(
            claim_id="claim-001",
            work_id="TSK-001",
            pattern="src/*.py",
            owner_id="agent-1",
        )
        coordination_doc.add_claim(
            claim_id="claim-002",
            work_id="TSK-002",
            pattern="src/*.py",
            owner_id="agent-2",
        )
        coordination_doc.add_claim(
            claim_id="claim-003",
            work_id="TSK-003",
            pattern="tests/*.py",
            owner_id="agent-3",
        )
        coordination_doc.release_claim("claim-002")

        active = coordination_doc.get_active_claims_for_pattern("src/*.py")
        assert len(active) == 1
        assert active[0]["id"] == "claim-001"

    def test_get_active_claims_excludes_expired(self, coordination_doc):
        """Should exclude expired claims."""
        # Add claim with past expiry
        past_expiry = "2020-01-01T00:00:00Z"  # Past date
        coordination_doc.add_claim(
            claim_id="claim-001",
            work_id="TSK-001",
            pattern="src/*.py",
            owner_id="agent-1",
            expires_at=past_expiry,
        )
        active = coordination_doc.get_active_claims_for_pattern("src/*.py")
        assert len(active) == 0


class TestCoordinationDocSerialization:
    """Tests for CoordinationDoc serialization."""

    def test_to_bytes_returns_bytes(self, coordination_doc):
        """to_bytes should return bytes."""
        data = coordination_doc.to_bytes()
        assert isinstance(data, bytes)

    def test_from_bytes_roundtrip(self, coordination_doc):
        """Should roundtrip through to_bytes/from_bytes."""
        coordination_doc.add_claim(
            claim_id="claim-001",
            work_id="TSK-001",
            pattern="src/*.py",
            owner_id="agent-1",
        )
        data = coordination_doc.to_bytes()
        restored = CoordinationDoc.from_bytes(data)

        assert "claim-001" in restored.claims
        assert restored.claims["claim-001"]["pattern"] == "src/*.py"

    def test_from_bytes_preserves_token_counter(self, coordination_doc):
        """Should preserve fencing token counter."""
        coordination_doc.get_next_fencing_token()
        coordination_doc.get_next_fencing_token()
        data = coordination_doc.to_bytes()
        restored = CoordinationDoc.from_bytes(data)
        assert restored._token_counter == 2


class TestLoadSaveCoordination:
    """Tests for load_coordination and save_coordination functions."""

    def test_load_creates_new_doc_if_missing(self, temp_state_dir):
        """load_coordination should create new doc if file doesn't exist."""
        doc = load_coordination()
        assert isinstance(doc, CoordinationDoc)
        assert doc.claims == {}

    def test_save_creates_files(self, temp_state_dir, coordination_doc):
        """save_coordination should create state files."""
        coordination_doc.add_claim(
            claim_id="claim-001",
            work_id="TSK-001",
            pattern="src/*.py",
            owner_id="agent-1",
        )
        save_coordination(coordination_doc)

        # Check files exist
        json_path = temp_state_dir / "coordination" / "state.json"
        assert json_path.exists()

    def test_save_load_roundtrip(self, temp_state_dir, coordination_doc):
        """Should roundtrip through save/load."""
        coordination_doc.add_claim(
            claim_id="claim-001",
            work_id="TSK-001",
            pattern="src/*.py",
            owner_id="agent-1",
            mode="shared",
        )
        save_coordination(coordination_doc)
        restored = load_coordination()

        assert "claim-001" in restored.claims
        assert restored.claims["claim-001"]["mode"] == "shared"


class TestRebuildFromJsonl:
    """Tests for rebuild_from_jsonl function."""

    def test_rebuild_from_empty_file(self, tmp_path):
        """Should handle empty JSONL file."""
        empty_file = tmp_path / "empty.jsonl"
        empty_file.touch()
        doc = rebuild_from_jsonl(empty_file)
        assert doc.claims == {}

    def test_rebuild_processes_claim_created(self, sample_jsonl_file):
        """Should process claim_created events."""
        doc = rebuild_from_jsonl(sample_jsonl_file)
        assert "claim-001" in doc.claims
        assert "claim-002" in doc.claims

    def test_rebuild_processes_claim_released(self, sample_jsonl_file):
        """Should process claim_released events."""
        doc = rebuild_from_jsonl(sample_jsonl_file)
        # claim-001 was released
        assert doc.claims["claim-001"]["status"] == "released"
        # claim-002 was not released
        assert doc.claims["claim-002"]["status"] == "active"

    def test_rebuild_processes_claim_renewed(self, sample_jsonl_file):
        """Should process claim_renewed events."""
        doc = rebuild_from_jsonl(sample_jsonl_file)
        # claim-002 was renewed
        assert doc.claims["claim-002"]["expires_at"] == "2026-01-25T14:00:00Z"

    def test_rebuild_updates_token_counter(self, sample_jsonl_file):
        """Should update token counter to max fencing token."""
        doc = rebuild_from_jsonl(sample_jsonl_file)
        # Max fencing token in test data is 2
        assert doc._token_counter == 2


class TestLoroAvailability:
    """Tests for Loro library availability check."""

    def test_loro_available_is_boolean(self):
        """LORO_AVAILABLE should be a boolean."""
        assert isinstance(LORO_AVAILABLE, bool)


class TestCoordinationDocEdgeCases:
    """Tests for edge cases and error handling in CoordinationDoc."""

    def test_from_bytes_with_invalid_json(self):
        """from_bytes should handle invalid JSON gracefully."""
        invalid_data = b"not valid json at all {"
        doc = CoordinationDoc.from_bytes(invalid_data)
        # Should return empty doc on parse error
        assert doc.claims == {}
        assert doc._token_counter == 0

    def test_from_bytes_with_invalid_utf8(self):
        """from_bytes should handle invalid UTF-8 gracefully."""
        invalid_data = b"\xff\xfe invalid utf8"
        doc = CoordinationDoc.from_bytes(invalid_data)
        # Should return empty doc on decode error
        assert doc.claims == {}

    def test_from_bytes_with_empty_json(self):
        """from_bytes should handle empty JSON object."""
        empty_data = b"{}"
        doc = CoordinationDoc.from_bytes(empty_data)
        assert doc.claims == {}
        assert doc._token_counter == 0

    def test_from_bytes_with_partial_data(self):
        """from_bytes should handle partial JSON data."""
        partial_data = b'{"claims": {"c1": {"id": "c1"}}}'
        doc = CoordinationDoc.from_bytes(partial_data)
        assert "c1" in doc.claims
        assert doc._token_counter == 0  # Missing token_counter defaults to 0

    def test_init_with_none_doc(self):
        """CoordinationDoc should handle None doc parameter."""
        doc = CoordinationDoc(doc=None)
        assert doc.claims == {}


class TestLoadCoordinationEdgeCases:
    """Tests for edge cases in load_coordination."""

    def test_load_with_loro_file_read_error_falls_through(
        self, temp_state_dir, monkeypatch
    ):
        """Should fall through to JSON when Loro file raises exception."""
        # Create Loro file that will cause read error
        loro_path = temp_state_dir / "coordination" / "state.loro"
        loro_path.write_bytes(b"data")

        # Create valid JSON fallback
        json_path = temp_state_dir / "coordination" / "state.json"
        json_path.write_text('{"claims": {"c1": {"id": "c1"}}, "token_counter": 5}')

        # Make from_bytes raise exception for Loro file
        original_from_bytes = CoordinationDoc.from_bytes
        call_count = [0]

        @classmethod
        def mock_from_bytes(cls, data):
            call_count[0] += 1
            if call_count[0] == 1:  # First call (Loro file)
                raise ValueError("Simulated Loro parse error")
            return original_from_bytes(data)

        monkeypatch.setattr(CoordinationDoc, "from_bytes", mock_from_bytes)

        doc = load_coordination()
        # Should load from JSON fallback
        assert "c1" in doc.claims
        assert doc._token_counter == 5

    def test_load_with_json_read_error_raises(self, temp_state_dir, monkeypatch):
        """Should raise CRDTError when JSON file read raises exception."""
        from codeflow_py_lib.errors import CRDTError

        # Create JSON file (no Loro file)
        json_path = temp_state_dir / "coordination" / "state.json"
        json_path.write_text('{"claims": {}}')

        # Make from_bytes raise exception
        def mock_from_bytes(data):
            raise IOError("Simulated read error")

        monkeypatch.setattr(CoordinationDoc, "from_bytes", mock_from_bytes)

        with pytest.raises(CRDTError) as exc_info:
            load_coordination()
        assert "Corrupted" in str(exc_info.value) or "read error" in str(exc_info.value)

    def test_load_with_only_loro_file(self, temp_state_dir):
        """Should load from Loro file when only it exists."""
        # Create valid JSON-encoded data in Loro path (simulating JSON fallback format)
        loro_path = temp_state_dir / "coordination" / "state.loro"
        loro_path.write_bytes(b'{"claims": {"c1": {"id": "c1"}}, "token_counter": 3}')

        doc = load_coordination()
        assert "c1" in doc.claims

    def test_load_with_invalid_loro_returns_empty(self, temp_state_dir):
        """Loading with invalid Loro data returns empty doc (from_bytes handles gracefully)."""
        # Create invalid Loro file (no JSON fallback)
        loro_path = temp_state_dir / "coordination" / "state.loro"
        loro_path.write_bytes(b"not valid json or loro data")

        doc = load_coordination()
        # from_bytes handles invalid data gracefully, returns empty doc
        assert doc.claims == {}


class TestSaveCoordinationEdgeCases:
    """Tests for edge cases in save_coordination."""

    def test_save_creates_coordination_directory(self, temp_state_dir):
        """save_coordination should create coordination dir if missing."""
        import shutil

        # Remove coordination directory
        coord_dir = temp_state_dir / "coordination"
        shutil.rmtree(coord_dir)
        assert not coord_dir.exists()

        doc = CoordinationDoc()
        doc.add_claim(
            claim_id="c1",
            work_id="w1",
            pattern="*.py",
            owner_id="o1",
        )
        save_coordination(doc)

        assert coord_dir.exists()
        assert (coord_dir / "state.json").exists()

    def test_save_with_write_error_raises(self, temp_state_dir, monkeypatch):
        """save_coordination should raise CRDTError on write failure."""
        from codeflow_py_lib.errors import CRDTError

        doc = CoordinationDoc()
        doc.add_claim(
            claim_id="c1",
            work_id="w1",
            pattern="*.py",
            owner_id="o1",
        )

        # Make the directory read-only to cause write failure
        def mock_write_bytes(self, data):
            raise PermissionError("Cannot write to file")

        monkeypatch.setattr(Path, "write_bytes", mock_write_bytes)

        with pytest.raises(CRDTError) as exc_info:
            save_coordination(doc)
        assert "Failed to save" in str(exc_info.value)


class TestRebuildFromJsonlEdgeCases:
    """Tests for edge cases in rebuild_from_jsonl."""

    def test_rebuild_with_e_event_type(self, tmp_path):
        """Should handle 'e' field as event type (alternative format)."""
        jsonl_path = tmp_path / "sessions.jsonl"
        events = [
            {
                "e": "claim_created",
                "id": "claim-001",
                "work_id": "TSK-001",
                "pattern": "src/*.py",
                "owner_id": "agent-1",
                "fencing_token": 5,
                "ts": "2026-01-25T10:00:00Z",
            },
        ]
        with open(jsonl_path, "w") as f:
            for event in events:
                f.write(json.dumps(event) + "\n")

        doc = rebuild_from_jsonl(jsonl_path)
        assert "claim-001" in doc.claims
        assert doc.claims["claim-001"]["work_id"] == "TSK-001"
        assert doc._token_counter == 5

    def test_rebuild_with_id_field(self, tmp_path):
        """Should use 'id' field when 'claim_id' is missing."""
        jsonl_path = tmp_path / "sessions.jsonl"
        events = [
            {
                "type": "claim_created",
                "id": "claim-from-id",
                "work_id": "TSK-001",
                "pattern": "src/*.py",
                "owner_id": "agent-1",
            },
        ]
        with open(jsonl_path, "w") as f:
            for event in events:
                f.write(json.dumps(event) + "\n")

        doc = rebuild_from_jsonl(jsonl_path)
        assert "claim-from-id" in doc.claims

    def test_rebuild_ignores_invalid_claim_created(self, tmp_path):
        """Should ignore claim_created without id."""
        jsonl_path = tmp_path / "sessions.jsonl"
        events = [
            {
                "type": "claim_created",
                # No id or claim_id
                "work_id": "TSK-001",
                "pattern": "src/*.py",
            },
        ]
        with open(jsonl_path, "w") as f:
            for event in events:
                f.write(json.dumps(event) + "\n")

        doc = rebuild_from_jsonl(jsonl_path)
        assert doc.claims == {}

    def test_rebuild_ignores_release_for_unknown_claim(self, tmp_path):
        """Should ignore claim_released for non-existent claim."""
        jsonl_path = tmp_path / "sessions.jsonl"
        events = [
            {
                "type": "claim_released",
                "claim_id": "nonexistent-claim",
            },
        ]
        with open(jsonl_path, "w") as f:
            for event in events:
                f.write(json.dumps(event) + "\n")

        doc = rebuild_from_jsonl(jsonl_path)
        assert doc.claims == {}

    def test_rebuild_ignores_renew_for_unknown_claim(self, tmp_path):
        """Should ignore claim_renewed for non-existent claim."""
        jsonl_path = tmp_path / "sessions.jsonl"
        events = [
            {
                "type": "claim_renewed",
                "claim_id": "nonexistent-claim",
                "expires_at": "2026-01-25T14:00:00Z",
            },
        ]
        with open(jsonl_path, "w") as f:
            for event in events:
                f.write(json.dumps(event) + "\n")

        doc = rebuild_from_jsonl(jsonl_path)
        assert doc.claims == {}

    def test_rebuild_with_ts_as_created_at(self, tmp_path):
        """Should use 'ts' field when 'created_at' is missing."""
        jsonl_path = tmp_path / "sessions.jsonl"
        events = [
            {
                "type": "claim_created",
                "claim_id": "claim-001",
                "work_id": "TSK-001",
                "pattern": "src/*.py",
                "owner_id": "agent-1",
                "ts": "2026-01-25T10:00:00Z",
            },
        ]
        with open(jsonl_path, "w") as f:
            for event in events:
                f.write(json.dumps(event) + "\n")

        doc = rebuild_from_jsonl(jsonl_path)
        assert doc.claims["claim-001"]["created_at"] == "2026-01-25T10:00:00Z"

    def test_rebuild_skips_unknown_event_types(self, tmp_path):
        """Should skip events with unknown types."""
        jsonl_path = tmp_path / "sessions.jsonl"
        events = [
            {"type": "unknown_event", "data": "something"},
            {
                "type": "claim_created",
                "claim_id": "claim-001",
                "work_id": "TSK-001",
                "pattern": "src/*.py",
                "owner_id": "agent-1",
            },
        ]
        with open(jsonl_path, "w") as f:
            for event in events:
                f.write(json.dumps(event) + "\n")

        doc = rebuild_from_jsonl(jsonl_path)
        # Only the claim_created event should be processed
        assert len(doc.claims) == 1

    def test_rebuild_with_nonexistent_file(self, tmp_path):
        """Should handle nonexistent file gracefully."""
        missing_file = tmp_path / "nonexistent.jsonl"
        doc = rebuild_from_jsonl(missing_file)
        assert doc.claims == {}


class TestSyncFromCrdt:
    """Tests for _sync_from_crdt method."""

    def test_sync_from_crdt_no_doc(self):
        """_sync_from_crdt should do nothing when _doc is None."""
        doc = CoordinationDoc()
        doc._doc = None
        # Should not raise
        doc._sync_from_crdt()

    def test_sync_from_crdt_when_loro_unavailable(self, monkeypatch):
        """_sync_from_crdt should do nothing when Loro unavailable."""
        import codeflow_py_lib.crdt as crdt_module

        # Simulate Loro being unavailable
        original_loro = crdt_module.loro
        monkeypatch.setattr(crdt_module, "loro", None)

        doc = CoordinationDoc()
        doc._sync_from_crdt()  # Should not raise

        # Restore
        monkeypatch.setattr(crdt_module, "loro", original_loro)

"""
test_crdt_io.py - Tests for CRDT load/save operations.
"""

import json
from datetime import datetime, timedelta

import codeflow_py_lib.crdt as crdt_module
from codeflow_py_lib.crdt import (
    CoordinationDoc,
    load_coordination,
    rebuild_from_jsonl,
    save_coordination,
)


class TestLoadCoordination:
    """Tests for load_coordination function."""

    def test_load_creates_new_doc(self, temp_dir, monkeypatch):
        """Should create new doc when no state exists."""
        state_dir = temp_dir / ".state"
        state_dir.mkdir()
        # Patch get_state_dir in the crdt module where it's used
        monkeypatch.setattr(crdt_module, "get_state_dir", lambda: state_dir)

        doc = load_coordination()

        assert len(doc.claims) == 0

    def test_load_from_json_fallback(self, temp_dir, monkeypatch):
        """Should load from JSON fallback file."""
        state_dir = temp_dir / ".state"
        coord_dir = state_dir / "coordination"
        coord_dir.mkdir(parents=True)
        # Patch get_state_dir in the crdt module where it's used
        monkeypatch.setattr(crdt_module, "get_state_dir", lambda: state_dir)

        # Create JSON state file (the fallback is state.json, not state.loro)
        json_path = coord_dir / "state.json"
        json_path.write_text(
            json.dumps(
                {
                    "claims": {
                        "claim-001": {
                            "id": "claim-001",
                            "work_id": "TSK-001",
                            "pattern": "file:src/*.ts",
                            "status": "active",
                        }
                    },
                    "token_counter": 5,
                }
            )
        )

        doc = load_coordination()

        assert "claim-001" in doc.claims
        assert doc._token_counter == 5


class TestSaveCoordination:
    """Tests for save_coordination function."""

    def test_save_creates_directory(self, temp_dir, monkeypatch):
        """Should create coordination directory if needed."""
        state_dir = temp_dir / ".state"
        state_dir.mkdir()
        monkeypatch.setattr(crdt_module, "get_state_dir", lambda: state_dir)

        doc = CoordinationDoc()
        doc.add_claim(
            claim_id="claim-001",
            work_id="TSK-001",
            pattern="file:src/*.ts",
            owner_id="agent-001",
        )

        save_coordination(doc)

        assert (state_dir / "coordination").exists()

    def test_save_creates_json_fallback(self, temp_dir, monkeypatch):
        """Should save JSON fallback file."""
        state_dir = temp_dir / ".state"
        state_dir.mkdir()
        monkeypatch.setattr(crdt_module, "get_state_dir", lambda: state_dir)

        doc = CoordinationDoc()
        doc.add_claim(
            claim_id="claim-001",
            work_id="TSK-001",
            pattern="file:src/*.ts",
            owner_id="agent-001",
        )

        save_coordination(doc)

        json_path = state_dir / "coordination" / "state.json"
        assert json_path.exists()

        data = json.loads(json_path.read_text())
        assert "claim-001" in data["claims"]

    def test_save_load_roundtrip(self, temp_dir, monkeypatch):
        """Should preserve data through save/load cycle."""
        state_dir = temp_dir / ".state"
        state_dir.mkdir()
        monkeypatch.setattr(crdt_module, "get_state_dir", lambda: state_dir)

        # Create and save doc
        doc1 = CoordinationDoc()
        doc1.add_claim(
            claim_id="claim-001",
            work_id="TSK-001",
            pattern="file:src/*.ts",
            owner_id="agent-001",
            mode="shared",
        )
        doc1._token_counter = 10
        save_coordination(doc1)

        # Load and verify
        doc2 = load_coordination()

        assert "claim-001" in doc2.claims
        assert doc2.claims["claim-001"]["mode"] == "shared"
        assert doc2._token_counter == 10


class TestRebuildFromJsonl:
    """Tests for rebuild_from_jsonl function."""

    def test_rebuild_creates_claims(self, mock_jsonl_ledger):
        """Should create claims from JSONL events."""
        doc = rebuild_from_jsonl(mock_jsonl_ledger)

        assert "claim-001" in doc.claims
        assert "claim-002" in doc.claims

    def test_rebuild_applies_release_events(self, mock_jsonl_ledger):
        """Should apply release events."""
        doc = rebuild_from_jsonl(mock_jsonl_ledger)

        # claim-001 was released in the mock ledger
        assert doc.claims["claim-001"]["status"] == "released"
        # claim-002 should still be active
        assert doc.claims["claim-002"]["status"] == "active"

    def test_rebuild_updates_token_counter(self, mock_jsonl_ledger):
        """Should update token counter from events."""
        doc = rebuild_from_jsonl(mock_jsonl_ledger)

        # Mock ledger has max token of 2
        assert doc._token_counter == 2

    def test_rebuild_with_renew_events(self, temp_dir):
        """Should apply renew events."""
        # Create JSONL with renew event
        ledger_path = temp_dir / "sessions.jsonl"
        now = datetime.utcnow()
        new_expires = (now + timedelta(hours=2)).isoformat() + "Z"

        events = [
            {
                "type": "claim_created",
                "id": "claim-001",
                "work_id": "TSK-001",
                "pattern": "file:src/*.ts",
                "owner_id": "agent-001",
                "fencing_token": 1,
                "expires_at": (now + timedelta(hours=1)).isoformat() + "Z",
            },
            {
                "type": "claim_renewed",
                "claim_id": "claim-001",
                "expires_at": new_expires,
            },
        ]

        with open(ledger_path, "w") as f:
            for event in events:
                f.write(json.dumps(event) + "\n")

        doc = rebuild_from_jsonl(ledger_path)

        assert doc.claims["claim-001"]["expires_at"] == new_expires

    def test_rebuild_empty_file(self, temp_dir):
        """Should handle empty JSONL file."""
        ledger_path = temp_dir / "sessions.jsonl"
        ledger_path.write_text("")

        doc = rebuild_from_jsonl(ledger_path)

        assert len(doc.claims) == 0

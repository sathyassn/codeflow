"""
test_crdt_sync.py - Tests for CRDT sync operations.
"""

from datetime import datetime, timedelta, timezone

import codeflow_py_lib.paths as paths_module
from codeflow_py_lib.crdt import (
    CoordinationDoc,
    load_coordination,
    save_coordination,
)


class TestSyncCrdtToDb:
    """Tests for CRDT to DB sync."""

    def test_sync_inserts_new_claims(self, mock_db_ops, temp_dir, monkeypatch):
        """Should insert new claims from CRDT to DB."""
        state_dir = temp_dir / ".state"
        (state_dir / "coordination").mkdir(parents=True)
        monkeypatch.setattr(paths_module, "get_state_dir", lambda: state_dir)

        # Create CRDT with claims
        doc = CoordinationDoc()
        now = datetime.now(timezone.utc)
        expires = now + timedelta(seconds=600)
        doc._claims = {
            "claim-001": {
                "id": "claim-001",
                "work_id": "TSK-001",
                "pattern": "file:src/*.ts",
                "mode": "exclusive",
                "owner_id": "agent-001",
                "fencing_token": 1,
                "expires_at": expires.isoformat() + "Z",
                "status": "active",
                "created_at": now.isoformat() + "Z",
            }
        }
        save_coordination(doc)

        # Simulate sync to DB
        for claim_id, claim in doc.claims.items():
            existing = mock_db_ops.execute_query(
                "SELECT id FROM work_claims WHERE id = :id", {"id": claim_id}
            )
            if not existing:
                with mock_db_ops.connection() as conn:
                    conn.execute(
                        """
                        INSERT INTO work_claims (id, work_id, pattern, mode, owner_id, fencing_token, expires_at, status, created_at)
                        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                    """,
                        (
                            claim_id,
                            claim["work_id"],
                            claim["pattern"],
                            claim["mode"],
                            claim["owner_id"],
                            claim["fencing_token"],
                            claim["expires_at"],
                            claim["status"],
                            claim["created_at"],
                        ),
                    )
                    conn.commit()

        # Verify claim was inserted
        results = mock_db_ops.execute_query("SELECT * FROM work_claims")
        assert len(results) == 1
        assert results[0]["id"] == "claim-001"

    def test_sync_updates_existing_claims(self, mock_db_ops, temp_dir, monkeypatch):
        """Should update existing claims in DB."""
        state_dir = temp_dir / ".state"
        (state_dir / "coordination").mkdir(parents=True)
        monkeypatch.setattr(paths_module, "get_state_dir", lambda: state_dir)

        now = datetime.now(timezone.utc)
        expires = now + timedelta(seconds=600)

        # Insert existing claim in DB
        with mock_db_ops.connection() as conn:
            conn.execute(
                """
                INSERT INTO work_claims (id, work_id, pattern, mode, owner_id, fencing_token, expires_at, status)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            """,
                (
                    "claim-001",
                    "TSK-001",
                    "file:src/*.ts",
                    "exclusive",
                    "agent-001",
                    1,
                    expires.isoformat() + "Z",
                    "active",
                ),
            )
            conn.commit()

        # Create CRDT with updated claim
        doc = CoordinationDoc()
        doc._claims = {
            "claim-001": {
                "id": "claim-001",
                "work_id": "TSK-001",
                "pattern": "file:src/*.ts",
                "mode": "exclusive",
                "owner_id": "agent-001",
                "fencing_token": 1,
                "expires_at": expires.isoformat() + "Z",
                "status": "released",
            }
        }
        save_coordination(doc)

        # Sync to DB
        for claim_id, claim in doc.claims.items():
            with mock_db_ops.connection() as conn:
                conn.execute(
                    """
                    UPDATE work_claims SET status = ?, expires_at = ? WHERE id = ?
                """,
                    (claim["status"], claim.get("expires_at"), claim_id),
                )
                conn.commit()

        # Verify status was updated
        results = mock_db_ops.execute_query(
            "SELECT status FROM work_claims WHERE id = :id", {"id": "claim-001"}
        )
        assert results[0]["status"] == "released"


class TestSyncDbToCrdt:
    """Tests for DB to CRDT sync."""

    def test_sync_adds_new_claims(self, mock_db_ops, temp_dir, monkeypatch):
        """Should add new claims from DB to CRDT."""
        state_dir = temp_dir / ".state"
        (state_dir / "coordination").mkdir(parents=True)
        monkeypatch.setattr(paths_module, "get_state_dir", lambda: state_dir)

        # Create empty CRDT
        doc = CoordinationDoc()
        save_coordination(doc)

        now = datetime.now(timezone.utc)
        expires = now + timedelta(seconds=600)

        # Insert claim in DB
        with mock_db_ops.connection() as conn:
            conn.execute(
                """
                INSERT INTO work_claims (id, work_id, pattern, mode, owner_id, fencing_token, expires_at, status)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            """,
                (
                    "claim-001",
                    "TSK-001",
                    "file:src/*.ts",
                    "exclusive",
                    "agent-001",
                    1,
                    expires.isoformat() + "Z",
                    "active",
                ),
            )
            conn.commit()

        # Sync from DB to CRDT
        claims = mock_db_ops.execute_query("SELECT * FROM work_claims")
        doc = load_coordination()

        for claim in claims:
            claim_id = claim["id"]
            if claim_id not in doc.claims:
                doc._claims[claim_id] = {
                    "id": claim_id,
                    "work_id": claim.get("work_id"),
                    "pattern": claim.get("pattern"),
                    "mode": claim.get("mode", "exclusive"),
                    "owner_id": claim.get("owner_id"),
                    "fencing_token": claim.get("fencing_token", 0),
                    "expires_at": claim.get("expires_at"),
                    "status": claim.get("status", "active"),
                    "created_at": claim.get("created_at"),
                }

        save_coordination(doc)

        # Verify claim was added to CRDT
        doc = load_coordination()
        assert "claim-001" in doc.claims


class TestBidirectionalSync:
    """Tests for bidirectional sync."""

    def test_bidirectional_sync(self, mock_db_ops, temp_dir, monkeypatch):
        """Should sync in both directions."""
        state_dir = temp_dir / ".state"
        (state_dir / "coordination").mkdir(parents=True)
        monkeypatch.setattr(paths_module, "get_state_dir", lambda: state_dir)

        now = datetime.now(timezone.utc)
        expires = now + timedelta(seconds=600)

        # Create CRDT with one claim
        doc = CoordinationDoc()
        doc._claims = {
            "claim-crdt": {
                "id": "claim-crdt",
                "work_id": "TSK-001",
                "pattern": "file:src/*.ts",
                "mode": "exclusive",
                "owner_id": "agent-001",
                "fencing_token": 1,
                "expires_at": expires.isoformat() + "Z",
                "status": "active",
                "created_at": now.isoformat() + "Z",
            }
        }
        doc._token_counter = 1
        save_coordination(doc)

        # Insert different claim in DB
        with mock_db_ops.connection() as conn:
            conn.execute(
                """
                INSERT INTO work_claims (id, work_id, pattern, mode, owner_id, fencing_token, expires_at, status)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            """,
                (
                    "claim-db",
                    "TSK-002",
                    "dir:src/auth",
                    "shared",
                    "agent-002",
                    2,
                    expires.isoformat() + "Z",
                    "active",
                ),
            )
            conn.commit()

        # Bidirectional sync
        # 1. DB to CRDT
        doc = load_coordination()
        claims = mock_db_ops.execute_query("SELECT * FROM work_claims")
        for claim in claims:
            claim_id = claim["id"]
            if claim_id not in doc.claims:
                doc._claims[claim_id] = {
                    "id": claim_id,
                    "work_id": claim.get("work_id"),
                    "pattern": claim.get("pattern"),
                    "mode": claim.get("mode", "exclusive"),
                    "owner_id": claim.get("owner_id"),
                    "fencing_token": claim.get("fencing_token", 0),
                    "expires_at": claim.get("expires_at"),
                    "status": claim.get("status", "active"),
                }
        save_coordination(doc)

        # 2. CRDT to DB
        doc = load_coordination()
        for claim_id, claim in doc.claims.items():
            existing = mock_db_ops.execute_query(
                "SELECT id FROM work_claims WHERE id = :id", {"id": claim_id}
            )
            if not existing:
                with mock_db_ops.connection() as conn:
                    conn.execute(
                        """
                        INSERT INTO work_claims (id, work_id, pattern, mode, owner_id, fencing_token, expires_at, status)
                        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                    """,
                        (
                            claim_id,
                            claim["work_id"],
                            claim["pattern"],
                            claim.get("mode", "exclusive"),
                            claim["owner_id"],
                            claim.get("fencing_token", 0),
                            claim.get("expires_at"),
                            claim.get("status", "active"),
                        ),
                    )
                    conn.commit()

        # Verify both claims exist in both stores
        doc = load_coordination()
        assert "claim-crdt" in doc.claims
        assert "claim-db" in doc.claims

        results = mock_db_ops.execute_query("SELECT id FROM work_claims ORDER BY id")
        ids = [r["id"] for r in results]
        assert "claim-crdt" in ids
        assert "claim-db" in ids

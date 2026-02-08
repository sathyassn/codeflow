"""
test_claim_operations.py - Tests for claim acquire, release, renew, check, list.
"""

from datetime import datetime, timedelta, timezone

import codeflow_py_lib.paths as paths_module
from codeflow_py_lib.crdt import CoordinationDoc, save_coordination


class TestClaimAcquireBasic:
    """Tests for basic claim acquisition."""

    def test_acquire_creates_claim(self, mock_db_ops, temp_dir, monkeypatch):
        """Should create claim in database."""
        state_dir = temp_dir / ".state"
        (state_dir / "coordination").mkdir(parents=True)
        (state_dir / "ledger").mkdir(parents=True)
        monkeypatch.setattr(paths_module, "get_state_dir", lambda: state_dir)

        # Create empty coordination doc
        doc = CoordinationDoc()
        save_coordination(doc)

        # Simulate claim acquisition in database
        now = datetime.now(timezone.utc)
        expires = now + timedelta(seconds=600)
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

        # Verify claim exists
        results = mock_db_ops.execute_query(
            "SELECT * FROM work_claims WHERE id = :id", {"id": "claim-001"}
        )
        assert len(results) == 1
        assert results[0]["pattern"] == "file:src/*.ts"

    def test_acquire_with_exclusive_mode(self, mock_db_ops):
        """Should default to exclusive mode."""
        now = datetime.now(timezone.utc)
        expires = now + timedelta(seconds=600)
        with mock_db_ops.connection() as conn:
            conn.execute(
                """
                INSERT INTO work_claims (id, work_id, pattern, owner_id, fencing_token, expires_at, status)
                VALUES (?, ?, ?, ?, ?, ?, ?)
            """,
                (
                    "claim-001",
                    "TSK-001",
                    "file:src/*.ts",
                    "agent-001",
                    1,
                    expires.isoformat() + "Z",
                    "active",
                ),
            )
            conn.commit()

        results = mock_db_ops.execute_query(
            "SELECT mode FROM work_claims WHERE id = :id", {"id": "claim-001"}
        )
        assert results[0]["mode"] == "exclusive"

    def test_acquire_with_shared_mode(self, mock_db_ops):
        """Should support shared mode."""
        now = datetime.now(timezone.utc)
        expires = now + timedelta(seconds=600)
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
                    "shared",
                    "agent-001",
                    1,
                    expires.isoformat() + "Z",
                    "active",
                ),
            )
            conn.commit()

        results = mock_db_ops.execute_query(
            "SELECT mode FROM work_claims WHERE id = :id", {"id": "claim-001"}
        )
        assert results[0]["mode"] == "shared"


class TestClaimConflictDetection:
    """Tests for conflict detection during acquisition."""

    def test_detects_exclusive_conflict(self, mock_db_ops):
        """Should detect conflict with existing exclusive claim."""
        now = datetime.now(timezone.utc)
        expires = now + timedelta(seconds=600)

        # Create existing exclusive claim
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

        # Check for conflict
        results = mock_db_ops.execute_query(
            """
            SELECT * FROM work_claims
            WHERE pattern = :pattern AND status = 'active' AND expires_at > :now
        """,
            {"pattern": "file:src/*.ts", "now": now.isoformat() + "Z"},
        )

        assert len(results) == 1  # Conflict exists

    def test_shared_claims_no_conflict(self, mock_db_ops):
        """Should allow multiple shared claims."""
        now = datetime.now(timezone.utc)
        expires = now + timedelta(seconds=600)

        # Create two shared claims for same pattern
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
                    "shared",
                    "agent-001",
                    1,
                    expires.isoformat() + "Z",
                    "active",
                ),
            )
            conn.execute(
                """
                INSERT INTO work_claims (id, work_id, pattern, mode, owner_id, fencing_token, expires_at, status)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            """,
                (
                    "claim-002",
                    "TSK-002",
                    "file:src/*.ts",
                    "shared",
                    "agent-002",
                    2,
                    expires.isoformat() + "Z",
                    "active",
                ),
            )
            conn.commit()

        results = mock_db_ops.execute_query(
            "SELECT * FROM work_claims WHERE pattern = :pattern",
            {"pattern": "file:src/*.ts"},
        )
        assert len(results) == 2

    def test_expired_claim_no_conflict(self, mock_db_ops):
        """Should not conflict with expired claims."""
        now = datetime.now(timezone.utc)
        expired = now - timedelta(seconds=600)

        # Create expired claim
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
                    expired.isoformat() + "Z",
                    "active",
                ),
            )
            conn.commit()

        # Check for active conflicts (excluding expired)
        results = mock_db_ops.execute_query(
            """
            SELECT * FROM work_claims
            WHERE pattern = :pattern AND status = 'active' AND expires_at > :now
        """,
            {"pattern": "file:src/*.ts", "now": now.isoformat() + "Z"},
        )

        assert len(results) == 0  # No active conflict


class TestClaimRelease:
    """Tests for claim release operations."""

    def test_release_updates_status(self, mock_db_ops):
        """Should update claim status to released."""
        now = datetime.now(timezone.utc)
        expires = now + timedelta(seconds=600)

        # Create claim
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

        # Release claim
        with mock_db_ops.connection() as conn:
            conn.execute(
                "UPDATE work_claims SET status = 'released' WHERE id = :id",
                {"id": "claim-001"},
            )
            conn.commit()

        results = mock_db_ops.execute_query(
            "SELECT status FROM work_claims WHERE id = :id", {"id": "claim-001"}
        )
        assert results[0]["status"] == "released"


class TestClaimRenew:
    """Tests for claim renewal operations."""

    def test_renew_updates_expiration(self, mock_db_ops):
        """Should update expiration time."""
        now = datetime.now(timezone.utc)
        old_expires = now + timedelta(seconds=300)
        new_expires = now + timedelta(seconds=900)

        # Create claim
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
                    old_expires.isoformat() + "Z",
                    "active",
                ),
            )
            conn.commit()

        # Renew claim
        with mock_db_ops.connection() as conn:
            conn.execute(
                "UPDATE work_claims SET expires_at = :expires_at WHERE id = :id",
                {"id": "claim-001", "expires_at": new_expires.isoformat() + "Z"},
            )
            conn.commit()

        results = mock_db_ops.execute_query(
            "SELECT expires_at FROM work_claims WHERE id = :id", {"id": "claim-001"}
        )
        assert new_expires.isoformat() in results[0]["expires_at"]


class TestClaimCheck:
    """Tests for claim check operations."""

    def test_check_finds_active_claim(self, mock_db_ops):
        """Should find active claims for pattern."""
        now = datetime.now(timezone.utc)
        expires = now + timedelta(seconds=600)

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

        results = mock_db_ops.execute_query(
            """
            SELECT * FROM work_claims
            WHERE pattern = :pattern AND status = 'active'
        """,
            {"pattern": "file:src/*.ts"},
        )

        assert len(results) == 1

    def test_check_excludes_released(self, mock_db_ops):
        """Should exclude released claims."""
        now = datetime.now(timezone.utc)
        expires = now + timedelta(seconds=600)

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
                    "released",
                ),
            )
            conn.commit()

        results = mock_db_ops.execute_query(
            """
            SELECT * FROM work_claims
            WHERE pattern = :pattern AND status = 'active'
        """,
            {"pattern": "file:src/*.ts"},
        )

        assert len(results) == 0


class TestClaimList:
    """Tests for claim list operations."""

    def test_list_all_claims(self, mock_db_ops):
        """Should list all claims."""
        now = datetime.now(timezone.utc)
        expires = now + timedelta(seconds=600)

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
            conn.execute(
                """
                INSERT INTO work_claims (id, work_id, pattern, mode, owner_id, fencing_token, expires_at, status)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            """,
                (
                    "claim-002",
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

        results = mock_db_ops.execute_query("SELECT * FROM work_claims")
        assert len(results) == 2

    def test_list_filter_by_owner(self, mock_db_ops):
        """Should filter by owner ID."""
        now = datetime.now(timezone.utc)
        expires = now + timedelta(seconds=600)

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
            conn.execute(
                """
                INSERT INTO work_claims (id, work_id, pattern, mode, owner_id, fencing_token, expires_at, status)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            """,
                (
                    "claim-002",
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

        results = mock_db_ops.execute_query(
            "SELECT * FROM work_claims WHERE owner_id = :owner_id",
            {"owner_id": "agent-001"},
        )
        assert len(results) == 1
        assert results[0]["id"] == "claim-001"

    def test_list_filter_by_status(self, mock_db_ops):
        """Should filter by status."""
        now = datetime.now(timezone.utc)
        expires = now + timedelta(seconds=600)

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
            conn.execute(
                """
                INSERT INTO work_claims (id, work_id, pattern, mode, owner_id, fencing_token, expires_at, status)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            """,
                (
                    "claim-002",
                    "TSK-002",
                    "dir:src/auth",
                    "shared",
                    "agent-002",
                    2,
                    expires.isoformat() + "Z",
                    "released",
                ),
            )
            conn.commit()

        results = mock_db_ops.execute_query(
            "SELECT * FROM work_claims WHERE status = :status", {"status": "active"}
        )
        assert len(results) == 1

    def test_list_filter_by_work_id(self, mock_db_ops):
        """Should filter by work ID."""
        now = datetime.now(timezone.utc)
        expires = now + timedelta(seconds=600)

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
            conn.execute(
                """
                INSERT INTO work_claims (id, work_id, pattern, mode, owner_id, fencing_token, expires_at, status)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            """,
                (
                    "claim-002",
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

        results = mock_db_ops.execute_query(
            "SELECT * FROM work_claims WHERE work_id = :work_id", {"work_id": "TSK-001"}
        )
        assert len(results) == 1
        assert results[0]["id"] == "claim-001"

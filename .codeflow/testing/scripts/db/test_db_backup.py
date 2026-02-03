"""
test_db_backup.py - Tests for database backup operations.
"""

import sqlite3
from pathlib import Path

from db_operations import DatabaseConfig, DatabaseOperations


class TestBackup:
    """Tests for backup operation."""

    def test_backup_creates_file(self, temp_dir):
        """Backup should create a file."""
        db_path = str(temp_dir / "test.db")
        backup_path = str(temp_dir / "backup.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("CREATE TABLE items (id INTEGER, data TEXT)")
            conn.execute("INSERT INTO items VALUES (1, 'test')")
            conn.commit()

        result_path = ops.backup(backup_path)

        assert Path(result_path).exists()
        assert result_path == backup_path

    def test_backup_contains_data(self, temp_dir):
        """Backup should contain all data from original."""
        db_path = str(temp_dir / "test.db")
        backup_path = str(temp_dir / "backup.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("CREATE TABLE items (id INTEGER, data TEXT)")
            conn.execute("INSERT INTO items VALUES (1, 'test1')")
            conn.execute("INSERT INTO items VALUES (2, 'test2')")
            conn.execute("INSERT INTO items VALUES (3, 'test3')")
            conn.commit()

        ops.backup(backup_path)

        # Verify backup contents
        backup_conn = sqlite3.connect(backup_path)
        backup_conn.row_factory = sqlite3.Row
        cursor = backup_conn.execute("SELECT COUNT(*) as cnt FROM items")
        count = cursor.fetchone()["cnt"]
        backup_conn.close()

        assert count == 3

    def test_backup_auto_path(self, temp_dir):
        """Backup without path should create timestamped file."""
        db_path = str(temp_dir / "test.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("CREATE TABLE items (id INTEGER)")
            conn.commit()

        backup_path = ops.backup()

        assert Path(backup_path).exists()
        assert "backups" in backup_path
        assert "codeflow-" in backup_path

    def test_backup_with_explicit_path(self, temp_dir):
        """Backup should work with explicitly provided path."""
        db_path = str(temp_dir / "test.db")
        # Note: Backup to explicit path requires parent dir to exist
        # (auto-path mode creates the backup directory automatically)
        backup_dir = temp_dir / "custom_backups"
        backup_dir.mkdir(parents=True)
        backup_path = str(backup_dir / "backup.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("CREATE TABLE items (id INTEGER)")
            conn.commit()

        ops.backup(backup_path)

        assert Path(backup_path).exists()

    def test_backup_preserves_schema(self, temp_dir):
        """Backup should preserve table schema."""
        db_path = str(temp_dir / "test.db")
        backup_path = str(temp_dir / "backup.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("""
                CREATE TABLE items (
                    id INTEGER PRIMARY KEY,
                    name TEXT NOT NULL,
                    value REAL DEFAULT 0.0
                )
            """)
            conn.execute("CREATE INDEX idx_name ON items(name)")
            conn.commit()

        ops.backup(backup_path)

        # Verify schema in backup
        backup_conn = sqlite3.connect(backup_path)
        cursor = backup_conn.execute(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='items'"
        )
        schema = cursor.fetchone()[0]
        backup_conn.close()

        assert "INTEGER PRIMARY KEY" in schema
        assert "NOT NULL" in schema

    def test_backup_preserves_indexes(self, temp_dir):
        """Backup should preserve indexes."""
        db_path = str(temp_dir / "test.db")
        backup_path = str(temp_dir / "backup.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        with ops.connection() as conn:
            conn.execute("CREATE TABLE items (id INTEGER, name TEXT)")
            conn.execute("CREATE INDEX idx_name ON items(name)")
            conn.execute("CREATE INDEX idx_id ON items(id)")
            conn.commit()

        ops.backup(backup_path)

        # Verify indexes in backup
        backup_conn = sqlite3.connect(backup_path)
        cursor = backup_conn.execute(
            "SELECT name FROM sqlite_master WHERE type='index' AND name NOT LIKE 'sqlite%'"
        )
        indexes = [row[0] for row in cursor.fetchall()]
        backup_conn.close()

        assert "idx_name" in indexes
        assert "idx_id" in indexes


class TestBackupRestoration:
    """Tests for backup restoration scenarios."""

    def test_restore_from_backup(self, temp_dir):
        """Should be able to use backup as new database."""
        db_path = str(temp_dir / "original.db")
        backup_path = str(temp_dir / "backup.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        # Create original
        with ops.connection() as conn:
            conn.execute("CREATE TABLE items (id INTEGER, data TEXT)")
            conn.execute("INSERT INTO items VALUES (1, 'original')")
            conn.commit()

        # Create backup
        ops.backup(backup_path)

        # Modify original
        with ops.connection() as conn:
            conn.execute("UPDATE items SET data = 'modified'")
            conn.commit()

        # Open backup and verify it has original data
        backup_config = DatabaseConfig(db_path=backup_path)
        backup_ops = DatabaseOperations(backup_config)
        results = backup_ops.execute_query("SELECT data FROM items WHERE id = 1")

        assert results[0]["data"] == "original"

    def test_backup_is_independent(self, temp_dir):
        """Changes to original should not affect backup."""
        db_path = str(temp_dir / "original.db")
        backup_path = str(temp_dir / "backup.db")
        config = DatabaseConfig(db_path=db_path)
        ops = DatabaseOperations(config)

        # Create original
        with ops.connection() as conn:
            conn.execute("CREATE TABLE items (id INTEGER)")
            conn.execute("INSERT INTO items VALUES (1)")
            conn.commit()

        # Create backup
        ops.backup(backup_path)

        # Add more data to original
        with ops.connection() as conn:
            conn.execute("INSERT INTO items VALUES (2), (3), (4)")
            conn.commit()

        # Original should have 4 rows
        results = ops.execute_query("SELECT COUNT(*) as cnt FROM items")
        assert results[0]["cnt"] == 4

        # Backup should still have 1 row
        backup_conn = sqlite3.connect(backup_path)
        cursor = backup_conn.execute("SELECT COUNT(*) FROM items")
        backup_count = cursor.fetchone()[0]
        backup_conn.close()

        assert backup_count == 1

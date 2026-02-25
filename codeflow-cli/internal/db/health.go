package db

import (
	"context"
	"fmt"
)

// HealthReport contains the results of database health checks.
type HealthReport struct {
	IntegrityOK  bool     `json:"integrity_ok"`
	ForeignKeyOK bool     `json:"foreign_key_ok"`
	Errors       []string `json:"errors,omitempty"`
}

// CheckHealth runs both integrity_check and foreign_key_check and returns a
// structured report instead of failing on the first error.
func (d *DB) CheckHealth(ctx context.Context) (*HealthReport, error) {
	report := &HealthReport{
		IntegrityOK:  true,
		ForeignKeyOK: true,
	}

	// Run integrity_check.
	var result string
	if err := d.db.QueryRowContext(ctx, "PRAGMA integrity_check").Scan(&result); err != nil {
		return nil, fmt.Errorf("db: running integrity check: %w", err)
	}
	if result != "ok" {
		report.IntegrityOK = false
		report.Errors = append(report.Errors, fmt.Sprintf("integrity_check: %s", result))
	}

	// Run foreign_key_check.
	rows, err := d.db.QueryContext(ctx, "PRAGMA foreign_key_check")
	if err != nil {
		return nil, fmt.Errorf("db: running foreign key check: %w", err)
	}
	defer rows.Close()

	for rows.Next() {
		var table, rowID, parent, fkIndex string
		if err := rows.Scan(&table, &rowID, &parent, &fkIndex); err != nil {
			return nil, fmt.Errorf("db: scanning foreign key check result: %w", err)
		}
		report.ForeignKeyOK = false
		report.Errors = append(report.Errors,
			fmt.Sprintf("foreign_key_check: table=%s rowid=%s parent=%s fkidx=%s",
				table, rowID, parent, fkIndex))
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("db: reading foreign key check results: %w", err)
	}

	return report, nil
}

// IsHealthy returns true if both integrity and foreign key checks pass.
func (r *HealthReport) IsHealthy() bool {
	return r.IntegrityOK && r.ForeignKeyOK
}

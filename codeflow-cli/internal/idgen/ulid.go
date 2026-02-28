package idgen

import (
	"crypto/rand"
	"strings"
	"time"

	"github.com/oklog/ulid/v2"
)

// NewULID generates a new ULID string using crypto/rand entropy and the
// current timestamp. The returned string is lowercase, 26 characters,
// Crockford base32 encoded.
func NewULID() string {
	ms := ulid.Timestamp(time.Now())
	id, err := ulid.New(ms, rand.Reader)
	if err != nil {
		// crypto/rand.Reader should never fail on supported platforms.
		// If it does, fall back to ulid.Make which uses a less secure source.
		return strings.ToLower(ulid.Make().String())
	}
	return strings.ToLower(id.String())
}

// NewPrefixedULID generates a new ULID prefixed with the given string,
// separated by a hyphen. For example, NewPrefixedULID("task") returns
// "task-01jm...".
//
// If prefix is empty, the bare ULID is returned without a leading hyphen.
func NewPrefixedULID(prefix string) string {
	id := NewULID()
	if prefix == "" {
		return id
	}
	return prefix + "-" + id
}

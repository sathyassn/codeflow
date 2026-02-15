# Protected Paths Reference

## Protection Tiers

### Tier 1: Core (Highest Protection)

Source: `.codeflow/config/enforcement/enforcement-policy.json`

System-critical paths requiring staging workflow:

```text
.claude/hooks/**
.claude/settings.json
.codeflow/scripts/security/**
.codeflow/config/enforcement/enforcement-policy.json
```

### Tier 2: Extended

Source: `.codeflow/config/enforcement/protection/protected-extended.list`

User-defined paths requiring elevated protection. Configure by editing the list file.

Example entries:

```text
.env
.env.*
**/credentials.json
**/secrets/**
config/production.json
```

### Tier 3: Ad-hoc

Source: `.codeflow/protected-adhoc.list`

Automatically tracked during session based on sensitivity detection. Cleared on session end unless promoted to extended.

## Checking Protection

Use `cf-security-management:validate-protected-resource` to check if a path is protected.

**Never check protection manually.** The operation handles:

- Pattern matching across all tiers
- Priority resolution (Core > Extended > Ad-hoc)
- Returns tier and required workflow

## Pattern Syntax

| Pattern | Matches |
|---------|---------|
| `*` | Any characters except `/` |
| `**` | Any characters including `/` |
| `?` | Single character |
| `[abc]` | Character class |

## Protection Behaviors by Tier

| Tier | Direct Edit | Staging Required | User Approval | Audit Logged |
|------|-------------|------------------|---------------|--------------|
| Core | BLOCKED | Yes | Yes | Yes |
| Extended | BLOCKED | Yes | Yes | Yes |
| Ad-hoc | WARNED | Optional | Recommended | Yes |

## Configuring Extended Paths

Edit `.codeflow/protected-extended.list`:

```text
# Comments start with #
# One pattern per line

# Secrets
.env
.env.*
**/secrets/**

# Production configs
config/production.*
deploy/production/**
```

After editing, run `.codeflow/scripts/security/reload-protection.sh` to apply changes.

## Sensitivity Detection

Patterns triggering ad-hoc protection (detected automatically):

| Pattern | Reason |
|---------|--------|
| `password` | Credential exposure |
| `secret` | Credential exposure |
| `api_key` | Credential exposure |
| `private_key` | Cryptographic material |
| `BEGIN RSA` | Cryptographic material |
| `token` | Auth token exposure |

## Promoting Ad-hoc to Extended

If an ad-hoc protected file should be permanently protected:

```bash
.codeflow/scripts/security/promote-protection.sh "{path}"
```

This adds the path to protected-extended.list.

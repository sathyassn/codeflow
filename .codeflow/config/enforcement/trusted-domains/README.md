# Trusted Domains for WebFetch

**Purpose:** Curated domain allowlists for WebFetch validation by approval mode.

**Security Layer:** L2 (PreToolUse Hook)

**Version:** 1.2.0

**Last Updated:** 2025-12-16

---

## Overview

These domain lists control which URLs Claude Code can automatically fetch via WebFetch.
The PreToolUse hook validates each WebFetch request against the appropriate list
based on the active approval mode.

**Key principle:** No blanket `domain:*` access. Every domain is explicitly vetted.

---

## Domain Lists

| File | Mode | Domains | Description |
|------|------|---------|-------------|
| `standard.list` | Standard | ~10 | Core development domains only |
| `autonomous.list` | Autonomous | ~40 | Extended domains for experienced developers |
| `permissive.list` | Permissive | ~80 | Broad but curated list for CI/CD |

**Strict mode:** All WebFetch requests require approval (no allowlist).

---

## File Format

- One domain per line
- Comments start with `#`
- Empty lines ignored
- Case-insensitive matching
- **Automatic subdomain matching** (see Best Practices below)

Example:

```text
# Anthropic / Claude (base domain enables all subdomains)
anthropic.com

# Documentation
developer.mozilla.org

# Code Hosting
github.com
```

---

## Best Practices for Adding Domains

### 1. Use Base Domains for Subdomain Coverage

The hook walks up the domain hierarchy. If the base domain is in the list,
ALL subdomains are automatically allowed.

```text
✅ CORRECT - Use base domain when you want all subdomains:
anthropic.com           # Allows: www, docs, console, api, support, etc.
github.com              # Allows: api.github.com, raw.githubusercontent.com, etc.

❌ WRONG - Don't list subdomains redundantly:
www.anthropic.com       # Only allows www.anthropic.com, NOT docs.anthropic.com
docs.anthropic.com      # Redundant if anthropic.com is already listed
api.anthropic.com       # Redundant if anthropic.com is already listed
```

### 2. Use Specific Subdomains When Appropriate

Use specific subdomains when you want to allow only that subdomain, not the entire domain:

```text
✅ CORRECT - Specific subdomain for limited access:
developer.mozilla.org   # Only MDN, not all of mozilla.org
docs.python.org         # Only docs, not all of python.org
engineering.fb.com      # Only eng blog, not all of fb.com

❌ WRONG - Base domain when you only need a subdomain:
mozilla.org             # Too broad - includes everything
python.org              # Too broad - includes non-doc content
fb.com                  # Too broad - includes social media
```

### 3. Never Include Paths

The hook matches domains only. Paths are ignored and cause invalid entries.

```text
✅ CORRECT - Domain only:
aws.amazon.com

❌ WRONG - Path included (will not work correctly):
aws.amazon.com/blogs
docs.docker.com/engine/install
```

### 4. Verify Domain Ownership

Before adding a domain, verify:

- Who owns/operates the domain
- Whether it's the official/primary domain
- Check for redirects (e.g., docs.anthropic.com → docs.claude.com)

### 5. Choose the Right List

| Add to... | When... |
|-----------|---------|
| `standard.list` | Essential for all developers (docs, GitHub, npm) |
| `autonomous.list` | Useful for experienced devs (cloud docs, more registries) |
| `permissive.list` | Useful for CI/CD or advanced workflows |

### How Subdomain Matching Works

The hook (cf-pre-tool-use-webfetch.sh) implements domain hierarchy walking:

```text
Request URL: https://docs.anthropic.com/api-reference

1. Extract domain: docs.anthropic.com
2. Check exact match: docs.anthropic.com → Not in list
3. Walk up hierarchy: anthropic.com → Found! ✅ ALLOW

Request URL: https://evil.example.com

1. Extract domain: evil.example.com
2. Check exact match: evil.example.com → Not in list
3. Walk up hierarchy: example.com → Not in list
4. No more parents → ❌ ASK (trigger approval prompt)
```

---

## Adding Domains Checklist

1. [ ] Identify the appropriate list based on domain category
2. [ ] Determine if base domain or specific subdomain is appropriate
3. [ ] Verify domain ownership and check for redirects
4. [ ] Add domain to the correct `.list` file (domain only, no paths)
5. [ ] Include a comment explaining the category
6. [ ] Remove any redundant subdomains now covered by base domain
7. [ ] Test with `testing/security/test-security-allows.sh`

**Criteria for inclusion:**

- Official documentation sites
- Major code hosting platforms
- Established package registries
- Trusted technical communities
- Security resources (OWASP, CVE, NIST)

**Never add:**

- User-generated content platforms without curation
- File sharing services
- URL shorteners
- Unknown or untrusted domains

---

## Hook Integration

The `pre-tool-use-webfetch.sh` hook validates WebFetch requests. The hook reads its
configuration from the `_web_fetch_config` object in settings files.

### Hook Configuration in settings.json

```json
{
  "_web_fetch_config": {
    "_note": "WebFetch hook reads this config. Local settings override project settings.",
    "approvalMode": "standard",
    "untrustedAction": "ask"
  },
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "WebFetch",
        "hooks": [
          {
            "type": "command",
            "command": "bash .claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-webfetch.sh",
            "timeout": 2
          }
        ]
      }
    ]
  }
}
```

### Configuration Options

| Option | Values | Description |
|--------|--------|-------------|
| `approvalMode` | `strict`, `standard`, `autonomous`, `permissive` | Which domain list to use |
| `untrustedAction` | `ask`, `block` | What to do for untrusted domains |

### Configuration Precedence

1. `.claude/settings.local.json` (highest - user override)
2. `.claude/settings.json` (project default)
3. Hook defaults: `standard` mode, `ask` action

**Why this approach:** Claude Code hooks MERGE across settings files (both run), but the
hook reads configuration from the JSON files directly, allowing local settings to override
project settings for the approval mode.

---

## Maintenance

**When to review:**

- Quarterly domain audit
- After major framework/tool changes
- When adding new development stack support

**Review checklist:**

- [ ] All domains still active and relevant
- [ ] No domains have changed ownership
- [ ] No security incidents reported for listed domains
- [ ] New essential domains identified

---

## Related Files

- Hook script: `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-webfetch.sh`
- Documentation: `docs/security/workflow-approval-modes.md`
- Templates: `.claude/settings-templates/*.json`

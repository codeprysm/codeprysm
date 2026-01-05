# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |

## Reporting a Vulnerability

We take security vulnerabilities seriously. If you discover a security issue, please report it responsibly.

### How to Report

**Please do NOT open a public GitHub issue for security vulnerabilities.**

Instead, send an email to: **team@codeprysm.io**

Include the following information:
- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Any suggested fixes (optional)

### What to Expect

- **Acknowledgment**: We will acknowledge receipt of your report within 48 hours.
- **Assessment**: We will assess the vulnerability and determine its severity.
- **Updates**: We will keep you informed of our progress.
- **Resolution**: We aim to resolve critical vulnerabilities within 30 days.
- **Credit**: We will credit you in the security advisory (unless you prefer to remain anonymous).

### Scope

This security policy applies to:
- The CodePrism codebase (all crates)
- Official Docker images
- Documentation that could lead to security issues

### Out of Scope

- Vulnerabilities in dependencies (please report these to the respective projects)
- Social engineering attacks
- Physical security issues

## Security Best Practices

When using CodePrism:

1. **Keep Updated**: Always use the latest version
2. **Environment Variables**: Never commit API keys or secrets
3. **Network Security**: When running the MCP server, ensure proper network isolation
4. **Qdrant Security**: Configure Qdrant with appropriate authentication in production

## Disclosure Policy

- We follow coordinated disclosure practices
- We will work with you to understand and resolve the issue
- We will not take legal action against researchers who follow this policy
- We will publicly acknowledge your contribution (with your permission)

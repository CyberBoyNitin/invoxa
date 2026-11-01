# Security Policy

## Supported versions

Security fixes are applied to the current `main` branch. This project is a working MVP and does not yet have long-term support releases.

## Reporting a vulnerability

Please **do not** open a public GitHub issue for security vulnerabilities.

Use GitHub's private vulnerability reporting:

1. Open the repository's **Security** tab.
2. Choose **Advisories**.
3. Click **Report a vulnerability**.

Include:

- A description of the issue and its impact.
- Steps to reproduce or a minimal proof of concept.
- Affected routes, components, or configuration.
- Whether tenant isolation, authentication, approval, exports, or webhook delivery is involved.

Please avoid including real customer invoices, credentials, or personal data in a report. Use synthetic fixtures.

## Scope

In scope:

- Cross-tenant data access.
- Authentication, session, CSRF, API-key, and authorization flaws.
- Approval or export policy bypasses.
- SSRF or signature/replay weaknesses in webhook delivery.
- Unsafe file handling or path traversal.
- Secret leakage in logs or API responses.

Out of scope for this MVP unless it demonstrates a concrete security impact:

- Missing production hardening that is already documented in [`docs/SECURITY.md`](docs/SECURITY.md), such as the absence of a malware scanner or S3/KMS adapter.
- Denial-of-service reports without a practical reproduction.
- Findings that require a compromised host or database.

## Security documentation

See [`docs/SECURITY.md`](docs/SECURITY.md) for the implemented controls, tested properties, and known production gaps.

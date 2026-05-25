# Security Policy

## Reporting Vulnerabilities

Please report vulnerabilities privately through GitHub Security Advisories for this repository. Do not open public issues for exploitable bugs, secret exposure, local data leakage, or bridge/native messaging bypasses.

Include:

- Affected version or commit.
- Operating system.
- Reproduction steps.
- Impact and any known workarounds.

## Supported Versions

The project is pre-1.0. Security fixes target the latest `main` branch unless a release branch is explicitly maintained.

## Security Expectations

- Real API keys must never be committed.
- Local capture databases and generated exports must not be committed.
- Browser/editor native bridges must validate payload size and schema before ingestion.
- AI context must be redacted before provider calls when redaction is enabled.


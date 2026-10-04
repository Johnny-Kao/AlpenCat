# Security Policy

AlpenCat is an experimental runtime and is not yet recommended for
production-critical workloads.

## Reporting

Please do not publish a suspected security vulnerability in a public issue.

Use the repository's private vulnerability-reporting or security-advisory
mechanism when available. If no private reporting channel is available, contact
the maintainer privately through the repository owner profile before disclosing
technical details publicly.

## Scope

Security-relevant areas include:

- unsafe FFI boundaries
- memory safety
- data races and synchronization
- backend fallback behavior
- malformed task metadata
- GPU buffer sizing and transfer boundaries
- dependency and supply-chain integrity
- execution-budget or nested-parallelism bypasses

Performance regressions without a safety impact should be reported as ordinary
bugs rather than security issues.

## Support status

Only the latest development state is currently evaluated. No long-term security
support policy exists for v0.x releases.

# Security policy

## Reporting a vulnerability

Please report security issues privately with GitHub's "Report a vulnerability" (private
vulnerability reporting) on this repository, or email **alex@cloudlabworks.dev**. Do not open
a public issue.

Include what you found, how to reproduce it, and the commit or the live endpoint involved. You
will get an acknowledgement within 7 days. Fixes ship to `main` and to
https://api.cloudlabworks.dev, and the advisory credits the reporter unless you ask otherwise.

## Scope

- The code in this repository: the Worker, the explorer app and the Rust crates.
- The live API at https://api.cloudlabworks.dev. Please test against your own
  `wrangler dev --local` instance where you can, and stay within the published rate limits
  on the live API. Load or denial-of-service testing against it is out of scope.

The NC2 permission lists are copied from public Nutanix documentation; report errors in the
lists themselves as ordinary issues.

## Supported versions

Only `main` and the live deployment receive security fixes.

Organization-wide contact: https://cloudlabworks.dev/.well-known/security.txt

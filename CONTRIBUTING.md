# Contributing to clw-api

Small, focused pull requests are welcome. Open an issue first for anything larger than a fix.

## Ground rules

- The API stays free, unauthenticated and stateless: no storage, no secrets, no cookies, no
  tracking. A change that needs any of these is a design discussion, not a pull request.
- The receipt checker stays deterministic: no model calls, no network, same input gives the
  same output.
- Permission lists come only from public cloud-vendor documentation. Keep the source URL, the
  page's last-updated date and the fetch date with every list.
- Update `openapi/openapi.yaml` with any route or response change, then run
  `npm run build:openapi` in `worker/`. The tests fail when the served spec drifts.

## Checks

Run the same checks as CI before you open a pull request; the README lists the commands.

## Developer Certificate of Origin

Every commit must be signed off under the [Developer Certificate of Origin](https://developercertificate.org/).
Sign off with `git commit -s`, which adds a line like this to the commit message:

```
Signed-off-by: Your Name <you@example.org>
```

By signing off you certify that you wrote the change, or otherwise have the right to submit it
under the project's MIT license.

# Contributing to Masthead

Bug reports, hardware reports and pull requests are welcome at
[github.com/kitefold-labs/masthead](https://github.com/kitefold-labs/masthead).

## Reporting a bug

Include the output of `masthead-daemon --print` (it lists your hardware and
one sample of every reading; remove anything you consider private), your
desktop and version, and what you expected to see.

Wrong or missing sensor and GPU readings are the most useful reports: the
probe code is tested against fixtures, and real hardware is the only way to
find what it misses.

## Pull requests

- Run `just lint test` before pushing. CI runs the same checks plus the
  resource-budget soak test; see [docs/development.md](docs/development.md).
- Keep the resting budget: no new work in the 1 Hz path unless a face shows
  it, no allocation in steady-state sampling, no disk writes.
- New readings go into `masthead-probe` with a fixture test, then into the
  daemon's detail JSON, then into the faces.
- Sign off your commits (`git commit -s`) to certify the
  [Developer Certificate of Origin](https://developercertificate.org/).

By contributing you agree that your work is licensed under the
[Apache License 2.0](LICENSE).

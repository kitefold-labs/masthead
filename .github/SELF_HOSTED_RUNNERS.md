<!-- Copyright 2026 Kitefold Labs; SPDX-License-Identifier: Apache-2.0 -->
# CI execution on ghost

Workflows select `[self-hosted, linux, x64, kitefold-ghost, ephemeral]`.
The local controller on ghost creates a fresh Ubuntu guest and repository-scoped
one-job GitHub runner for each assignment. GitHub still orchestrates CI and stores
its logs/artifacts; compilation, testing and packaging execute on the PC.

The controller exposes neither the desktop home directory nor its Docker socket
to jobs. Docker services/build tools run inside the disposable guest. Host GitHub
registration credentials stay outside the guest. Repository tokens, environments,
manual release approvals and existing test/coverage gates keep their normal scope.

When ghost is off or its worker is occupied, jobs queue; there is no automatic
GitHub-hosted fallback. Inspect `systemctl --user status kitefold-ci.service` on
ghost and the private controller logs under `kitefold-ci-setup/state` to diagnose
availability. Jobs rotate across the configured Kitefold repositories. KVM is used
when available; software emulation otherwise trades execution speed for isolation.

Revert this migration's runs-on lines to the original Ubuntu labels before
stopping the controller for an extended outage. Do not bypass required checks or
weaken coverage thresholds to accommodate the runner change. Production releases
remain governed by their existing triggers and environment approvals.

GitHub-managed Dependabot updates for public repositories cannot use self-hosted
runners; those platform-managed jobs remain on GitHub. See
https://docs.github.com/en/code-security/concepts/supply-chain-security/dependabot-on-actions.

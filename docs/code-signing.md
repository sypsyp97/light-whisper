# Code signing policy

## Current status

The published v1.7.4 Windows installer is unsigned. A free SignPath Foundation
application was submitted on October 7, 2026. Review is pending; no signing
service is active for this project. This page does not imply that existing
downloads have a trusted signature.

If accepted, the intended attribution is: Free code signing provided by
[SignPath.io](https://signpath.io/), certificate by
[SignPath Foundation](https://signpath.org/).
The certificate publisher would be SignPath Foundation.

## Responsibilities and release requirements

[sypsyp97](https://github.com/sypsyp97), the repository owner, is the proposed
author, reviewer and signing approver. External contributions must be reviewed
before release. Source control and SignPath access must use multi-factor
authentication before production signing is enabled. This is a requirement,
not a claim that account configuration has already been verified.

Signing is intended for the Light-Whisper application and its Windows installer.
Upstream libraries must not be signed under this project's subscription.
Their licenses and notices remain in [Third-party notices](../THIRD_PARTY_NOTICES.md).
The NVIDIA CUDA and Microsoft runtime redistribution terms must be disclosed
to SignPath for an eligibility decision; model weights are downloaded separately.

The existing [release procedure](releasing.md) builds installers locally and
requires checks for the exact candidate commit. For SignPath, the application,
native runtime and installer build must first move to GitHub-hosted runners.
Uploading a locally built installer to a workflow does not establish build origin.
Existing checks and candidate-commit gates must still pass.

Each signing request requires maintainer approval. The signed application's
product and version metadata must match the release. Sign the application
before packaging it, then sign and verify the final installer. Publication must
verify Windows Authenticode trust, timestamp and the uploaded asset's SHA-256.
Do not replace an existing release asset without separate release authorization.

User data handling and third-party services are described in the
[Privacy policy](privacy.md).

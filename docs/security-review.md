# Initial dependency and container review

This is a preparation record dated 2026-09-11, not permanent publication clearance.
The runtime base was `gcr.io/distroless/cc-debian12:nonroot` at digest
`sha256:9dac0a79194e45a7da0158a9c6da57b217585af0786db3845d1f0ec1a0dd182f`.
Trivy 0.74.0 reported no high/critical findings under its selected vendor
severity policy. Its full CycloneDX report includes other vendors' ratings,
which can differ. Do not treat the gate as a statement that all databases assign
low severity. The build artifacts retain the full report for assessment.

The source dependencies for the supported Linux target declare MIT, Apache-2.0,
BSD, ISC, Unicode data, Zlib, and MIT-0 licensing, often as alternatives. Cargo
Deny checks the explicit allowlist and unknown sources. This does not complete
third-party notice packaging; preserve notices before distributing release
artifacts publicly. No advisory ignore list has been added.

## Container findings

These dispositions apply to the reviewed preparation snapshot. They do not
suppress scanner results. The low/medium findings below must be revisited if
native interfaces, TLS providers, target architecture, or the image changes.
Source inspection used the workspace manifests and the bounded API/tool paths;
it does not prove that every transitive native path is unreachable. Confirmed
credential, authentication, injection or cryptographic defects must be fixed,
not accepted through this table.

| Finding | Disposition |
| --- | --- |
| [CVE-2010-4756](https://security-tracker.debian.org/tracker/CVE-2010-4756) | No direct application use of the affected libc interface was found. Accept for private preparation only; transitive/native-library reachability is not certified. Reassess against an updated base before public release. |
| [CVE-2018-20796](https://security-tracker.debian.org/tracker/CVE-2018-20796) | No direct application use of the affected libc interface was found. Accept for private preparation only; transitive/native-library reachability is not certified. Reassess against an updated base before public release. |
| [CVE-2019-1010022](https://security-tracker.debian.org/tracker/CVE-2019-1010022) | Accept for private preparation: defense-in-depth/ASLR findings require a separate memory-corruption or local observation path. No claim of universal non-exploitability; review again before public release. |
| [CVE-2019-1010023](https://security-tracker.debian.org/tracker/CVE-2019-1010023) | The server does not execute ldd or load caller-provided ELF files. Retain the finding; arbitrary command and filesystem execution are outside its tool surface. |
| [CVE-2019-1010024](https://security-tracker.debian.org/tracker/CVE-2019-1010024) | Accept for private preparation: defense-in-depth/ASLR findings require a separate memory-corruption or local observation path. No claim of universal non-exploitability; review again before public release. |
| [CVE-2019-1010025](https://security-tracker.debian.org/tracker/CVE-2019-1010025) | Accept for private preparation: defense-in-depth/ASLR findings require a separate memory-corruption or local observation path. No claim of universal non-exploitability; review again before public release. |
| [CVE-2019-9192](https://security-tracker.debian.org/tracker/CVE-2019-9192) | No direct application use of the affected libc interface was found. Accept for private preparation only; transitive/native-library reachability is not certified. Reassess against an updated base before public release. |
| [CVE-2022-27943](https://security-tracker.debian.org/tracker/CVE-2022-27943) | The runtime does not invoke GNU demangling tools on caller input. Retain the finding and refresh the base image. |
| [CVE-2025-27587](https://security-tracker.debian.org/tracker/CVE-2025-27587) | Not used by the application TLS/JWT path: the workspace uses rustls and Rust cryptography, not OpenSSL CMP, CMS, DTLS or EVP APIs. Retain visibility and refresh the base image; not a blanket exception for future OpenSSL usage. |
| [CVE-2026-18374](https://security-tracker.debian.org/tracker/CVE-2026-18374) | No direct application use of the affected libc interface was found. Accept for private preparation only; transitive/native-library reachability is not certified. Reassess against an updated base before public release. |
| [CVE-2026-19499](https://security-tracker.debian.org/tracker/CVE-2026-19499) | No direct application use of the affected libc interface was found. Accept for private preparation only; transitive/native-library reachability is not certified. Reassess against an updated base before public release. |
| [CVE-2026-19542](https://security-tracker.debian.org/tracker/CVE-2026-19542) | No direct application use of the affected libc interface was found. Accept for private preparation only; transitive/native-library reachability is not certified. Reassess against an updated base before public release. |
| [CVE-2026-42767](https://security-tracker.debian.org/tracker/CVE-2026-42767) | Not used by the application TLS/JWT path: the workspace uses rustls and Rust cryptography, not OpenSSL CMP, CMS, DTLS or EVP APIs. Retain visibility and refresh the base image; not a blanket exception for future OpenSSL usage. |
| [CVE-2026-5435](https://security-tracker.debian.org/tracker/CVE-2026-5435) | No direct application use of the affected libc interface was found. Accept for private preparation only; transitive/native-library reachability is not certified. Reassess against an updated base before public release. |
| [CVE-2026-5450](https://security-tracker.debian.org/tracker/CVE-2026-5450) | No direct application use of the affected libc interface was found. Accept for private preparation only; transitive/native-library reachability is not certified. Reassess against an updated base before public release. |
| [CVE-2026-54874](https://security-tracker.debian.org/tracker/CVE-2026-54874) | Not used by the application TLS/JWT path: the workspace uses rustls and Rust cryptography, not OpenSSL CMP, CMS, DTLS or EVP APIs. Retain visibility and refresh the base image; not a blanket exception for future OpenSSL usage. |
| [CVE-2026-5928](https://security-tracker.debian.org/tracker/CVE-2026-5928) | No direct application use of the affected libc interface was found. Accept for private preparation only; transitive/native-library reachability is not certified. Reassess against an updated base before public release. |
| [CVE-2026-6238](https://security-tracker.debian.org/tracker/CVE-2026-6238) | No direct application use of the affected libc interface was found. Accept for private preparation only; transitive/native-library reachability is not certified. Reassess against an updated base before public release. |
| [CVE-2026-63072](https://security-tracker.debian.org/tracker/CVE-2026-63072) | Not used by the application TLS/JWT path: the workspace uses rustls and Rust cryptography, not OpenSSL CMP, CMS, DTLS or EVP APIs. Retain visibility and refresh the base image; not a blanket exception for future OpenSSL usage. |
| [CVE-2026-63074](https://security-tracker.debian.org/tracker/CVE-2026-63074) | Not used by the application TLS/JWT path: the workspace uses rustls and Rust cryptography, not OpenSSL CMP, CMS, DTLS or EVP APIs. Retain visibility and refresh the base image; not a blanket exception for future OpenSSL usage. |
| [CVE-2026-63076](https://security-tracker.debian.org/tracker/CVE-2026-63076) | Not used by the application TLS/JWT path: the workspace uses rustls and Rust cryptography, not OpenSSL CMP, CMS, DTLS or EVP APIs. Retain visibility and refresh the base image; not a blanket exception for future OpenSSL usage. |
| [CVE-2026-6368](https://security-tracker.debian.org/tracker/CVE-2026-6368) | No direct application use of the affected libc interface was found. Accept for private preparation only; transitive/native-library reachability is not certified. Reassess against an updated base before public release. |
| [CVE-2026-6791](https://security-tracker.debian.org/tracker/CVE-2026-6791) | No direct application use of the affected libc interface was found. Accept for private preparation only; transitive/native-library reachability is not certified. Reassess against an updated base before public release. |
| [CVE-2026-75803](https://security-tracker.debian.org/tracker/CVE-2026-75803) | Not used by the application TLS/JWT path: the workspace uses rustls and Rust cryptography, not OpenSSL CMP, CMS, DTLS or EVP APIs. Retain visibility and refresh the base image; not a blanket exception for future OpenSSL usage. |
| [CVE-2026-77117](https://security-tracker.debian.org/tracker/CVE-2026-77117) | No direct application use of the affected libc interface was found. Accept for private preparation only; transitive/native-library reachability is not certified. Reassess against an updated base before public release. |
| [CVE-2026-80489](https://security-tracker.debian.org/tracker/CVE-2026-80489) | No direct application use of the affected libc interface was found. Accept for private preparation only; transitive/native-library reachability is not certified. Reassess against an updated base before public release. |

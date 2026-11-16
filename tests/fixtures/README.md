# Test fixtures

Invented FHIR payloads for `tests/e2e.rs`, served through medical-common's
mock server. Nothing here was captured from Kaiser or describes a real
person.

| File | Stands for |
|---|---|
| `long-id-eob-searchset.json` | An `ExplanationOfBenefit` search whose claim names a practitioner by an id longer than FHIR R4's 64 characters, as Epic's unconstrained ids are. `PATIENT_ID` is replaced with the mock's patient |
| `long-id-practitioner.json` | That practitioner |
| `refused-id-eob-searchset.json` | An `ExplanationOfBenefit` search whose claim names a practitioner by something FHIR does not allow as an id (it has spaces) |
